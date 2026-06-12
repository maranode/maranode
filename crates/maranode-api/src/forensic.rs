use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::Utc;
use sha2::{Digest, Sha256};

use crate::state::AppState;

pub fn snapshot_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("forensic-snapshots")
}

pub async fn take_snapshot(state: &AppState, incident_id: &str) -> Result<(String, String)> {
    let now = Utc::now();
    let ts = now.format("%Y%m%dT%H%M%SZ");

    // collect runtime info
    let models: Vec<_> = state
        .store
        .list()
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|m| serde_json::json!({ "id": m.id, "sha256": m.sha256 }))
        .collect();

    let workspace_list: Vec<String> = {
        let db = state.workspace_db.lock().await;
        db.list().unwrap_or_default()
    };

    let audit_seq = state.audit.seq().await;

    let rt = state.rt();

    let isolation_ok = state.isolation_ok.load(std::sync::atomic::Ordering::Relaxed);

    let snapshot = serde_json::json!({
        "incident_id": incident_id,
        "captured_at": now,
        "daemon_version": state.version,
        "isolation_ok": isolation_ok,
        "audit_seq": audit_seq,
        "models_loaded": models,
        "workspaces": workspace_list,
        "air_gap": rt.air_gap,
        "log_prompts": rt.log_prompts,
    });

    let json_bytes = serde_json::to_vec_pretty(&snapshot)?;
    let sha256 = hex::encode(Sha256::digest(&json_bytes));

    let dir = snapshot_dir(&state.data_dir);
    std::fs::create_dir_all(&dir)?;
    let filename = format!("{ts}-{incident_id}.json");
    let path = dir.join(&filename);
    std::fs::write(&path, &json_bytes)?;

    Ok((path.to_string_lossy().to_string(), sha256))
}
