use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

use maranode_common::incident::{BreakGlassCred, IncidentPhase, IncidentState, PhaseEntry};

pub type IncidentHandle = Arc<Mutex<Option<IncidentState>>>;

pub fn new_incident_handle() -> IncidentHandle {
    Arc::new(Mutex::new(None))
}

pub fn break_glass_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("break-glass")
}

pub fn incident_state_path(data_dir: &Path) -> PathBuf {
    data_dir.join("incident-state.json")
}

pub async fn persist_incident(state: &IncidentState, data_dir: &Path) -> Result<()> {
    let path = incident_state_path(data_dir);
    let json = serde_json::to_vec_pretty(state)?;
    tokio::fs::write(&path, json).await?;
    Ok(())
}

pub async fn load_incident_on_start(data_dir: &Path) -> Option<IncidentState> {
    let path = incident_state_path(data_dir);
    let bytes = tokio::fs::read(&path).await.ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub async fn transition_phase(
    incident: &mut IncidentState,
    new_phase: IncidentPhase,
    by: &str,
    note: Option<String>,
    data_dir: &Path,
) -> Result<()> {
    incident.phase_log.push(PhaseEntry {
        phase: new_phase.clone(),
        at: Utc::now(),
        by: by.to_string(),
        note: note.clone(),
    });
    incident.phase = new_phase;
    if incident.phase == IncidentPhase::Resolved {
        incident.resolved_at = Some(Utc::now());
        incident.resolution_summary = note;
        incident.audit_frozen = false;
    }
    persist_incident(incident, data_dir).await?;
    Ok(())
}

pub async fn notify_webhooks(incident: &IncidentState) {
    if incident.webhook_urls.is_empty() {
        return;
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_default();

    let payload = serde_json::json!({
        "incident_id": incident.id,
        "phase": incident.phase.to_string(),
        "declared_by": incident.declared_by,
        "reason": incident.reason,
    });

    for url in &incident.webhook_urls {
        let _ = client.post(url).json(&payload).send().await;
    }
}

pub fn generate_break_glass_cred(purpose: &str) -> (String, BreakGlassCred) {
    use std::time::{SystemTime, UNIX_EPOCH};

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();

    let mut raw = format!("bglass-{purpose}-{ts:08x}");
    // add some extra entropy from process id
    raw.push_str(&format!("-{:08x}", std::process::id()));

    let id = uuid_from_bytes(Sha256::digest(raw.as_bytes()).as_slice());
    let token = format!("bg1_{}", hex::encode(&Sha256::digest(format!("{raw}-token").as_bytes())[..16]));
    let token_hash = hex::encode(Sha256::digest(token.as_bytes()));

    let cred = BreakGlassCred {
        id: id.clone(),
        purpose: purpose.to_string(),
        token_hash,
        created_at: Utc::now(),
        used_at: None,
        used_by: None,
    };

    (token, cred)
}

pub fn load_break_glass_creds(data_dir: &Path) -> Result<Vec<BreakGlassCred>> {
    let dir = break_glass_dir(data_dir);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut creds = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        if entry.path().extension().and_then(|e| e.to_str()) == Some("json") {
            let bytes = std::fs::read(entry.path())?;
            if let Ok(c) = serde_json::from_slice::<BreakGlassCred>(&bytes) {
                creds.push(c);
            }
        }
    }
    Ok(creds)
}

pub fn save_break_glass_cred(data_dir: &Path, cred: &BreakGlassCred) -> Result<()> {
    let dir = break_glass_dir(data_dir);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", cred.id));
    std::fs::write(path, serde_json::to_vec_pretty(cred)?)?;
    Ok(())
}

pub fn verify_and_consume_break_glass(
    data_dir: &Path,
    token: &str,
    used_by: &str,
) -> Result<BreakGlassCred> {
    let token_hash = hex::encode(Sha256::digest(token.as_bytes()));
    let creds = load_break_glass_creds(data_dir)?;

    let mut matched = creds
        .into_iter()
        .find(|c| c.token_hash == token_hash)
        .ok_or_else(|| anyhow::anyhow!("break-glass token not found"))?;

    if matched.is_used() {
        anyhow::bail!("break-glass token already used at {:?}", matched.used_at);
    }

    matched.used_at = Some(Utc::now());
    matched.used_by = Some(used_by.to_string());
    save_break_glass_cred(data_dir, &matched)?;

    Ok(matched)
}

fn uuid_from_bytes(bytes: &[u8]) -> String {
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        u16::from_be_bytes([bytes[4], bytes[5]]),
        u16::from_be_bytes([bytes[6], bytes[7]]),
        u16::from_be_bytes([bytes[8], bytes[9]]),
        {
            let mut n = 0u64;
            for b in &bytes[10..16] { n = (n << 8) | (*b as u64); }
            n
        }
    )
}
