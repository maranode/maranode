//! maranode daemon binary (maranoded)

mod admin;
mod baseline_check;
mod config;
mod lifecycle;
mod probe;
mod reload;
mod shutdown;
mod unix_serve;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use clap::Parser;
use tracing::info;

use maranode_attestation::{seal, unseal, is_sealed, detect_tee, get_tee_report};
use maranode_api::state::Stats;
use maranode_api::{build_router, new_oidc_pending, runtime::new_shared, AppState, ChangeManagementConfig, DlpConfig, EngineEmbedder, IncidentHandle};
use maranode_api::dlp::{ForcepointCfg, PurviewCfg, SymantecCfg};
use maranode_api::incident::{load_incident_on_start, new_incident_handle};
use maranode_common::classification::ClassificationPolicy;
use maranode_audit::AuditLog;
use maranode_common::events::AuditEvent;
use maranode_common::models::ModelId;
use maranode_common::types::AirGapMode;
use maranode_common::user::{AuthProvider, Role, User};
use maranode_inference::engine::InferenceEngine;
use maranode_inference::{DevicePreference, InferenceQueue, LlamaCppEngine};
use maranode_isolation::{IsolationConfig, Isolator, WhitelistEntry};
use maranode_rag::RagEngine;
use maranode_store::{kek, maybe_bootstrap, BootstrapOptions, ModelStore, UserDb, WorkspaceDb};

use crate::config::DaemonConfig;
use crate::reload::{runtime_from_config, ReloadServices, StartupPins, StartupSnapshot};

#[derive(Parser, Debug)]
#[command(name = "maranoded", about = "Maranode runtime daemon", version)]
struct Args {
    #[arg(long, env = "MARANODE_CONFIG")]
    config: Option<PathBuf>,

    #[arg(long)]
    no_isolation: bool,

    #[arg(long, env = "MARANODE_DATA_DIR")]
    data_dir: Option<PathBuf>,

    #[arg(long, env = "MARANODE_BIND")]
    bind: Option<String>,

    #[arg(long, env = "RUST_LOG")]
    log_level: Option<String>,

    #[arg(long, env = "MARANODE_DEVICE")]
    device: Option<String>,

    #[arg(long, env = "MARANODE_RAG")]
    rag: bool,

    #[arg(long, env = "MARANODE_EMBEDDING_MODEL")]
    embedding_model: Option<String>,

    #[arg(long, env = "MARANODE_RAG_COLLECTION")]
    rag_collection: Option<String>,

    #[arg(long, env = "MARANODE_ADMIN_KEY")]
    admin_key: Option<String>,

    #[arg(long, env = "MARANODE_SKIP_BOOTSTRAP")]
    skip_bootstrap: bool,

    #[arg(long, env = "MARANODE_YES_BOOTSTRAP")]
    yes_bootstrap: bool,

    #[arg(long, env = "MARANODE_MODELS_DIR")]
    models_dir: Option<PathBuf>,

    #[arg(long, env = "MARANODE_UNIX_SOCKET")]
    unix_socket: Option<String>,

    #[arg(long)]
    no_unix_socket: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let (mut cfg, cfg_path) = DaemonConfig::discover_or_explicit(args.config.as_deref())?;

    let pins = StartupPins {
        data_dir: args.data_dir.clone(),
        bind: args.bind.clone(),
        log_level: args.log_level.clone(),
        device: args.device.clone(),
        unix_socket: if args.no_unix_socket {
            Some(None)
        } else if let Some(s) = &args.unix_socket {
            Some(if s.is_empty() { None } else { Some(s.clone()) })
        } else {
            None
        },
        no_isolation: args.no_isolation,
        rag_enabled: args.rag.then_some(true),
        embedding_model: args.embedding_model.clone(),
        rag_collection: args.rag_collection.clone(),
        admin_key: args.admin_key.clone(),
        models_dir: args.models_dir.clone(),
    };

