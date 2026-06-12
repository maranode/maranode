use anyhow::Result;
use clap::Subcommand;
use serde_json::json;

#[derive(Subcommand)]
pub enum DlpCommand {
    /// sync data labels from a DLP provider into the classification policy
    Sync {
        /// provider: purview, forcepoint, or symantec
        #[arg(long)]
        provider: String,
        /// Purview: Azure tenant ID
        #[arg(long)]
        tenant_id: Option<String>,
        /// Purview: Azure app registration client ID
        #[arg(long)]
        client_id: Option<String>,
        /// Purview: Azure client secret
        #[arg(long)]
        client_secret: Option<String>,
        /// Forcepoint / Symantec: base URL of the management server
        #[arg(long)]
        url: Option<String>,
        /// Forcepoint / Symantec: username
        #[arg(long)]
        username: Option<String>,
        /// Forcepoint / Symantec: password
        #[arg(long)]
        password: Option<String>,
    },
}

pub async fn run(cmd: DlpCommand, host: &str) -> Result<()> {
    match cmd {
        DlpCommand::Sync {
            provider,
            tenant_id,
            client_id,
            client_secret,
            url,
            username,
            password,
        } => {
            let mut body = json!({ "provider": provider });

            match provider.to_lowercase().as_str() {
                "purview" => {
                    if let (Some(t), Some(c), Some(s)) = (&tenant_id, &client_id, &client_secret) {
                        body["purview"] = json!({
                            "tenant_id": t,
                            "client_id": c,
                            "client_secret": s,
                        });
                    }
                }
                "forcepoint" => {
                    if let (Some(u), Some(n), Some(p)) = (&url, &username, &password) {
                        body["forcepoint"] = json!({
                            "base_url": u,
                            "username": n,
                            "password": p,
                        });
                    }
                }
                "symantec" => {
                    if let (Some(u), Some(n), Some(p)) = (&url, &username, &password) {
                        body["symantec"] = json!({
                            "enforce_url": u,
                            "username": n,
                            "password": p,
                        });
                    }
                }
                _ => {}
            }

            let client = reqwest::Client::new();
            let resp = client
                .post(format!("{host}/v1/dlp/sync"))
                .json(&body)
                .send()
                .await?;

            let status = resp.status();
            let text = resp.text().await?;

            if !status.is_success() {
                anyhow::bail!("DLP sync failed ({}): {}", status, text);
            }

            let data: serde_json::Value = serde_json::from_str(&text)?;
            let imported = data["labels_imported"].as_u64().unwrap_or(0);
            println!("Synced {} label(s) from {}", imported, provider);
            if let Some(cols) = data["collections"].as_array() {
                for col in cols {
                    println!(
                        "  {} -> {}",
                        col["collection"].as_str().unwrap_or("?"),
                        col["label"].as_str().unwrap_or("?")
                    );
                }
            }

            Ok(())
        }
    }
}
