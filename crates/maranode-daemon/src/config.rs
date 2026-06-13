//! daemon TOML config. Priority: file, then env, then CLI

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use maranode_common::types::AirGapMode;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DaemonConfig {
    pub data_dir: PathBuf,

    pub models_dir: PathBuf,

    pub bind: String,

    pub unix_socket: Option<String>,

    pub log_level: String,

    /// inference device: auto, cpu, gpu, or npu
    /// for auto: try metal, then cuda, rocm, vulkan, openvino, then cpu. Or set cpu, gpu, or npu directly.
    pub device: String,
    pub inference: InferenceConfig,
    pub isolation: IsolationConfig,
    pub auth: AuthConfig,
    pub rag: RagConfig,
    pub assistant: AssistantConfig,
    pub logging: LoggingConfig,
    pub integrity: IntegrityConfig,
    pub registry: RegistryConfig,
    pub change_mgmt: ChangeManagementConfig,
    pub dlp: DlpConfig,
    pub tpm: TpmConfig,
    pub smtp: Option<SmtpConfig>,
    /// hex-encoded 32-byte key for AES-256-GCM prompt/response encryption in TEE mode.
    /// generate with: maranode tee keygen
    pub tee_encrypt_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct InferenceConfig {
    pub max_queue_depth: usize,
    pub max_parallel: usize,
    pub max_loaded_models: usize,
}

impl Default for InferenceConfig {
    fn default() -> Self {
        Self {
            max_queue_depth: 32,
            max_parallel: 4,
            max_loaded_models: 2,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct IsolationConfig {
    pub mode: AirGapMode,
    pub api_port: u16,
    pub allowed_sources: Vec<String>,
    pub whitelist: Vec<WhitelistEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhitelistEntry {
    pub host: String,
    pub port: u16,
    pub comment: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AuthConfig {
    pub admin_key: Option<String>,
    /// session lifetime in hours. default is 24
    pub session_hours: i64,
    pub oidc: Option<OidcConfig>,
    pub ldap: Option<LdapConfig>,
    pub saml: Option<SamlConfig>,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            admin_key: None,
            session_hours: 24,
            oidc: None,
            ldap: None,
            saml: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OidcConfig {
    /// example: https://accounts.google.com
    pub issuer_url: String,
    pub client_id: String,
    pub client_secret: String,
    /// must be same redirect URI as registered at provider
    pub redirect_uri: String,
    /// default role for new users: admin, operator, or viewer
    #[serde(default = "default_viewer")]
    pub default_role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LdapConfig {
    pub url: String,
    pub bind_dn: String,
    pub bind_pw: String,
    pub base_dn: String,
    #[serde(default = "default_ldap_uid")]
    pub uid_attr: String,
    #[serde(default)]
    pub group_role_map: Vec<LdapGroupRole>,
    #[serde(default = "default_viewer")]
    pub default_role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LdapGroupRole {
    pub group_dn: String,
    pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SamlConfig {
    pub idp_metadata_url: String,
    pub sp_entity_id: String,
    pub sp_cert: Option<String>,
    pub sp_key: Option<String>,
    /// PEM or bare base64 DER of the IdP signing certificate.
    /// when set, only this cert is trusted for signature verification.
    /// if absent, the cert embedded in the assertion <ds:X509Certificate> is used (TOFU).
    pub idp_cert: Option<String>,
    #[serde(default = "default_viewer")]
    pub default_role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LoggingConfig {
    /// when true, full prompt and response content is written into the audit log.
    /// disabled by default — only hashed prompt is logged.
    /// requires explicit opt-in; treat the audit log as sensitive when enabled.
    pub log_prompts: bool,
    /// how many days of content-logged entries to retain when pruning.
    /// only applies to entries that contain prompt/response content.
    /// 0 means no automatic pruning.
    pub content_log_retention_days: u32,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            log_prompts: false,
            content_log_retention_days: 90,
        }
    }
}

fn default_viewer() -> String {
    "viewer".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
    /// envelope From address for outgoing mail
    pub from: String,
    /// use STARTTLS (true) or plain TCP (false); TLS-on-connect is not supported
    #[serde(default = "bool_true")]
    pub starttls: bool,
}

fn bool_true() -> bool { true }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct IntegrityConfig {
    /// where to look for .mrn-baseline files. default: <data_dir>/baselines/
    pub baselines_dir: Option<PathBuf>,

    /// what to do on behavioral drift: allow, warn, or refuse
    /// allow: run inference as normal, just log the drift event
    /// warn: log the event and emit a tracing warning
    /// refuse: block the model from serving until restarted
    #[serde(default = "default_drift_action")]
    pub drift_action: DriftAction,
}

impl Default for IntegrityConfig {
    fn default() -> Self {
        Self {
            baselines_dir: None,
            drift_action: DriftAction::Warn,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DriftAction {
    Allow,
    Warn,
    Refuse,
}

fn default_drift_action() -> DriftAction {
    DriftAction::Warn
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RegistryConfig {
    pub require_approval_token: bool,
    pub tokens_dir: Option<PathBuf>,
}

impl Default for RegistryConfig {
    fn default() -> Self {
        Self {
            require_approval_token: false,
            tokens_dir: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TpmConfig {
    /// enable TPM sealing for workspace KEK and audit HMAC key
    pub enabled: bool,
    /// PCR indices to seal against (comma-separated, e.g. "0,7")
    /// defaults to "0,7" (firmware + Secure Boot)
    pub pcr_indices: String,
    /// passphrase used for the software fallback when TPM is unavailable
    /// must be set in config or via MARANODE_TPM_PASSPHRASE env var
    pub software_passphrase: Option<String>,
    /// which key purposes to seal: "workspace-kek", "audit-hmac", "admin-cred"
    pub seal_purposes: Vec<String>,
}

impl Default for TpmConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            pcr_indices: "0,7".into(),
            software_passphrase: None,
            seal_purposes: vec!["workspace-kek".into(), "audit-hmac".into()],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct DlpConfig {
    pub purview_tenant_id: Option<String>,
    pub purview_client_id: Option<String>,
    pub purview_client_secret: Option<String>,
    pub purview_subscription_id: Option<String>,
    pub forcepoint_url: Option<String>,
    pub forcepoint_username: Option<String>,
    pub forcepoint_password: Option<String>,
    pub symantec_url: Option<String>,
    pub symantec_username: Option<String>,
    pub symantec_password: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ChangeManagementConfig {
    pub servicenow_url: Option<String>,
    pub servicenow_user: Option<String>,
    pub servicenow_password: Option<String>,
    pub jira_url: Option<String>,
    pub jira_project: Option<String>,
    pub jira_user: Option<String>,
    pub jira_token: Option<String>,
}

fn default_ldap_uid() -> String {
    "uid".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RagIngestPolicy {
    #[default]
    Anyone,
    AdminOnly,
    Allowlist,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RagConfig {
    pub enabled: bool,
    pub embedding_model: String,
    pub default_collection: String,
    pub chunk_size: usize,
    pub chunk_overlap: usize,
    pub top_k: usize,
    pub min_score: f32,
    pub max_context_chars: usize,
    pub ingest_policy: RagIngestPolicy,
    pub ingest_allowlist: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AssistantConfig {
    pub name: String,
    pub system_prompt: String,
    pub system_prompt_file: Option<PathBuf>,
}

impl Default for AssistantConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            system_prompt: String::new(),
            system_prompt_file: None,
        }
    }
}

impl AssistantConfig {
    pub fn resolved_system_prompt(&self) -> Result<Option<String>> {
        // step 1: prompt file has highest priority
        if let Some(path) = &self.system_prompt_file {
            let text = std::fs::read_to_string(path).map_err(|e| {
                anyhow::anyhow!(
                    "failed to read assistant.system_prompt_file '{}': {}",
                    path.display(),
                    e
                )
            })?;
            let trimmed = text.trim().to_string();
            if !trimmed.is_empty() {
                return Ok(Some(trimmed));
            }
        }

        // step 2: inline system_prompt field
        let inline = self.system_prompt.trim();
        if !inline.is_empty() {
            return Ok(Some(inline.to_string()));
        }

        // step 3: build prompt from name only
        let name = self.name.trim();
        if !name.is_empty() {
            return Ok(Some(format!(
                "You are {name}, a helpful and honest AI assistant. \
                 Be polite, concise, and accurate. \
                 Never assist with requests that could harm people.",
                name = name
            )));
        }

        // step 4: no prompt configured
        Ok(None)
    }
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            data_dir: maranode_common::paths::default_data_dir(),
            models_dir: maranode_common::paths::default_models_dir(),
            bind: "127.0.0.1:11984".into(),
            unix_socket: maranode_common::paths::default_unix_socket(),
            log_level: "info".into(),
            device: "auto".into(),
            inference: InferenceConfig::default(),
            isolation: IsolationConfig::default(),
            auth: AuthConfig::default(),
            rag: RagConfig::default(),
            assistant: AssistantConfig::default(),
            logging: LoggingConfig::default(),
            integrity: IntegrityConfig::default(),
            registry: RegistryConfig::default(),
            change_mgmt: ChangeManagementConfig::default(),
            dlp: DlpConfig::default(),
            tpm: TpmConfig::default(),
            smtp: None,
            tee_encrypt_key: None,
        }
    }
}

impl Default for IsolationConfig {
    fn default() -> Self {
        Self {
            mode: AirGapMode::AirGap,
            api_port: 11984,
            allowed_sources: vec!["127.0.0.1".into(), "::1".into()],
            whitelist: vec![],
        }
    }
}

impl Default for RagConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            embedding_model: "bge-m3:latest".into(),
            default_collection: "default".into(),
            chunk_size: 1200,
            chunk_overlap: 200,
            top_k: 5,
            min_score: 0.40,
            max_context_chars: 6000,
            ingest_policy: RagIngestPolicy::Anyone,
            ingest_allowlist: vec![],
        }
    }
}

impl DaemonConfig {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let contents = std::fs::read_to_string(path)?;
        let config: Self = toml::from_str(&contents)
            .map_err(|e| anyhow::anyhow!("invalid config at {}: {}", path.display(), e))?;
        Ok(config)
    }

    pub fn discover() -> Result<(Self, Option<PathBuf>)> {
        for path in config_search_paths() {
            if path.exists() {
                let cfg = Self::load(&path)?;
                return Ok((cfg, Some(path)));
            }
        }
        Ok((Self::default(), None))
    }

    /// replace file values from env vars. names match CLI and env docs
    pub fn apply_env_overrides(&mut self) {
        if let Ok(v) = std::env::var("MARANODE_DATA_DIR") {
            self.data_dir = v.into();
        }
        if let Ok(v) = std::env::var("MARANODE_MODELS_DIR") {
            self.models_dir = v.into();
        }
        if let Ok(v) = std::env::var("MARANODE_BIND") {
            self.bind = v;
        }
        if let Ok(v) = std::env::var("RUST_LOG") {
            self.log_level = v;
        }
        if let Ok(v) = std::env::var("MARANODE_DEVICE") {
            self.device = v;
        }
        if let Ok(v) = std::env::var("MARANODE_UNIX_SOCKET") {
            self.unix_socket = if v.is_empty() { None } else { Some(v) };
        }
        if let Ok(v) = std::env::var("MARANODE_ADMIN_KEY") {
            self.auth.admin_key = Some(v);
        }
        if std::env::var_os("MARANODE_RAG").is_some_and(|v| v == "1" || v == "true") {
            self.rag.enabled = true;
        }
        if std::env::var_os("MARANODE_LOG_PROMPTS").is_some_and(|v| v == "1" || v == "true") {
            self.logging.log_prompts = true;
        }
        if let Ok(v) = std::env::var("MARANODE_EMBEDDING_MODEL") {
            self.rag.embedding_model = v;
        }
        if let Ok(v) = std::env::var("MARANODE_RAG_COLLECTION") {
            self.rag.default_collection = v;
        }
        if let Ok(v) = std::env::var("MARANODE_TPM_PASSPHRASE") {
            self.tpm.software_passphrase = Some(v);
        }
        if std::env::var_os("MARANODE_TPM").is_some_and(|v| v == "1" || v == "true") {
            self.tpm.enabled = true;
        }
    }

    pub fn discover_or_explicit(explicit: Option<&Path>) -> Result<(Self, Option<PathBuf>)> {
        if let Some(p) = explicit {
            if !p.exists() {
                anyhow::bail!("config file not found: {}", p.display());
            }
            let cfg = Self::load(p)?;
            return Ok((cfg, Some(p.to_path_buf())));
        }
        Self::discover()
    }
}

fn config_search_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        paths.push(PathBuf::from(xdg).join("maranode/config.toml"));
    } else if let Some(home) = std::env::var_os("HOME") {
        paths.push(PathBuf::from(home).join(".config/maranode/config.toml"));
    }

    paths.push(PathBuf::from("/etc/maranode/config.toml"));

    paths
}