    if let Some(d) = args.data_dir {
        cfg.data_dir = d;
    }
    if let Some(b) = args.bind {
        cfg.bind = b;
    }
    if let Some(l) = args.log_level {
        cfg.log_level = l;
    }
    if let Some(d) = args.device {
        cfg.device = d;
    }
    if let Some(s) = args.unix_socket {
        cfg.unix_socket = if s.is_empty() { None } else { Some(s) };
    }
    if args.no_unix_socket {
        cfg.unix_socket = None;
    }
    if args.no_isolation || !cfg!(target_os = "linux") {
        cfg.isolation.mode = AirGapMode::Disabled;
    }
    if args.rag {
        cfg.rag.enabled = true;
    }
    if let Some(m) = args.embedding_model {
        cfg.rag.embedding_model = m;
    }
    if let Some(c) = args.rag_collection {
        cfg.rag.default_collection = c;
    }
    if let Some(k) = args.admin_key {
        cfg.auth.admin_key = Some(k);
    }
    if let Some(m) = args.models_dir {
        cfg.models_dir = m;
    }

    let snapshot = StartupSnapshot {
        bind: cfg.bind.clone(),
        data_dir: cfg.data_dir.clone(),
        device: cfg.device.clone(),
        rag_enabled: cfg.rag.enabled,
        embedding_model: cfg.rag.embedding_model.clone(),
        unix_socket: cfg.unix_socket.clone(),
        max_parallel: cfg.inference.max_parallel,
    };

    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

    let env_filter = EnvFilter::try_new(&cfg.log_level).unwrap_or_else(|_| EnvFilter::new("info"));
    let (filter_layer, log_reload) = tracing_subscriber::reload::Layer::new(env_filter);
    tracing_subscriber::registry()
        .with(filter_layer)
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .init();

    info!("maranoded v{} starting", env!("CARGO_PKG_VERSION"));

    match &cfg_path {
        Some(p) => info!("Config loaded from {}", p.display()),
        None => info!("No config file found: using built-in defaults"),
    }

    std::fs::create_dir_all(&cfg.data_dir)?;

    let store = ModelStore::open(&cfg.data_dir)?;
    info!("Model store opened at {}", cfg.data_dir.display());

    maybe_bootstrap(
        &store,
        &BootstrapOptions {
            models_dir: cfg.models_dir.clone(),
            skip: args.skip_bootstrap,
            yes: args.yes_bootstrap,
        },
    )
    .await
    .context("first-run model bootstrap")?;

    let audit = open_audit_log(&cfg)?;
    info!("Audit log opened (seq={})", audit.seq().await);

    let iso_cfg = IsolationConfig {
        mode: cfg.isolation.mode.clone(),
        api_port: cfg.isolation.api_port,
        api_allowed_sources: cfg.isolation.allowed_sources.clone(),
        whitelist: cfg
            .isolation
            .whitelist
            .iter()
            .map(|e| WhitelistEntry {
                host: e.host.clone(),
                port: e.port,
                comment: e.comment.clone(),
            })
            .collect(),
    };
    let isolator = Arc::new(Mutex::new(Isolator::new(iso_cfg)));
    isolator.lock().expect("isolator lock poisoned").apply()?;

    let air_gap_active = !matches!(cfg.isolation.mode, AirGapMode::Disabled);

    audit
        .append(
            "daemon",
            AuditEvent::DaemonStart {
                version: env!("CARGO_PKG_VERSION").into(),
                air_gap: air_gap_active,
            },
        )
        .await?;

    match maranode_attestation::binary::measure_self() {
        Ok(m) => {
            let tpm = maranode_attestation::tpm::read_pcrs();
            let tpm_available = matches!(tpm, maranode_attestation::TpmResult::Available { .. });
            audit
                .append(
                    "daemon",
                    AuditEvent::BinaryAttested {
                        binary_sha256: m.sha256,
                        binary_path: m.path,
                        tpm_available,
                    },
                )
                .await?;
            if tpm_available {
                info!("Binary attested (SHA-256 measured, TPM PCRs read)");
            } else {
                info!("Binary attested (SHA-256 measured, no TPM)");
            }
        }
        Err(e) => tracing::warn!("Binary attestation failed: {e}"),
    }

    // probe TEE environment and record in audit chain
    {
        let tee_report = get_tee_report(b"daemon-startup");
        let binary_sha256 = maranode_attestation::binary::measure_self()
            .map(|m| m.sha256)
            .unwrap_or_default();
        if !tee_report.is_synthetic {
            info!(tee_type = %tee_report.tee_type, "TEE environment detected");
        }
        audit.append("daemon", AuditEvent::TeeAttested {
            tee_type: tee_report.tee_type.to_string(),
            report_hash: tee_report.report_hash,
            binary_sha256,
        }).await?;
    }

