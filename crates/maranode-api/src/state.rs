//! shared app state passed to each axum handler

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

use maranode_audit::AuditLog;
use maranode_common::classification::ClassificationPolicy;
use maranode_inference::InferenceEngine;
use maranode_rag::RagEngine;
use maranode_store::{ModelStore, UserDb, WorkspaceDb};

use crate::changemgmt::ChangeManagementConfig;
use crate::dlp::DlpConfig;
use crate::runtime::{RuntimeSettings, SharedRuntime};

#[derive(Debug)]
pub struct Stats {
    pub requests: AtomicU64,
    pub errors: AtomicU64,
    pub tokens_in: AtomicU64,
    pub tokens_out: AtomicU64,
    pub duration_ms: AtomicU64,
    pub started_at: Instant,
}

impl Stats {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            requests: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            tokens_in: AtomicU64::new(0),
            tokens_out: AtomicU64::new(0),
            duration_ms: AtomicU64::new(0),
            started_at: Instant::now(),
        })
    }

    pub fn record_ok(&self, tokens_in: u32, tokens_out: u32, duration_ms: u64) {
        self.requests.fetch_add(1, Ordering::Relaxed);
        self.tokens_in
            .fetch_add(tokens_in as u64, Ordering::Relaxed);
        self.tokens_out
            .fetch_add(tokens_out as u64, Ordering::Relaxed);
        self.duration_ms.fetch_add(duration_ms, Ordering::Relaxed);
    }

    pub fn record_error(&self) {
        self.requests.fetch_add(1, Ordering::Relaxed);
        self.errors.fetch_add(1, Ordering::Relaxed);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RagIngestPolicy {
    /// no key needed, any caller can ingest
    Anyone,
    /// only admin key can ingest
    AdminOnly,
    /// only keys in rag_ingest_allowlist or admin key can ingest
    Allowlist,
}

#[derive(Debug, Clone, Default)]
pub struct IdentityConfig {
    pub oidc: Option<OidcCfg>,
    pub ldap: Option<LdapCfg>,
    pub saml: Option<SamlCfg>,
    pub session_hours: i64,
}

#[derive(Debug, Clone)]
pub struct OidcCfg {
    pub issuer_url: String,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
    pub default_role: String,
}

#[derive(Debug, Clone)]
pub struct LdapCfg {
    pub url: String,
    pub bind_dn: String,
    pub bind_pw: String,
    pub base_dn: String,
    pub uid_attr: String,
    pub group_role_map: Vec<(String, String)>,
    pub default_role: String,
}

#[derive(Debug, Clone)]
pub struct SamlCfg {
    pub idp_metadata_url: String,
    pub sp_entity_id: String,
    pub sp_cert: Option<String>,
    pub sp_key: Option<String>,
    pub idp_cert: Option<String>,
    pub default_role: String,
}

#[derive(Debug, Default)]
pub struct WorkspaceUsage {
    pub concurrent: u32,
    /// model_key -> (size_bytes, in_flight_count)
    pub active_models: HashMap<String, (u64, u32)>,
}

impl WorkspaceUsage {
    pub fn memory_bytes(&self) -> u64 {
        self.active_models.values().map(|(sz, _)| sz).sum()
    }

    pub fn model_count(&self) -> u32 {
        self.active_models.len() as u32
    }

    pub fn acquire(&mut self, model_key: &str, size_bytes: u64) {
        self.concurrent += 1;
        let e = self.active_models.entry(model_key.to_string()).or_insert((size_bytes, 0));
        e.1 += 1;
    }

    pub fn release(&mut self, model_key: &str) {
        self.concurrent = self.concurrent.saturating_sub(1);
        if let Some(e) = self.active_models.get_mut(model_key) {
            e.1 = e.1.saturating_sub(1);
            if e.1 == 0 {
                self.active_models.remove(model_key);
            }
        }
    }
}

/// TTL for a pending OIDC login flow (10 minutes).
const OIDC_PENDING_TTL: Duration = Duration::from_secs(600);

/// short-lived state kept between oidc_login and oidc_callback.
/// keyed by the CSRF state token value.
pub struct OidcPendingState {
    pub pkce_verifier_secret: String,
    pub nonce_secret: String,
    pub expires_at: Instant,
}

pub type OidcPendingMap = Arc<Mutex<HashMap<String, OidcPendingState>>>;

pub fn new_oidc_pending() -> OidcPendingMap {
    Arc::new(Mutex::new(HashMap::new()))
}

/// insert a new pending entry; also prune any expired entries at the same time.
pub async fn oidc_pending_insert(
    map: &OidcPendingMap,
    state_token: String,
    pkce_verifier_secret: String,
    nonce_secret: String,
) {
    let mut guard = map.lock().await;
    let now = Instant::now();
    guard.retain(|_, v| v.expires_at > now);
    guard.insert(
        state_token,
        OidcPendingState {
            pkce_verifier_secret,
            nonce_secret,
            expires_at: now + OIDC_PENDING_TTL,
        },
    );
}

/// consume a pending entry; returns None if missing or expired.
pub async fn oidc_pending_take(
    map: &OidcPendingMap,
    state_token: &str,
) -> Option<OidcPendingState> {
    let mut guard = map.lock().await;
    let entry = guard.remove(state_token)?;
    if entry.expires_at < Instant::now() {
        return None;
    }
    Some(entry)
}

#[derive(Clone)]
pub struct AppState {
    pub store: ModelStore,
    pub audit: AuditLog,
    pub engine: Arc<dyn InferenceEngine>,
    pub version: String,
    pub data_dir: PathBuf,
    pub rag: Option<Arc<RagEngine>>,

    /// settings that can change on reload (auth, prompts, RAG policy, identity, air-gap)
    pub runtime: SharedRuntime,

    pub stats: Arc<Stats>,

    /// workspace database, shared behind mutex
    pub workspace_db: Arc<Mutex<WorkspaceDb>>,

    /// per-workspace audit logs, keyed by slug
    pub workspace_audits: Arc<Mutex<HashMap<String, AuditLog>>>,

    /// per-workspace rate limiter: slug -> (count, window_start_secs)
    pub rate_limiter: Arc<Mutex<HashMap<String, (u32, u64)>>>,

    /// per-workspace resource usage now (concurrency and loaded models)
    pub workspace_usage: Arc<Mutex<HashMap<String, WorkspaceUsage>>>,

    /// user and session database
    pub user_db: Arc<Mutex<UserDb>>,

    /// short-lived OIDC pending state: csrf_token -> (pkce_verifier, nonce, ttl)
    pub oidc_pending: OidcPendingMap,

    /// per-IP auth rate limiter: ip_str -> (count, window_start_secs)
    pub auth_ip_limiter: Arc<Mutex<HashMap<String, (u32, u64)>>>,

    /// false when the last isolation probe detected egress (isolation broken).
    /// starts as true (optimistic); probe task sets it to false on drift.
    pub isolation_ok: Arc<AtomicBool>,

    pub change_mgmt: Arc<ChangeManagementConfig>,

    pub classification: Arc<tokio::sync::RwLock<ClassificationPolicy>>,

    pub dlp: Arc<DlpConfig>,
}

impl AppState {
    /// copy of reloadable settings (cheap clone)
    pub fn rt(&self) -> RuntimeSettings {
        self.runtime
            .read()
            .expect("runtime settings lock poisoned")
            .clone()
    }

    pub fn replace_runtime(&self, settings: RuntimeSettings) {
        *self
            .runtime
            .write()
            .expect("runtime settings lock poisoned") = settings;
    }
}

/// extract the best available client IP from request headers.
/// checks X-Forwarded-For first, then X-Real-IP, then returns "unknown".
pub fn client_ip(headers: &axum::http::HeaderMap) -> String {
    if let Some(xff) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
        if let Some(first) = xff.split(',').next() {
            let ip = first.trim().to_string();
            if !ip.is_empty() {
                return ip;
            }
        }
    }
    if let Some(real) = headers.get("x-real-ip").and_then(|v| v.to_str().ok()) {
        let ip = real.trim().to_string();
        if !ip.is_empty() {
            return ip;
        }
    }
    "unknown".to_string()
}

/// sliding-window per-IP rate limiter for auth endpoints.
/// allows up to `max_per_minute` attempts per 60-second window per IP.
pub async fn check_auth_ip_rate(
    limiter: &Mutex<HashMap<String, (u32, u64)>>,
    ip: &str,
    max_per_minute: u32,
) -> Result<(), crate::error::ApiError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let mut map = limiter.lock().await;

    // prune stale entries every so often to keep memory bounded
    if map.len() > 4096 {
        map.retain(|_, (_, start)| now.saturating_sub(*start) < 120);
    }

    let entry = map.entry(ip.to_string()).or_insert((0, now));
    if now.saturating_sub(entry.1) >= 60 {
        *entry = (1, now);
    } else {
        entry.0 = entry.0.saturating_add(1);
        if entry.0 > max_per_minute {
            return Err(crate::error::ApiError::rate_limited(
                "too many login attempts from this IP, try again in a minute",
            ));
        }
    }
    Ok(())
}
