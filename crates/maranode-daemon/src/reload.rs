//! reload config.toml without daemon restart.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use maranode_api::{
    AppState, IdentityConfig, LdapCfg, OidcCfg, RagIngestPolicy, RuntimeSettings, SamlCfg,
};
use maranode_common::events::AuditEvent;
use maranode_common::types::AirGapMode;
use maranode_inference::InferenceQueue;
use maranode_isolation::{IsolationConfig, Isolator, WhitelistEntry};
use maranode_rag::RagConfig;
use anyhow::{Context, Result};
use serde::Serialize;
use tracing::info;
use tracing_subscriber::reload::Handle;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Registry;

use crate::config::{DaemonConfig, RagIngestPolicy as CfgRagPolicy};

/// CLI flags fixed for whole process lifetime.
#[derive(Clone, Default)]
pub struct StartupPins {
    pub data_dir: Option<PathBuf>,
    pub bind: Option<String>,
    pub log_level: Option<String>,
    pub device: Option<String>,
    pub unix_socket: Option<Option<String>>,
    pub no_isolation: bool,
    pub rag_enabled: Option<bool>,
    pub embedding_model: Option<String>,
    pub rag_collection: Option<String>,
    pub admin_key: Option<String>,
    pub models_dir: Option<PathBuf>,
}

impl StartupPins {
    pub fn apply(&self, cfg: &mut DaemonConfig) {
        if let Some(d) = &self.data_dir {
            cfg.data_dir = d.clone();
        }
        if let Some(b) = &self.bind {
            cfg.bind = b.clone();
        }
        if let Some(l) = &self.log_level {
            cfg.log_level = l.clone();
        }
        if let Some(d) = &self.device {
            cfg.device = d.clone();
        }
        if let Some(u) = &self.unix_socket {
            cfg.unix_socket = u.clone();
        }
        if self.no_isolation || !cfg!(target_os = "linux") {
            cfg.isolation.mode = AirGapMode::Disabled;
        }
        if let Some(r) = self.rag_enabled {
            cfg.rag.enabled = r;
        }
        if let Some(m) = &self.embedding_model {
            cfg.rag.embedding_model = m.clone();
        }
        if let Some(c) = &self.rag_collection {
            cfg.rag.default_collection = c.clone();
        }
        if let Some(k) = &self.admin_key {
            cfg.auth.admin_key = Some(k.clone());
        }
        if let Some(m) = &self.models_dir {
            cfg.models_dir = m.clone();
        }
    }
}

/// config at process start. Used to find settings that need restart on reload.
#[derive(Clone)]
pub struct StartupSnapshot {
    pub bind: String,
    pub data_dir: PathBuf,
    pub device: String,
    pub rag_enabled: bool,
    pub embedding_model: String,
    pub unix_socket: Option<String>,
    pub max_parallel: usize,
}