    let device_pref = match cfg.device.to_lowercase().as_str() {
        "cpu" => DevicePreference::Cpu,
        "gpu" => DevicePreference::Gpu,
        "npu" => DevicePreference::Npu,
        "ryzenai" => DevicePreference::RyzenAi,
        "auto" | "" => DevicePreference::Auto,
        other => anyhow::bail!(
            "unknown device '{}' (config or --device): expected: auto, cpu, gpu, npu, ryzenai",
            other
        ),
    };

    let raw_engine: Arc<dyn InferenceEngine> =
        Arc::new(LlamaCppEngine::new(device_pref, cfg.inference.max_loaded_models).context("initialising llama.cpp engine")?);

    let max_queue = cfg.inference.max_queue_depth;
    let max_parallel = cfg.inference.max_parallel;
    let inference_queue = InferenceQueue::new(raw_engine, max_queue, max_parallel);
    let engine: Arc<dyn InferenceEngine> = inference_queue.clone();

    info!(
        "Inference queue ready (max_parallel={}, max_waiting={})",
        max_parallel, max_queue,
    );

    let master_key = load_master_kek(&cfg).await?;

    let ws_db_path = cfg.data_dir.join("workspaces.db");
    let workspace_db = WorkspaceDb::open_with_kek(&ws_db_path, master_key)
        .context("opening workspace database")?;

    let workspaces = workspace_db.list().context("listing workspaces")?;
    let mut workspace_audits = HashMap::new();
    for ws in &workspaces {
        let ws_data = cfg.data_dir.join("workspaces").join(&ws.slug);
        std::fs::create_dir_all(&ws_data)?;
        let ws_audit = AuditLog::open(
            &maranode_audit::log::default_log_path(&ws_data),
            &maranode_audit::log::default_key_path(&ws_data),
        )?;
        ws_audit
            .set_rotation(maranode_audit::rotate::RotateConfig {
                max_bytes: cfg.logging.audit_max_mb.saturating_mul(1024 * 1024),
                max_age_days: cfg.logging.audit_max_age_days,
            })
            .await;
        workspace_audits.insert(ws.slug.clone(), ws_audit);
    }
    info!("Workspace store ready ({} workspace(s))", workspaces.len());

    let rag = if cfg.rag.enabled {
        let embed_model = ModelId::parse(&cfg.rag.embedding_model).ok_or_else(|| {
            anyhow::anyhow!(
                "invalid rag.embedding_model '{}': expected <name>:<tag>",
                cfg.rag.embedding_model
            )
        })?;
        let rag_config = maranode_rag::RagConfig {
            enabled: true,
            embedding_model: cfg.rag.embedding_model.clone(),
            default_collection: cfg.rag.default_collection.clone(),
            chunk_size: cfg.rag.chunk_size,
            chunk_overlap: cfg.rag.chunk_overlap,
            top_k: cfg.rag.top_k,
            min_score: cfg.rag.min_score,
            max_context_chars: cfg.rag.max_context_chars,
        };
        let embedder = Arc::new(EngineEmbedder::new(
            Arc::clone(&engine),
            store.clone(),
            embed_model,
        ));
        let rag_dek = workspace_db.get_dek_bytes("default").ok().flatten();
        let rag_engine = match rag_dek {
            Some(dek) => {
                info!("RAG store encryption: active (default workspace DEK)");
                RagEngine::open_with_dek(&cfg.data_dir, embedder, rag_config, dek)
            }
            None => RagEngine::open(&cfg.data_dir, embedder, rag_config),
        }
        .context("initialising RAG engine")?;
        info!(
            "RAG enabled (model '{}', collection '{}')",
            cfg.rag.embedding_model, cfg.rag.default_collection
        );
        Some(Arc::new(rag_engine))
    } else {
        None
    };

    if cfg.auth.admin_key.is_none() {
        tracing::warn!(
            "auth.admin_key is not set: running in open development mode. \
             Set MARANODE_ADMIN_KEY or auth.admin_key in config before exposing this to a network."
        );
    }

    let system_prompt = cfg
        .assistant
        .resolved_system_prompt()
        .context("loading assistant system prompt")?;

    match &system_prompt {
        Some(p) => info!(
            "System prompt active ({} chars, first 80: {:?})",
            p.chars().count(),
            p.chars().take(80).collect::<String>()
        ),
        None => info!("No system prompt configured (assistant.name / system_prompt not set)"),
    }

    let user_db_path = cfg.data_dir.join("users.db");
    let user_db = UserDb::open(&user_db_path).context("opening user database")?;

