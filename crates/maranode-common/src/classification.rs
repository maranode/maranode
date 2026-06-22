use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DataLabel {
    Public = 0,
    Restricted = 1,
    Confidential = 2,
    Pii = 3,
    Phi = 4,
}

impl DataLabel {
    pub fn as_str(&self) -> &'static str {
        match self {
            DataLabel::Public => "PUBLIC",
            DataLabel::Restricted => "RESTRICTED",
            DataLabel::Confidential => "CONFIDENTIAL",
            DataLabel::Pii => "PII",
            DataLabel::Phi => "PHI",
        }
    }

    pub fn level(&self) -> u8 {
        *self as u8
    }
}

impl std::fmt::Display for DataLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for DataLabel {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        match s.to_uppercase().as_str() {
            "PUBLIC" => Ok(DataLabel::Public),
            "RESTRICTED" => Ok(DataLabel::Restricted),
            "CONFIDENTIAL" => Ok(DataLabel::Confidential),
            "PII" => Ok(DataLabel::Pii),
            "PHI" => Ok(DataLabel::Phi),
            other => anyhow::bail!("unknown data label '{other}' (expected: PUBLIC, RESTRICTED, CONFIDENTIAL, PII, PHI)"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionPolicy {
    pub label: DataLabel,
    #[serde(default = "bool_true")]
    pub block_on_violation: bool,
}

fn bool_true() -> bool { true }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspacePolicy {
    pub max_clearance: DataLabel,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClassificationPolicy {
    #[serde(default)]
    pub collections: HashMap<String, CollectionPolicy>,
    #[serde(default)]
    pub workspaces: HashMap<String, WorkspacePolicy>,
}

impl ClassificationPolicy {
    pub fn policy_path(data_dir: &Path) -> PathBuf {
        data_dir.join("classification").join("policy.json")
    }

    pub fn load(data_dir: &Path) -> anyhow::Result<Self> {
        let path = Self::policy_path(data_dir);
        if !path.exists() {
            return Ok(Self::default());
        }
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

    pub fn collection_label(&self, collection: &str) -> Option<&CollectionPolicy> {
        self.collections.get(collection)
    }

    pub fn workspace_clearance(&self, workspace_slug: &str) -> DataLabel {
        self.workspaces
            .get(workspace_slug)
            .map(|w| w.max_clearance)
            .unwrap_or(DataLabel::Public)
    }

    pub fn check_access(&self, workspace_slug: &str, collection: &str) -> Option<ViolationInfo> {
        let clearance = self.workspace_clearance(workspace_slug);
        if let Some(col_policy) = self.collections.get(collection) {
            if col_policy.label > clearance {
                return Some(ViolationInfo {
                    collection: collection.to_string(),
                    required_label: col_policy.label,
                    workspace_clearance: clearance,
                    block: col_policy.block_on_violation,
                });
            }
        }
        None
    }

    pub fn set_collection_label(&mut self, collection: &str, label: DataLabel, block_on_violation: bool) {
        self.collections.insert(
            collection.to_string(),
            CollectionPolicy { label, block_on_violation },
        );
    }

    pub fn check_all_collections(&self, workspace_slug: &str) -> Vec<ViolationInfo> {
        let clearance = self.workspace_clearance(workspace_slug);
        self.collections
            .iter()
            .filter(|(_, p)| p.label > clearance)
            .map(|(col, p)| ViolationInfo {
                collection: col.clone(),
                required_label: p.label,
                workspace_clearance: clearance,
                block: p.block_on_violation,
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct ViolationInfo {
    pub collection: String,
    pub required_label: DataLabel,
    pub workspace_clearance: DataLabel,
    pub block: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn label_ordering_strings_and_parse() {
        assert!(DataLabel::Phi > DataLabel::Public);
        assert!(DataLabel::Confidential > DataLabel::Restricted);
        assert_eq!(DataLabel::Pii.level(), 3);
        assert_eq!(DataLabel::Phi.as_str(), "PHI");
        assert_eq!(
            DataLabel::from_str("confidential").unwrap(),
            DataLabel::Confidential
        );
        assert_eq!(DataLabel::from_str("PHI").unwrap(), DataLabel::Phi);
        assert!(DataLabel::from_str("nope").is_err());
    }

    #[test]
    fn unknown_workspace_is_public() {
        let p = ClassificationPolicy::default();
        assert_eq!(p.workspace_clearance("ghost"), DataLabel::Public);
        assert!(p.check_access("ghost", "anything").is_none());
    }

    #[test]
    fn access_violates_when_label_above_clearance() {
        let mut p = ClassificationPolicy::default();
        p.set_collection_label("phi-notes", DataLabel::Phi, true);
        p.workspaces.insert(
            "clinic".into(),
            WorkspacePolicy {
                max_clearance: DataLabel::Confidential,
            },
        );

        let v = p
            .check_access("clinic", "phi-notes")
            .expect("violation expected");
        assert_eq!(v.required_label, DataLabel::Phi);
        assert_eq!(v.workspace_clearance, DataLabel::Confidential);
        assert!(v.block);

        // raise the clearance to PHI and access is allowed
        p.workspaces.insert(
            "clinic".into(),
            WorkspacePolicy {
                max_clearance: DataLabel::Phi,
            },
        );
        assert!(p.check_access("clinic", "phi-notes").is_none());
    }

    #[test]
    fn check_all_lists_only_violations() {
        let mut p = ClassificationPolicy::default();
        p.set_collection_label("open", DataLabel::Public, true);
        p.set_collection_label("secret", DataLabel::Confidential, false);
        // default workspace clearance is Public
        let vs = p.check_all_collections("w");
        assert_eq!(vs.len(), 1);
        assert_eq!(vs[0].collection, "secret");
        assert!(!vs[0].block);
    }

    #[test]
    fn collection_block_defaults_true() {
        let cp: CollectionPolicy = serde_json::from_str(r#"{"label":"PII"}"#).unwrap();
        assert_eq!(cp.label, DataLabel::Pii);
        assert!(cp.block_on_violation);
    }
}
