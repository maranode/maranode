// Symantec DLP Enforce Server REST API label sync.
// Polls the Enforce Server for policy groups and maps them to Maranode DataLabel.
// Requires Enforce Server credentials with API access enabled.

use anyhow::Result;
use maranode_common::classification::DataLabel;

use super::{ImportedLabel, SymantecCfg};

pub async fn sync(cfg: &SymantecCfg) -> Result<Vec<ImportedLabel>> {
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true) // Enforce Server often has self-signed cert
        .build()?;

    let base = cfg.enforce_url.trim_end_matches('/');

    // Symantec DLP Enforce uses session-based auth
    let login_url = format!("{}/ProtectManager/webservices/v2/users/login", base);
    let session_resp = client
        .post(&login_url)
        .basic_auth(&cfg.username, Some(&cfg.password))
        .send()
        .await?;

    let auth_token = session_resp
        .headers()
        .get("X-Auth-Token")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| anyhow::anyhow!("Symantec DLP: no X-Auth-Token in login response"))?
        .to_string();

    // fetch policy groups — each group corresponds to a data category
    let groups_url = format!("{}/ProtectManager/webservices/v2/policies/groups", base);
    let groups_resp = client
        .get(&groups_url)
        .header("X-Auth-Token", &auth_token)
        .send()
        .await?
        .json::<serde_json::Value>()
        .await?;

    let mut imported = Vec::new();
    if let Some(groups) = groups_resp["policyGroupList"].as_array() {
        for group in groups {
            let name = group["name"].as_str().unwrap_or("").to_string();
            if name.is_empty() {
                continue;
            }
            let label = map_symantec_policy(&name.to_lowercase());
            imported.push(ImportedLabel {
                collection: name,
                label,
            });
        }
    }

    // logout
    let _ = client
        .post(format!("{}/ProtectManager/webservices/v2/users/logout", base))
        .header("X-Auth-Token", &auth_token)
        .send()
        .await;

    Ok(imported)
}

fn map_symantec_policy(name: &str) -> DataLabel {
    if name.contains("phi") || name.contains("hipaa") || name.contains("medical") || name.contains("health") {
        DataLabel::Phi
    } else if name.contains("pii") || name.contains("personal") || name.contains("gdpr") || name.contains("ccpa") {
        DataLabel::Pii
    } else if name.contains("confidential") || name.contains("secret") || name.contains("proprietary") {
        DataLabel::Confidential
    } else if name.contains("restricted") || name.contains("internal") || name.contains("sensitive") {
        DataLabel::Restricted
    } else {
        DataLabel::Public
    }
}