    if user_db.count()? == 0 {
        if let Some(admin_key) = &cfg.auth.admin_key {
            let hash = UserDb::hash_password(admin_key)?;
            let admin = User {
                id: uuid::Uuid::new_v4(),
                username: "admin".into(),
                email: None,
                password_hash: Some(hash),
                role: Role::Admin,
                provider: AuthProvider::Local,
                provider_sub: None,
                active: true,
                created_at: chrono::Utc::now(),
                last_login: None,
            };
            user_db.create(&admin)?;
            info!("Created initial admin user (username: admin, password: admin_key)");
        } else {
            info!("No users configured and no admin_key set: user DB empty, admin key auth only");
        }
    }

    info!("User store ready ({} user(s))", user_db.count()?);

    let runtime = runtime_from_config(&cfg, air_gap_active, system_prompt);

    let recovered_incident = load_incident_on_start(&cfg.data_dir).await;
    let recovered_frozen = recovered_incident.as_ref().map(|i| i.audit_frozen).unwrap_or(false);
    if let Some(ref inc) = recovered_incident {
        tracing::warn!(incident_id = %inc.id, phase = %inc.phase, "Incident state recovered from disk");
    }

    let state = AppState {
        store,
        audit: audit.clone(),
        engine,
        version: env!("CARGO_PKG_VERSION").into(),
        data_dir: cfg.data_dir.clone(),
        rag,
        runtime: new_shared(runtime),
        stats: Stats::new(),
        workspace_db: Arc::new(tokio::sync::Mutex::new(workspace_db)),
        workspace_audits: Arc::new(tokio::sync::Mutex::new(workspace_audits)),
        rate_limiter: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        workspace_usage: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        user_db: Arc::new(tokio::sync::Mutex::new(user_db)),
        oidc_pending: new_oidc_pending(),
        auth_ip_limiter: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        isolation_ok: Arc::new(AtomicBool::new(true)),
        classification: Arc::new(tokio::sync::RwLock::new(
            ClassificationPolicy::load(&cfg.data_dir).unwrap_or_default()
        )),
        change_mgmt: Arc::new(ChangeManagementConfig {
            servicenow_url: cfg.change_mgmt.servicenow_url.clone(),
            servicenow_user: cfg.change_mgmt.servicenow_user.clone(),
            servicenow_password: cfg.change_mgmt.servicenow_password.clone(),
            jira_url: cfg.change_mgmt.jira_url.clone(),
            jira_project: cfg.change_mgmt.jira_project.clone(),
            jira_user: cfg.change_mgmt.jira_user.clone(),
            jira_token: cfg.change_mgmt.jira_token.clone(),
        }),
        dlp: Arc::new({
            let d = &cfg.dlp;
            DlpConfig {
                purview: match (&d.purview_tenant_id, &d.purview_client_id, &d.purview_client_secret) {
                    (Some(t), Some(c), Some(s)) => Some(PurviewCfg {
                        tenant_id: t.clone(),
                        client_id: c.clone(),
                        client_secret: s.clone(),
                        subscription_id: d.purview_subscription_id.clone(),
                    }),
                    _ => None,
                },
                forcepoint: match (&d.forcepoint_url, &d.forcepoint_username, &d.forcepoint_password) {
                    (Some(u), Some(n), Some(p)) => Some(ForcepointCfg {
                        base_url: u.clone(),
                        username: n.clone(),
                        password: p.clone(),
                    }),
                    _ => None,
                },
                symantec: match (&d.symantec_url, &d.symantec_username, &d.symantec_password) {
                    (Some(u), Some(n), Some(p)) => Some(SymantecCfg {
                        enforce_url: u.clone(),
                        username: n.clone(),
                        password: p.clone(),
                    }),
                    _ => None,
                },
            }
        }),
        incident: {
            let handle = new_incident_handle();
            if let Some(inc) = recovered_incident {
                *handle.lock().await = Some(inc);
            }
            handle
        },
        audit_frozen: Arc::new(AtomicBool::new(recovered_frozen)),
    };

    let reload_services = Arc::new(ReloadServices {
        state: state.clone(),
        config_path: cfg_path.clone(),
        pins: pins.clone(),
        snapshot,
        isolator: isolator.clone(),
        inference_queue,
        log_reload,
    });

    #[cfg(unix)]
    spawn_sighup_reload(reload_services.clone());

