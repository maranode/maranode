use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoldRecord {
    pub id: String,
    pub placed_at: DateTime<Utc>,
    pub placed_by: String,
    pub reason: String,
    pub seq_from: u64,
    pub seq_to: u64,
    pub expires_at: Option<DateTime<Utc>>,
    pub hold_key_pubkey: String, // base64 Ed25519 verifying key
    pub placement_sig: String,   // base64 Ed25519 sig over placement payload
    pub released_at: Option<DateTime<Utc>>,
    pub released_by: Option<String>,
    pub release_sig: Option<String>, // base64 Ed25519 sig from hold key over release payload
    pub tpm_sealed: bool,
}

impl HoldRecord {
    pub fn is_active(&self) -> bool {
        if self.released_at.is_some() {
            return false;
        }
        match self.expires_at {
            Some(exp) => Utc::now() < exp,
            None => true,
        }
    }

    pub fn covers_seq(&self, seq: u64) -> bool {
        self.is_active() && seq >= self.seq_from && seq <= self.seq_to
    }
}

#[derive(Serialize)]
pub struct PlacementPayload<'a> {
    pub hold_id: &'a str,
    pub placed_by: &'a str,
    pub reason: &'a str,
    pub seq_from: u64,
    pub seq_to: u64,
    pub placed_at: &'a DateTime<Utc>,
    pub hold_key_pubkey: &'a str,
}

#[derive(Serialize)]
pub struct ReleasePayload<'a> {
    pub hold_id: &'a str,
    pub released_by: &'a str,
    pub released_at: &'a DateTime<Utc>,
}
