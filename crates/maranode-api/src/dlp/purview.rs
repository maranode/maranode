// Microsoft Purview / Azure Information Protection label sync
// Reads sensitivity labels from Purview and maps them to Maranode DataLabel.
// Requires: tenant_id, client_id, client_secret (app registration with
// InformationProtectionPolicy.Read.All permission).

use anyhow::Result;
use maranode_common::classification::DataLabel;

use super::{ImportedLabel, PurviewCfg};

pub async fn sync(cfg: &PurviewCfg) -> Result<Vec<ImportedLabel>> {
    let client = reqwest::Client::new();

    // obtain bearer token from AAD
    let token_url = format!(
        "https://login.microsoftonline.com/{}/oauth2/v2.0/token",
        cfg.tenant_id
    );
    let token_resp = client
        .post(&token_url)
        .form(&[
            ("grant_type", "client_credentials"),
            ("client_id", &cfg.client_id),
            ("client_secret", &cfg.client_secret),
            ("scope", "https://graph.microsoft.com/.default"),
        ])
        .send()
        .await?
        .json::<serde_json::Value>()
        .await?;

    let access_token = token_resp["access_token"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Purview token missing access_token"))?
        .to_string();

    // list sensitivity labels from MS Graph
    let labels_resp = client
        .get("https://graph.microsoft.com/v1.0/security/informationProtection/sensitivityLabels")
        .bearer_auth(&access_token)
        .send()
        .await?
        .json::<serde_json::Value>()
        .await?;

    let mut imported = Vec::new();
    if let Some(labels) = labels_resp["value"].as_array() {
        for label in labels {
            let name = label["name"].as_str().unwrap_or("").to_lowercase();
            let mapped = map_purview_label(&name);
            let display_name = label["name"].as_str().unwrap_or("").to_string();
            if !display_name.is_empty() {
                imported.push(ImportedLabel {
                    collection: display_name,
                    label: mapped,
                });
            }
        }
    }

    Ok(imported)
}

fn map_purview_label(name: &str) -> DataLabel {
    if name.contains("phi") || name.contains("health") || name.contains("hipaa") {
        DataLabel::Phi
    } else if name.contains("pii") || name.contains("personal") || name.contains("gdpr") {
        DataLabel::Pii
    } else if name.contains("confidential") || name.contains("secret") {
        DataLabel::Confidential
    } else if name.contains("restricted") || name.contains("internal") {
        DataLabel::Restricted
    } else {
        DataLabel::Public
    }
}