#[derive(Clone)]
pub struct ReloadServices {
    pub state: AppState,
    pub config_path: Option<PathBuf>,
    pub pins: StartupPins,
    pub snapshot: StartupSnapshot,
    pub isolator: Arc<Mutex<Isolator>>,
    pub inference_queue: Arc<InferenceQueue>,
    pub log_reload: Handle<EnvFilter, Registry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReloadResponse {
    pub ok: bool,
    pub path: Option<String>,
    pub applied: Vec<String>,
    pub requires_restart: Vec<String>,
}

impl ReloadServices {
    pub async fn reload(&self) -> Result<ReloadResponse> {
        let path = self
            .config_path
            .as_ref()
            .context("no config file: set --config or place config at a standard path")?;

        let mut cfg = DaemonConfig::load(path)?;
        cfg.apply_env_overrides();
        self.pins.apply(&mut cfg);

        let mut requires_restart = Vec::new();
        if cfg.bind != self.snapshot.bind && self.pins.bind.is_none() {
            requires_restart.push("bind".into());
        }
        if cfg.data_dir != self.snapshot.data_dir && self.pins.data_dir.is_none() {
            requires_restart.push("data_dir".into());
        }
        if cfg.device != self.snapshot.device && self.pins.device.is_none() {
            requires_restart.push("device".into());
        }
        if cfg.rag.enabled != self.snapshot.rag_enabled && self.pins.rag_enabled.is_none() {
            requires_restart.push("rag.enabled".into());
        }
        if cfg.rag.embedding_model != self.snapshot.embedding_model
            && self.pins.embedding_model.is_none()
        {
            requires_restart.push("rag.embedding_model".into());
        }
        if cfg.inference.max_parallel != self.snapshot.max_parallel {
            requires_restart.push("inference.max_parallel".into());
        }
        let effective_unix = cfg.unix_socket.clone();
        if effective_unix != self.snapshot.unix_socket && self.pins.unix_socket.is_none() {
            requires_restart.push("unix_socket".into());
        }

        let mut applied = Vec::new();

        // Logging level
        if self.pins.log_level.is_none() {
            let filter = EnvFilter::try_new(&cfg.log_level)
                .with_context(|| format!("invalid log_level '{}'", cfg.log_level))?;
            self.log_reload.reload(filter)?;
            applied.push("log_level".into());
        }

        // inference queue max depth
        self.inference_queue
            .set_max_waiting(cfg.inference.max_queue_depth);
        applied.push("inference.max_queue_depth".into());

        // network isolation settings
        let iso_cfg = isolation_from_config(&cfg);
        {
            let mut iso = self.isolator.lock().expect("isolator lock poisoned");
            iso.reconfigure(iso_cfg)?;
        }
        applied.push("isolation".into());

        let air_gap = !matches!(cfg.isolation.mode, AirGapMode::Disabled);

        // assistant system prompt
        let system_prompt = cfg
            .assistant
            .resolved_system_prompt()
            .context("loading assistant system prompt")?;
        applied.push("assistant".into());

        // RAG settings apply only when RAG engine exists
        if let Some(rag) = &self.state.rag {
            let rag_cfg = RagConfig {
                enabled: true,
                embedding_model: cfg.rag.embedding_model.clone(),
                default_collection: cfg.rag.default_collection.clone(),
                chunk_size: cfg.rag.chunk_size,
                chunk_overlap: cfg.rag.chunk_overlap,
                top_k: cfg.rag.top_k,
                min_score: cfg.rag.min_score,
                max_context_chars: cfg.rag.max_context_chars,
            };
            rag.apply_runtime_config(&rag_cfg);
            applied.push("rag".into());
        } else if cfg.rag.enabled {
            requires_restart.push("rag.enabled".into());
        }

        let runtime = runtime_from_config(&cfg, air_gap, system_prompt);
        self.state.replace_runtime(runtime);
        applied.push("auth".into());

        self.state
            .audit
            .append(
                "daemon",
                AuditEvent::ConfigReloaded {
                    path: path.display().to_string(),
                },
            )
            .await?;

        info!(
            "config reloaded from {} (applied: {:?}, needs restart: {:?})",
            path.display(),
            applied,
            requires_restart
        );

        Ok(ReloadResponse {
            ok: true,
            path: Some(path.display().to_string()),
            applied,
            requires_restart,
        })
    }
}

pub fn runtime_from_config(
    cfg: &DaemonConfig,
    air_gap: bool,
    system_prompt: Option<String>,
) -> RuntimeSettings {
    let rag_ingest_policy = match cfg.rag.ingest_policy {
        CfgRagPolicy::Anyone => RagIngestPolicy::Anyone,
        CfgRagPolicy::AdminOnly => RagIngestPolicy::AdminOnly,
        CfgRagPolicy::Allowlist => RagIngestPolicy::Allowlist,
    };

    RuntimeSettings {
        admin_key: cfg.auth.admin_key.clone(),
        rag_ingest_policy,
        rag_ingest_allowlist: cfg.rag.ingest_allowlist.clone(),
        system_prompt,
        identity: identity_from_config(cfg),
        air_gap,
        log_prompts: cfg.logging.log_prompts,
        content_log_retention_days: cfg.logging.content_log_retention_days,
    }
}

pub fn identity_from_config(cfg: &DaemonConfig) -> IdentityConfig {
    IdentityConfig {
        session_hours: cfg.auth.session_hours,
        oidc: cfg.auth.oidc.as_ref().map(|o| OidcCfg {
            issuer_url: o.issuer_url.clone(),
            client_id: o.client_id.clone(),
            client_secret: o.client_secret.clone(),
            redirect_uri: o.redirect_uri.clone(),
            default_role: o.default_role.clone(),
        }),
        ldap: cfg.auth.ldap.as_ref().map(|l| LdapCfg {
            url: l.url.clone(),
            bind_dn: l.bind_dn.clone(),
            bind_pw: l.bind_pw.clone(),
            base_dn: l.base_dn.clone(),
            uid_attr: l.uid_attr.clone(),
            group_role_map: l
                .group_role_map
                .iter()
                .map(|g| (g.group_dn.clone(), g.role.clone()))
                .collect(),
            default_role: l.default_role.clone(),
        }),
        saml: cfg.auth.saml.as_ref().map(|s| SamlCfg {
            idp_metadata_url: s.idp_metadata_url.clone(),
            sp_entity_id: s.sp_entity_id.clone(),
            sp_cert: s.sp_cert.clone(),
            sp_key: s.sp_key.clone(),
            idp_cert: s.idp_cert.clone(),
            default_role: s.default_role.clone(),
        }),
    }
}

fn isolation_from_config(cfg: &DaemonConfig) -> IsolationConfig {
    IsolationConfig {
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
    }
}
