// PCR seal policy — defines which PCRs to bind keys to and what values are expected.
// When a key is sealed to a PCR policy, the TPM will only release it if all
// selected PCRs contain the expected values at the time of unsealing.
//
// Standard PCR layout (UEFI + Linux):
//   PCR 0  — firmware code (UEFI image)
//   PCR 1  — firmware configuration and data
//   PCR 2  — option ROM code
//   PCR 3  — option ROM configuration
//   PCR 4  — boot loader code (IPL)
//   PCR 5  — boot loader configuration
//   PCR 6  — state transitions (S3/S4/S5)
//   PCR 7  — Secure Boot policy and certificates
//   PCR 8-9 — GRUB commandline / kernel image
//   PCR 11  — systemd-boot / unified kernel image (UKI) hash
//   PCR 14  — Shim's MOK list
//
// Recommended seal targets for production servers: 0, 7 (+ optionally 11, 14)

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::tpm::TpmResult;

/// which PCRs to seal against and what SHA-256 values they must hold.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PcrPolicy {
    pub pcrs: BTreeMap<u8, String>, // index -> expected sha256 hex
    pub description: String,
}

impl PcrPolicy {
    pub fn policy_path(data_dir: &Path) -> PathBuf {
        data_dir.join("tpm").join("pcr-policy.json")
    }

    pub fn load(data_dir: &Path) -> anyhow::Result<Self> {
        let path = Self::policy_path(data_dir);
        let bytes = std::fs::read(&path)?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub fn save(&self, data_dir: &Path) -> anyhow::Result<()> {
        let path = Self::policy_path(data_dir);
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        std::fs::write(path, serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }

    /// capture the current PCR state and create a policy from it.
    /// only includes the PCR indices listed in `selected`.
    pub fn from_current(selected: &[u8]) -> anyhow::Result<Self> {
        let result = crate::tpm::read_pcrs();
        let pcrs = match result {
            TpmResult::Available { pcrs } => pcrs,
            TpmResult::Error { reason } => {
                anyhow::bail!("TPM PCR read error: {}", reason)
            }
            TpmResult::Unavailable => {
                anyhow::bail!("TPM not available on this platform")
            }
        };

        let mut selected_pcrs = BTreeMap::new();
        for idx in selected {
            if let Some(val) = pcrs.get(idx) {
                selected_pcrs.insert(*idx, val.clone());
            } else {
                anyhow::bail!("PCR {} not available in TPM response", idx);
            }
        }

        Ok(Self {
            pcrs: selected_pcrs,
            description: format!(
                "captured from live TPM, PCRs: {}",
                selected.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",")
            ),
        })
    }

    /// typical server profile: firmware (0) + Secure Boot state (7)
    pub fn server_profile(data_dir: &Path) -> anyhow::Result<Self> {
        let mut policy = Self::from_current(&[0, 7])?;
        policy.description = "server profile: PCR0 (firmware) + PCR7 (Secure Boot)".into();
        policy.save(data_dir)?;
        Ok(policy)
    }

    /// workstation profile adds kernel commandline checks
    pub fn workstation_profile(data_dir: &Path) -> anyhow::Result<Self> {
        let mut policy = Self::from_current(&[0, 7, 11])?;
        policy.description = "workstation profile: PCR0 + PCR7 + PCR11 (kernel/UKI)".into();
        policy.save(data_dir)?;
        Ok(policy)
    }

    /// returns PCR indices in this policy as a comma-separated string for tpm2-tools
    pub fn pcr_list_str(&self) -> String {
        let indices: Vec<_> = self.pcrs.keys().map(|i| i.to_string()).collect();
        format!("sha256:{}", indices.join(","))
    }

    /// check current TPM PCR values against expected values.
    /// returns Ok(()) if all match, Err with a description of mismatches.
    pub fn verify_current(&self) -> anyhow::Result<()> {
        let result = crate::tpm::read_pcrs();
        let current = match result {
            TpmResult::Available { pcrs } => pcrs,
            TpmResult::Error { reason } => anyhow::bail!("TPM PCR read error: {}", reason),
            TpmResult::Unavailable => anyhow::bail!("TPM not available"),
        };

        let mut mismatches = Vec::new();
        for (idx, expected) in &self.pcrs {
            match current.get(idx) {
                Some(actual) if actual == expected => {}
                Some(actual) => mismatches.push(format!("PCR{idx}: expected {expected}, got {actual}")),
                None => mismatches.push(format!("PCR{idx}: not present in current PCR set")),
            }
        }

        if mismatches.is_empty() {
            Ok(())
        } else {
            anyhow::bail!("PCR policy violation:\n  {}", mismatches.join("\n  "))
        }
    }

    /// true if the current PCRs satisfy this policy (non-failing version for logging)
    pub fn is_satisfied(&self) -> bool {
        self.verify_current().is_ok()
    }
}
