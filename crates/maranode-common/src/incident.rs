use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IncidentPhase {
    Declared,
    Investigating,
    Resolved,
}

impl std::fmt::Display for IncidentPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IncidentPhase::Declared => write!(f, "declared"),
            IncidentPhase::Investigating => write!(f, "investigating"),
            IncidentPhase::Resolved => write!(f, "resolved"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncidentState {
    pub id: String,
    pub declared_at: DateTime<Utc>,
    pub declared_by: String,
    pub reason: String,
    pub phase: IncidentPhase,
    pub audit_frozen: bool,
    pub resolved_at: Option<DateTime<Utc>>,
    pub resolution_summary: Option<String>,
    pub phase_log: Vec<PhaseEntry>,
    pub webhook_urls: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseEntry {
    pub phase: IncidentPhase,
    pub at: DateTime<Utc>,
    pub by: String,
    pub note: Option<String>,
}

impl IncidentState {
    pub fn new(id: String, declared_by: String, reason: String, webhook_urls: Vec<String>) -> Self {
        let now = Utc::now();
        Self {
            id,
            declared_at: now,
            declared_by: declared_by.clone(),
            reason: reason.clone(),
            phase: IncidentPhase::Declared,
            audit_frozen: true,
            resolved_at: None,
            resolution_summary: None,
            phase_log: vec![PhaseEntry {
                phase: IncidentPhase::Declared,
                at: now,
                by: declared_by,
                note: Some(reason),
            }],
            webhook_urls,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BreakGlassCred {
    pub id: String,
    pub purpose: String,
    pub token_hash: String, // SHA-256 of the actual token
    pub created_at: DateTime<Utc>,
    pub used_at: Option<DateTime<Utc>>,
    pub used_by: Option<String>,
}

impl BreakGlassCred {
    pub fn is_used(&self) -> bool {
        self.used_at.is_some()
    }
}
