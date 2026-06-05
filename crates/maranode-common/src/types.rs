//! shared value types such as AirGapMode

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AirGapMode {
    #[default]
    AirGap,
    Whitelist,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    pub ok: bool,
    pub version: String,
    pub air_gap_mode: AirGapMode,
    pub loaded_models: Vec<String>,
    pub uptime_secs: u64,
}
