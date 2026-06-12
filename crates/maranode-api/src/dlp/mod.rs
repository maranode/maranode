pub mod forcepoint;
pub mod purview;
pub mod symantec;

use maranode_common::classification::DataLabel;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct ImportedLabel {
    pub collection: String,
    pub label: DataLabel,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DlpConfig {
    pub purview: Option<PurviewCfg>,
    pub forcepoint: Option<ForcepointCfg>,
    pub symantec: Option<SymantecCfg>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PurviewCfg {
    pub tenant_id: String,
    pub client_id: String,
    pub client_secret: String,
    pub subscription_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForcepointCfg {
    pub base_url: String,
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymantecCfg {
    pub enforce_url: String,
    pub username: String,
    pub password: String,
}
