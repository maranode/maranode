//! state machine for network isolation on and off

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use maranode_common::types::AirGapMode;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IsolationConfig {
    pub mode: AirGapMode,
    pub api_port: u16,
    pub api_allowed_sources: Vec<String>,
    pub whitelist: Vec<WhitelistEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhitelistEntry {
    pub host: String,
    pub port: u16,
    pub comment: String,
}

impl Default for IsolationConfig {
    fn default() -> Self {
        Self {
            mode: AirGapMode::AirGap,
            api_port: 11984,
            api_allowed_sources: vec!["127.0.0.1".into(), "::1".into()],
            whitelist: vec![],
        }
    }
}

#[derive(Debug)]
pub struct VerifyOutcome {
    pub mode: AirGapMode,
    pub rules_present: bool,
    pub external_probe_blocked: bool,
    pub detail: String,
}

impl VerifyOutcome {
    pub fn is_ok(&self) -> bool {
        self.rules_present && (self.mode == AirGapMode::Disabled || self.external_probe_blocked)
    }
}

pub struct Isolator {
    config: IsolationConfig,
}

impl Isolator {
    pub fn new(config: IsolationConfig) -> Self {
        Self { config }
    }

    pub fn apply(&self) -> Result<()> {
        match self.config.mode {
            AirGapMode::AirGap => {
                info!("Applying air-gap iptables rules");
                crate::iptables::apply_air_gap(&self.config)?;
                info!("Air-gap enforcement active: all outbound traffic blocked");
            }
            AirGapMode::Whitelist => {
                info!(
                    "Applying whitelist iptables rules ({} entries)",
                    self.config.whitelist.len()
                );
                crate::iptables::apply_whitelist(&self.config)?;
            }
            AirGapMode::Disabled => {
                warn!("Network isolation DISABLED: no egress restrictions in effect");
            }
        }
        Ok(())
    }

    pub fn verify(&self) -> Result<VerifyOutcome> {
        let rules_present = crate::iptables::check_rules_present(&self.config)?;
        let external_probe_blocked = crate::probe::probe_external_blocked()?;

        let detail = if rules_present && external_probe_blocked {
            "All outbound traffic blocked. iptables rules intact.".into()
        } else if !rules_present {
            "WARNING: expected iptables rules are missing!".into()
        } else {
            "WARNING: external network probe succeeded: air-gap may be broken!".into()
        };

        Ok(VerifyOutcome {
            mode: self.config.mode,
            rules_present,
            external_probe_blocked,
            detail,
        })
    }

    pub fn teardown(&self) -> Result<()> {
        crate::iptables::remove_rules()?;
        info!("Isolation rules removed");
        Ok(())
    }

    /// update isolation config and apply iptables rules again
    pub fn reconfigure(&mut self, config: IsolationConfig) -> Result<()> {
        let mode_changed = self.config.mode != config.mode;
        if mode_changed || self.config.api_port != config.api_port {
            let _ = crate::iptables::remove_rules();
        }
        self.config = config;
        self.apply()
    }
}
