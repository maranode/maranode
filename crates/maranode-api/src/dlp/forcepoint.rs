// Forcepoint DLP REST API label sync.
// Connects to Forcepoint DLP Manager REST API (v1) to retrieve policy categories
// and map them to Maranode DataLabel values.

use anyhow::Result;
use maranode_common::classification::DataLabel;

use super::{ForcepointCfg, ImportedLabel};

pub async fn sync(cfg: &ForcepointCfg) -> Result<Vec<ImportedLabel>> {
    let client = reqwest::Client::new();

    // authenticate — Forcepoint DLP uses basic auth or session token
    let auth_url = format!("{}/api/v1/auth/login", cfg.base_url.trim_end_matches('/'));
    let session = client
        .post(&auth_url)
        .basic_auth(&cfg.username, Some(&cfg.password))
        .send()
        .await?
        .json::<serde_json::Value>()
        .await?;

    let token = session["sessionToken"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Forcepoint login: no sessionToken in response"))?
        .to_string();

    // fetch policy categories (these map to data classification labels)
    let cats_url = format!("{}/api/v1/policies/categories", cfg.base_url.trim_end_matches('/'));
    let cats_resp = client
        .get(&cats_url)
        .header("X-Session-Token", &token)
        .send()
        .await?
        .json::<serde_json::Value>()
        .await?;

    let mut imported = Vec::new();
    if let Some(categories) = cats_resp["categories"].as_array() {
        for cat in categories {
            let name = cat["name"].as_str().unwrap_or("").to_lowercase();
            let label = map_forcepoint_category(&name);
            let display = cat["name"].as_str().unwrap_or("").to_string();
            if !display.is_empty() {
                imported.push(ImportedLabel {
                    collection: display,
                    label,
                });
            }
        }
    }

    // logout
    let _ = client
        .post(format!("{}/api/v1/auth/logout", cfg.base_url.trim_end_matches('/')))
        .header("X-Session-Token", &token)
        .send()
        .await;

    Ok(imported)
}

fn map_forcepoint_category(name: &str) -> DataLabel {
    if name.contains("phi") || name.contains("hipaa") || name.contains("health") {
        DataLabel::Phi
    } else if name.contains("pii") || name.contains("gdpr") || name.contains("personal data") {
        DataLabel::Pii
    } else if name.contains("confidential") {
        DataLabel::Confidential
    } else if name.contains("restricted") || name.contains("internal") {
        DataLabel::Restricted
    } else {
        DataLabel::Public
    }
}
