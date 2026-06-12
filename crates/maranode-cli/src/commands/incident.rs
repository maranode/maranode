use anyhow::Result;
use clap::Subcommand;

#[derive(Subcommand)]
pub enum IncidentCommand {
    /// declare an active incident — ends user sessions and freezes the audit log
    Declare {
        #[arg(long)]
        reason: String,
        /// webhook URLs to notify on phase changes (comma-separated)
        #[arg(long)]
        webhooks: Option<String>,
    },

    /// move the incident to 'investigating' phase
    Investigate {
        #[arg(long)]
        note: Option<String>,
    },

    /// resolve the incident and unfreeze the audit log
    Resolve {
        #[arg(long)]
        summary: String,
    },

    /// show current incident status
    Status,

    /// take a forensic snapshot of the current runtime state
    Snapshot,

    /// generate a new single-use break-glass credential
    BgGenerate {
        #[arg(long, default_value = "emergency-access")]
        purpose: String,
    },

    /// use a break-glass credential (forces a mandatory audit event)
    BgUse {
        #[arg(long)]
        token: String,
    },
}

pub async fn run(cmd: IncidentCommand, host: &str) -> Result<()> {
    let client = reqwest::Client::new();

    match cmd {
        IncidentCommand::Declare { reason, webhooks } => {
            let webhooks_list: Vec<String> = webhooks
                .as_deref()
                .unwrap_or("")
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();

            let body = serde_json::json!({
                "reason": reason,
                "webhook_urls": webhooks_list,
            });
            let resp: serde_json::Value = client
                .post(format!("{host}/v1/incident/declare"))
                .json(&body)
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            println!("Incident declared!");
            println!("  ID:    {}", resp["incident_id"].as_str().unwrap_or("?"));
            println!("  Phase: {}", resp["phase"].as_str().unwrap_or("?"));
            println!("  Audit frozen: {}", resp["audit_frozen"]);
        }

        IncidentCommand::Investigate { note } => {
            let body = serde_json::json!({ "note": note });
            let resp: serde_json::Value = client
                .post(format!("{host}/v1/incident/investigate"))
                .json(&body)
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            println!("Incident {} moved to phase: {}", resp["incident_id"], resp["phase"]);
        }

        IncidentCommand::Resolve { summary } => {
            let body = serde_json::json!({ "summary": summary });
            let resp: serde_json::Value = client
                .post(format!("{host}/v1/incident/resolve"))
                .json(&body)
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            println!("Incident {} resolved.", resp["incident_id"]);
        }

        IncidentCommand::Status => {
            let resp: serde_json::Value = client
                .get(format!("{host}/v1/incident/status"))
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            if resp["active"].as_bool().unwrap_or(false) {
                println!("Active incident: {}", resp["incident_id"].as_str().unwrap_or("?"));
                println!("  Phase:       {}", resp["phase"].as_str().unwrap_or("?"));
                println!("  Declared at: {}", resp["declared_at"].as_str().unwrap_or("?"));
                println!("  Declared by: {}", resp["declared_by"].as_str().unwrap_or("?"));
                println!("  Reason:      {}", resp["reason"].as_str().unwrap_or("?"));
                println!("  Audit frozen:{}", resp["audit_frozen"]);
            } else {
                println!("No active incident.");
                println!("  Audit frozen: {}", resp["audit_frozen"]);
            }
        }

        IncidentCommand::Snapshot => {
            let resp: serde_json::Value = client
                .post(format!("{host}/v1/incident/snapshot"))
                .json(&serde_json::json!({}))
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            println!("Forensic snapshot saved:");
            println!("  Path:   {}", resp["snapshot_path"].as_str().unwrap_or("?"));
            println!("  SHA256: {}", resp["sha256"].as_str().unwrap_or("?"));
        }

        IncidentCommand::BgGenerate { purpose } => {
            let body = serde_json::json!({ "purpose": purpose });
            let resp: serde_json::Value = client
                .post(format!("{host}/v1/incident/break-glass/generate"))
                .json(&body)
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            println!("Break-glass credential generated:");
            println!("  ID:      {}", resp["cred_id"].as_str().unwrap_or("?"));
            println!("  Purpose: {}", resp["purpose"].as_str().unwrap_or("?"));
            println!("  Token:   {}", resp["token"].as_str().unwrap_or("?"));
            println!("  (Store this token securely — it will not be shown again)");
        }

        IncidentCommand::BgUse { token } => {
            let body = serde_json::json!({ "token": token });
            let resp: serde_json::Value = client
                .post(format!("{host}/v1/incident/break-glass/use"))
                .json(&body)
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            println!("Break-glass token accepted:");
            println!("  ID:      {}", resp["cred_id"].as_str().unwrap_or("?"));
            println!("  Purpose: {}", resp["purpose"].as_str().unwrap_or("?"));
            println!("  Used at: {}", resp["used_at"].as_str().unwrap_or("?"));
        }
    }

    Ok(())
}
