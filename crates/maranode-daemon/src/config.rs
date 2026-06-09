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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct InferenceConfig {
    /// max waiting requests. New requests get 503 when full.
    /// value 0 means no limit.
    pub max_queue_depth: usize,
    /// how many requests may run in parallel. default is 4.
    /// on CPU, each parallel slot holds its own KV cache in RAM (~1-2 GB for a 7B model).
    /// lower this on memory-constrained machines; raise it for GPU deployments with many users.
    /// changing this requires a daemon restart.
    pub max_parallel: usize,
}

impl Default for InferenceConfig {
    fn default() -> Self {
        Self {
            max_queue_depth: 32,
            max_parallel: 4,
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
    /// example: ldaps://dc.example.com:636
    pub url: String,
    /// bind DN for LDAP user search
    pub bind_dn: String,
    pub bind_pw: String,
    /// base DN for user search
    pub base_dn: String,
    /// LDAP login attribute, e.g. samaccountname or uid.
    #[serde(default = "default_ldap_uid")]
    pub uid_attr: String,
    /// map LDAP group DN to role. first match wins
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
    /// identity provider metadata URL
    pub idp_metadata_url: String,
    /// service provider entity ID
    pub sp_entity_id: String,
    /// service provider signing certificate in PEM format
    pub sp_cert: Option<String>,
    /// service provider signing key in PEM format
    pub sp_key: Option<String>,
    #[serde(default = "default_viewer")]
    pub default_role: String,
}

fn default_viewer() -> String {
    "viewer".into()
}
fn default_ldap_uid() -> String {
    "uid".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RagIngestPolicy {
    /// any client can ingest. no API key required
    #[default]
    Anyone,
    AdminOnly,
    Allowlist,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RagConfig {
    /// set true to enable local RAG. default is false
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
    /// assistant display name, e.g. Aria, MedBot, Support Agent
    /// used in auto prompt when system_prompt is empty
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
        if let Ok(v) = std::env::var("MARANODE_EMBEDDING_MODEL") {
            self.rag.embedding_model = v;
        }
        if let Ok(v) = std::env::var("MARANODE_RAG_COLLECTION") {
            self.rag.default_collection = v;
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

/// standard paths where config file is searched
fn config_search_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // XDG config dir, or `~/.config/maranode`
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        paths.push(PathBuf::from(xdg).join("maranode/config.toml"));
    } else if let Some(home) = std::env::var_os("HOME") {
        paths.push(PathBuf::from(home).join(".config/maranode/config.toml"));
    }

    // system path `/etc/maranode/config.toml`
    paths.push(PathBuf::from("/etc/maranode/config.toml"));

    paths
}
