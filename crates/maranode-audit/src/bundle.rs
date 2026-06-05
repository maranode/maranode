use std::io::{Cursor, Write};
use std::path::Path;

use anyhow::Result;
use chrono::Utc;
use sha2::{Digest, Sha256};

use crate::key::load;
use crate::verify::verify_log;

pub fn create_bundle(
    log_path: &Path,
    key_path: &Path,
    output_path: &Path,
    workspace: Option<&str>,
) -> Result<()> {
    let buf = Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(buf);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    let log_bytes = std::fs::read(log_path)?;
    let log_sha256 = format!("{:x}", Sha256::digest(&log_bytes));
    zip.start_file("audit.jsonl", opts)?;
    zip.write_all(&log_bytes)?;

    let key = load(key_path)?;
    let result = verify_log(log_path, &key)?;
    let integrity = serde_json::json!({
        "verified_at": Utc::now().to_rfc3339(),
        "entries_checked": result.entries_checked,
        "ok": result.ok,
        "first_violation": result.first_violation.as_ref().map(|v| serde_json::json!({
            "seq": v.seq,
            "detail": v.detail,
        })),
    });
    let integrity_bytes = serde_json::to_vec_pretty(&integrity)?;
    zip.start_file("integrity.json", opts)?;
    zip.write_all(&integrity_bytes)?;

    let integrity_sha256 = format!("{:x}", Sha256::digest(&integrity_bytes));
    let mut manifest = serde_json::json!({
        "created_at": Utc::now().to_rfc3339(),
        "files": [
            { "name": "audit.jsonl",    "sha256": log_sha256 },
            { "name": "integrity.json", "sha256": integrity_sha256 },
        ]
    });
    if let Some(slug) = workspace {
        manifest["workspace"] = serde_json::Value::String(slug.to_string());
    }
    zip.start_file("manifest.json", opts)?;
    zip.write_all(serde_json::to_vec_pretty(&manifest)?.as_slice())?;

    let finished = zip.finish()?;
    std::fs::write(output_path, finished.into_inner())?;

    Ok(())
}