    probe::spawn(state.clone());
    {
        let rt = state.rt();
        state
            .audit
            .set_rotation(maranode_audit::rotate::RotateConfig {
                max_bytes: rt.audit_max_mb.saturating_mul(1024 * 1024),
                max_age_days: rt.audit_max_age_days,
            })
            .await;
    }
    spawn_retention_scheduler(state.clone());

    let router = build_router(state).merge(admin::router(reload_services));

    info!("Listening on http://{}", cfg.bind);

    let tcp_listener = tokio::net::TcpListener::bind(&cfg.bind).await?;

    #[cfg(unix)]
    let unix_path = cfg.unix_socket.clone();

    #[cfg(unix)]
    if let Some(ref sock_path) = unix_path {
        if std::path::Path::new(sock_path).exists() {
            std::fs::remove_file(sock_path)?;
        }
        if let Some(parent) = std::path::Path::new(sock_path).parent() {
            std::fs::create_dir_all(parent)?;
        }
    }

    #[cfg(unix)]
    let unix_listener = if let Some(ref sock_path) = unix_path {
        let l = tokio::net::UnixListener::bind(sock_path)?;
        info!("Unix socket at {}", sock_path);
        Some(l)
    } else {
        None
    };

    #[cfg(unix)]
    {
        match unix_listener {
            Some(ul) => {
                let router2 = router.clone();
                tokio::select! {
                    r = axum::serve(tcp_listener, router).with_graceful_shutdown(shutdown::signal()) => r?,
                    r = unix_serve::serve_unix(ul, router2, shutdown::signal()) => r?,
                }
            }
            None => {
                axum::serve(tcp_listener, router)
                    .with_graceful_shutdown(shutdown::signal())
                    .await?;
            }
        }
    }

    #[cfg(not(unix))]
    axum::serve(tcp_listener, router)
        .with_graceful_shutdown(shutdown::signal())
        .await?;

    audit
        .append(
            "daemon",
            AuditEvent::DaemonStop {
                reason: "graceful shutdown".into(),
            },
        )
        .await?;

    if let Err(e) = isolator.lock().expect("isolator lock poisoned").teardown() {
        tracing::warn!("isolation teardown failed: {e}");
    }

    info!("maranoded stopped");
    Ok(())
}

fn open_audit_log(cfg: &DaemonConfig) -> Result<AuditLog> {
    let log_path = maranode_audit::log::default_log_path(&cfg.data_dir);
    let key_path = maranode_audit::log::default_key_path(&cfg.data_dir);

    if !cfg.tpm.enabled || !cfg.tpm.seal_purposes.iter().any(|p| p == "audit-hmac") {
        return AuditLog::open(&log_path, &key_path);
    }

    let purpose = "audit-hmac";
    let passphrase = cfg.tpm.software_passphrase.as_deref().unwrap_or("");
    let pcr_list = format!("sha256:{}", cfg.tpm.pcr_indices);

    if is_sealed(purpose, &cfg.data_dir) {
        match unseal(purpose, &cfg.data_dir, passphrase) {
            Ok(key_bytes) => {
                info!("Audit HMAC key unsealed from TPM");
                return AuditLog::open_with_key(&log_path, key_bytes);
            }
            Err(e) => anyhow::bail!("TPM unseal failed for audit-hmac: {e}"),
        }
    }

    // first run: load/create plain key, then seal it
    let key_bytes = maranode_audit::key::load_or_generate(&key_path)?;
    match seal(&key_bytes, purpose, &cfg.data_dir, Some(&pcr_list), passphrase) {
        Ok(meta) => info!(
            "Audit HMAC key sealed to TPM (backend={:?})",
            meta.backend
        ),
        Err(e) => tracing::warn!("Audit HMAC key TPM seal failed, using plain file: {e}"),
    }

    AuditLog::open_with_key(&log_path, key_bytes)
}

