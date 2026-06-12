use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChangeManagementConfig {
    pub servicenow_url: Option<String>,
    pub servicenow_user: Option<String>,
    pub servicenow_password: Option<String>,
    pub jira_url: Option<String>,
    pub jira_project: Option<String>,
    pub jira_user: Option<String>,
    pub jira_token: Option<String>,
}

impl ChangeManagementConfig {
    pub fn is_configured(&self) -> bool {
        self.servicenow_url.is_some() || self.jira_url.is_some()
    }
}

pub async fn open_ticket(cfg: &ChangeManagementConfig, model_id: &str, model_sha256: &str, note: Option<&str>, submitted_by: &str) -> Option<(String, String)> {
    let client = reqwest::Client::new();
    let short_sha = &model_sha256[..12.min(model_sha256.len())];
    let summary = format!("Model approval request: {model_id} (sha256: {short_sha}…)");
    let description = format!(
        "Model: {model_id}\nSHA-256: {model_sha256}\nSubmitted by: {submitted_by}\nNote: {}",
        note.unwrap_or("(none)")
    );

    if let Some(snow_url) = &cfg.servicenow_url {
        let endpoint = format!("{snow_url}/api/now/table/change_request");
        let body = serde_json::json!({
            "short_description": summary,
            "description": description,
            "type": "normal",
            "category": "software",
            "state": "1",
        });
        let mut req = client.post(&endpoint).json(&body);
        if let (Some(user), Some(pass)) = (&cfg.servicenow_user, &cfg.servicenow_password) {
            req = req.basic_auth(user, Some(pass));
        }
        match req.send().await {
            Ok(r) if r.status().is_success() => {
                if let Ok(v) = r.json::<serde_json::Value>().await {
                    let ticket_id = v["result"]["number"].as_str().unwrap_or("").to_string();
                    if !ticket_id.is_empty() {
                        return Some((ticket_id, "servicenow".into()));
                    }
                }
            }
            Ok(r) => tracing::warn!("ServiceNow ticket open failed: {}", r.status()),
            Err(e) => tracing::warn!("ServiceNow request error: {e}"),
        }
    }

    if let (Some(jira_url), Some(project)) = (&cfg.jira_url, &cfg.jira_project) {
        let endpoint = format!("{jira_url}/rest/api/3/issue");
        let body = serde_json::json!({
            "fields": {
                "project": { "key": project },
                "summary": summary,
                "description": {
                    "type": "doc",
                    "version": 1,
                    "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": description }] }]
                },
                "issuetype": { "name": "Task" },
            }
        });
        let mut req = client.post(&endpoint).json(&body);
        if let (Some(user), Some(token)) = (&cfg.jira_user, &cfg.jira_token) {
            req = req.basic_auth(user, Some(token));
        }
        match req.send().await {
            Ok(r) if r.status().is_success() => {
                if let Ok(v) = r.json::<serde_json::Value>().await {
                    let key = v["key"].as_str().unwrap_or("").to_string();
                    if !key.is_empty() {
                        return Some((key, "jira".into()));
                    }
                }
            }
            Ok(r) => tracing::warn!("Jira issue create failed: {}", r.status()),
            Err(e) => tracing::warn!("Jira request error: {e}"),
        }
    }

    None
}

pub async fn close_ticket(cfg: &ChangeManagementConfig, ticket_id: &str, system: &str, resolution: &str) {
    let client = reqwest::Client::new();

    if system == "servicenow" {
        if let Some(snow_url) = &cfg.servicenow_url {
            let endpoint = format!("{snow_url}/api/now/table/change_request/{ticket_id}");
            let body = serde_json::json!({
                "state": "3",
                "close_notes": resolution,
            });
            let mut req = client.patch(&endpoint).json(&body);
            if let (Some(user), Some(pass)) = (&cfg.servicenow_user, &cfg.servicenow_password) {
                req = req.basic_auth(user, Some(pass));
            }
            if let Err(e) = req.send().await {
                tracing::warn!("ServiceNow ticket close failed: {e}");
            }
        }
    }

    if system == "jira" {
        if let (Some(jira_url), Some(user), Some(token)) = (&cfg.jira_url, &cfg.jira_user, &cfg.jira_token) {
            // transition to Done — transition ID 31 is Jira's default "Done"
            let endpoint = format!("{jira_url}/rest/api/3/issue/{ticket_id}/transitions");
            let body = serde_json::json!({ "transition": { "id": "31" } });
            if let Err(e) = client.post(&endpoint).basic_auth(user, Some(token)).json(&body).send().await {
                tracing::warn!("Jira transition failed: {e}");
            }

            // add a comment with the resolution note
            let comment_url = format!("{jira_url}/rest/api/3/issue/{ticket_id}/comment");
            let comment_body = serde_json::json!({
                "body": {
                    "type": "doc", "version": 1,
                    "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": resolution }] }]
                }
            });
            let _ = client.post(&comment_url).basic_auth(user, Some(token)).json(&comment_body).send().await;
        }
    }
}

pub async fn test_connectivity(cfg: &ChangeManagementConfig) -> HashMap<String, bool> {
    let client = reqwest::Client::new();
    let mut results = HashMap::new();

    if let Some(snow_url) = &cfg.servicenow_url {
        let endpoint = format!("{snow_url}/api/now/table/change_request?sysparm_limit=1");
        let mut req = client.get(&endpoint);
        if let (Some(user), Some(pass)) = (&cfg.servicenow_user, &cfg.servicenow_password) {
            req = req.basic_auth(user, Some(pass));
        }
        let ok = req.send().await.map(|r| r.status().is_success()).unwrap_or(false);
        results.insert("servicenow".into(), ok);
    }

    if let (Some(jira_url), Some(user), Some(token)) = (&cfg.jira_url, &cfg.jira_user, &cfg.jira_token) {
        let endpoint = format!("{jira_url}/rest/api/3/myself");
        let ok = client
            .get(&endpoint)
            .basic_auth(user, Some(token))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false);
        results.insert("jira".into(), ok);
    }

    results
}