async fn load_master_kek(cfg: &DaemonConfig) -> Result<[u8; 32]> {
    if !cfg.tpm.enabled {
        let key = kek::load_or_create(&kek::default_kek_path(&cfg.data_dir))
            .context("loading master KEK")?;
        info!("Master KEK loaded (plain file)");
        return Ok(key);
    }

    let purpose = "workspace-kek";
    let passphrase = cfg.tpm.software_passphrase.as_deref().unwrap_or("");
    let pcr_list = format!("sha256:{}", cfg.tpm.pcr_indices);

    if is_sealed(purpose, &cfg.data_dir) {
        match unseal(purpose, &cfg.data_dir, passphrase) {
            Ok(bytes) => {
                if bytes.len() != 32 {
                    anyhow::bail!("unsealed workspace-kek is {} bytes, expected 32", bytes.len());
                }
                let mut key = [0u8; 32];
                key.copy_from_slice(&bytes);
                info!("Master KEK unsealed from TPM (purpose=workspace-kek)");
                return Ok(key);
            }
            Err(e) => anyhow::bail!("TPM unseal failed for workspace-kek: {e}"),
        }
    }

    // first run — generate KEK and seal it
    let key = kek::load_or_create(&kek::default_kek_path(&cfg.data_dir))
        .context("loading/creating master KEK")?;

    match seal(key.as_ref(), purpose, &cfg.data_dir, Some(&pcr_list), passphrase) {
        Ok(meta) => info!(
            "Master KEK sealed to TPM (backend={:?}, pcrs={})",
            meta.backend,
            meta.pcr_list.as_deref().unwrap_or("none")
        ),
        Err(e) => tracing::warn!("TPM seal failed, running with plain KEK file: {e}"),
    }

    Ok(key)
}

fn spawn_retention_scheduler(state: maranode_api::AppState) {
    tokio::spawn(async move {
        use maranode_audit::rotate::RotateConfig;
        use maranode_audit::AuditLog;
        use tokio::time::{interval, Duration};

        let mut ticker = interval(Duration::from_secs(12 * 60 * 60));
        ticker.tick().await; // skip the immediate first tick

        loop {
            ticker.tick().await;
            let rt = state.rt();
            let retain_days = rt.content_log_retention_days;
            let cfg = RotateConfig {
                max_bytes: rt.audit_max_mb.saturating_mul(1024 * 1024),
                max_age_days: rt.audit_max_age_days,
            };

            let main_dir = state.data_dir.clone();
            sweep_audit_dir(&state.audit, &main_dir, &cfg, retain_days, "main").await;

            let ws: Vec<(String, AuditLog)> = {
                let guard = state.workspace_audits.lock().await;
                guard.iter().map(|(s, l)| (s.clone(), l.clone())).collect()
            };
            for (slug, log) in ws {
                let ws_dir = state.data_dir.join("workspaces").join(&slug);
                sweep_audit_dir(&log, &ws_dir, &cfg, retain_days, &slug).await;
            }
        }
    });
}

/// run one rotation + segment cleanup pass over a single audit directory.
async fn sweep_audit_dir(
    log: &maranode_audit::AuditLog,
    dir: &std::path::Path,
    cfg: &maranode_audit::rotate::RotateConfig,
    retain_days: u32,
    label: &str,
) {
    let log_path = maranode_audit::log::default_log_path(dir);
    log.set_rotation(*cfg).await;
    match log.maybe_rotate(&log_path, cfg).await {
        Ok(Some(seg)) => tracing::info!(
            "rotation: sealed {} audit segment {} (seq {}-{})",
            label,
            seg.file,
            seg.seq_start,
            seg.seq_end
        ),
        Ok(None) => {}
        Err(e) => tracing::warn!("rotation: {} rotate failed: {e}", label),
    }
    match log.enforce_segment_retention(dir, retain_days).await {
        Ok(n) if n > 0 => {
            tracing::info!("retention: removed {} expired {} audit segment(s)", n, label)
        }
        Ok(_) => {}
        Err(e) => tracing::warn!("retention: {} segment cleanup failed: {e}", label),
    }

    if retain_days > 0 {
        match maranode_audit::retention::prune_log(&log_path, retain_days) {
            Ok(n) if n > 0 => {
                tracing::info!("retention: pruned {} entries from {} audit log", n, label)
            }
            Ok(_) => {}
            Err(e) => tracing::warn!("retention: {} prune failed: {e}", label),
        }
    }
}

#[cfg(unix)]
fn spawn_sighup_reload(services: Arc<ReloadServices>) {
    tokio::spawn(async move {
        use tokio::signal::unix::{signal, SignalKind};
        let mut hup = match signal(SignalKind::hangup()) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("SIGHUP reload disabled: {e}");
                return;
            }
        };
        loop {
            hup.recv().await;
            match services.reload().await {
                Ok(r) => info!(
                    "SIGHUP config reload ok (applied={:?}, requires_restart={:?})",
                    r.applied, r.requires_restart
                ),
                Err(e) => tracing::error!("SIGHUP config reload failed: {e:#}"),
            }
        }
    });
}
