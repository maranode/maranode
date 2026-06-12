use anyhow::Result;
use clap::Subcommand;
use std::path::Path;

use maranode_common::approval::ApprovalToken;

#[derive(Subcommand)]
pub enum RegistryCommand {
    /// submit a model sha256 for approval review
    Submit {
        #[arg(long)]
        model: String,
        #[arg(long)]
        sha256: String,
        #[arg(long)]
        by: String,
        #[arg(long)]
        note: Option<String>,
    },
    /// list pending and historical submissions
    List,
    /// list issued approval tokens
    Tokens,
    /// approve a model and issue a signed token
    Approve {
        #[arg(long)]
        sha256: String,
        #[arg(long)]
        by: String,
        #[arg(long)]
        note: Option<String>,
        /// expire token after N days
        #[arg(long)]
        expires_in_days: Option<i64>,
    },
    /// revoke an approval token
    Revoke {
        #[arg(long)]
        sha256: String,
        #[arg(long)]
        by: String,
    },
    /// export a token to a file for airgapped transfer
    ExportToken {
        #[arg(long)]
        sha256: String,
        #[arg(long)]
        out: std::path::PathBuf,
    },
    /// import a token file (useful for airgapped setups)
    ImportToken {
        path: std::path::PathBuf,
    },
    /// verify a token file signature and print its content
    VerifyToken {
        path: std::path::PathBuf,
    },
    /// open the approval web UI in the default browser
    Ui,
    /// test connectivity to configured change management systems
    HooksTest,
}

pub async fn run(cmd: RegistryCommand, data_dir: &Path, host: &str) -> Result<()> {
    let client = reqwest::Client::new();

    match cmd {
        RegistryCommand::Submit { model, sha256, by, note } => {
            let mut body = serde_json::json!({
                "model_id": model,
                "model_sha256": sha256,
                "submitted_by": by,
            });
            if let Some(n) = note {
                body["note"] = serde_json::Value::String(n);
            }
            let resp = client
                .post(format!("{host}/v1/registry/submit"))
                .json(&body)
                .send()
                .await?;
            let status = resp.status();
            let text = resp.text().await?;
            if status.is_success() {
                let rec: serde_json::Value = serde_json::from_str(&text)?;
                println!(
                    "Submitted  id={} sha={}…",
                    rec["submission_id"].as_str().unwrap_or("?"),
                    &sha256[..12.min(sha256.len())]
                );
            } else {
                anyhow::bail!("submit failed ({status}): {text}");
            }
        }

        RegistryCommand::List => {
            let resp = client
                .get(format!("{host}/v1/registry/pending"))
                .send()
                .await?
                .error_for_status()?
                .json::<Vec<serde_json::Value>>()
                .await?;
            if resp.is_empty() {
                println!("no submissions");
                return Ok(());
            }
            println!("{:<20} {:<16} {:<20} {:<12}", "model", "sha256", "submitted_by", "status");
            println!("{}", "-".repeat(72));
            for r in &resp {
                println!(
                    "{:<20} {:<16} {:<20} {:<12}",
                    r["model_id"].as_str().unwrap_or("?"),
                    &r["model_sha256"].as_str().unwrap_or("?")[..12.min(r["model_sha256"].as_str().unwrap_or("?").len())],
                    r["submitted_by"].as_str().unwrap_or("?"),
                    r["status"].as_str().unwrap_or("?"),
                );
            }
        }

        RegistryCommand::Tokens => {
            let resp = client
                .get(format!("{host}/v1/registry/tokens"))
                .send()
                .await?
                .error_for_status()?
                .json::<Vec<serde_json::Value>>()
                .await?;
            if resp.is_empty() {
                println!("no tokens issued");
                return Ok(());
            }
            println!("{:<20} {:<16} {:<20} {:<26}", "model", "sha256", "approved_by", "approved_at");
            println!("{}", "-".repeat(82));
            for t in &resp {
                println!(
                    "{:<20} {:<16} {:<20} {:<26}",
                    t["model_id"].as_str().unwrap_or("?"),
                    &t["model_sha256"].as_str().unwrap_or("?")[..12.min(t["model_sha256"].as_str().unwrap_or("?").len())],
                    t["approved_by"].as_str().unwrap_or("?"),
                    t["approved_at"].as_str().unwrap_or("?"),
                );
            }
        }

        RegistryCommand::Approve { sha256, by, note, expires_in_days } => {
            let mut body = serde_json::json!({ "approved_by": by });
            if let Some(n) = note {
                body["note"] = serde_json::Value::String(n);
            }
            if let Some(d) = expires_in_days {
                body["expires_in_days"] = serde_json::Value::Number(d.into());
            }
            let resp = client
                .post(format!("{host}/v1/registry/approve/{sha256}"))
                .json(&body)
                .send()
                .await?;
            let status = resp.status();
            let text = resp.text().await?;
            if status.is_success() {
                let tok: serde_json::Value = serde_json::from_str(&text)?;
                println!(
                    "Approved   token_id={} sha={}… approved_by={}",
                    tok["token_id"].as_str().unwrap_or("?"),
                    &sha256[..12.min(sha256.len())],
                    by,
                );
            } else {
                anyhow::bail!("approve failed ({status}): {text}");
            }
        }

        RegistryCommand::Revoke { sha256, by } => {
            let resp = client
                .post(format!("{host}/v1/registry/revoke/{sha256}"))
                .json(&serde_json::json!({ "revoked_by": by }))
                .send()
                .await?;
            if resp.status().is_success() {
                println!("Revoked sha={}…", &sha256[..12.min(sha256.len())]);
            } else {
                anyhow::bail!("revoke failed ({}): {}", resp.status(), resp.text().await?);
            }
        }

        RegistryCommand::ExportToken { sha256, out } => {
            let tokens_dir = data_dir.join("approval-tokens");
            let src = ApprovalToken::token_path(&tokens_dir, &sha256);
            if !src.exists() {
                anyhow::bail!("no token found for sha256 {}", &sha256[..12.min(sha256.len())]);
            }
            let token = ApprovalToken::load(&src)?;
            token.verify().map_err(|e| anyhow::anyhow!("token is invalid: {e}"))?;
            token.save(&out)?;
            println!("Exported to {}", out.display());
        }

        RegistryCommand::ImportToken { path } => {
            let token = ApprovalToken::load(&path)?;
            token.verify().map_err(|e| anyhow::anyhow!("token signature invalid: {e}"))?;
            let tokens_dir = data_dir.join("approval-tokens");
            let dest = ApprovalToken::token_path(&tokens_dir, &token.model_sha256);
            token.save(&dest)?;
            println!(
                "Imported   model={} sha={}… token_id={}",
                token.model_id,
                &token.model_sha256[..12.min(token.model_sha256.len())],
                token.token_id,
            );
        }

        RegistryCommand::VerifyToken { path } => {
            let token = ApprovalToken::load(&path)?;
            match token.verify() {
                Ok(_) => {
                    println!("Signature  OK");
                    println!("token_id   {}", token.token_id);
                    println!("model_id   {}", token.model_id);
                    println!("sha256     {}", token.model_sha256);
                    println!("approved   {} by {}", token.approved_at, token.approved_by);
                    if let Some(exp) = token.expires_at {
                        println!("expires    {exp}");
                    }
                    if let Some(note) = &token.note {
                        println!("note       {note}");
                    }
                    println!("pubkey     {}…", &token.signer_pubkey[..16.min(token.signer_pubkey.len())]);
                }
                Err(e) => {
                    println!("Signature  INVALID — {e}");
                    std::process::exit(1);
                }
            }
        }

        RegistryCommand::Ui => {
            let url = format!("{host}/v1/registry/ui");
            println!("Opening {url}");
            open_url(&url);
        }

        RegistryCommand::HooksTest => {
            let resp = client
                .post(format!("{host}/v1/registry/hooks/test"))
                .send()
                .await?;
            let status = resp.status();
            let text = resp.text().await?;
            if status.is_success() {
                let results: serde_json::Value = serde_json::from_str(&text)?;
                if let Some(obj) = results.as_object() {
                    if obj.is_empty() {
                        println!("no change management systems configured");
                    }
                    for (system, ok) in obj {
                        let indicator = if ok.as_bool().unwrap_or(false) { "OK" } else { "FAIL" };
                        println!("{system:<20} {indicator}");
                    }
                }
            } else {
                anyhow::bail!("hooks test failed ({status}): {text}");
            }
        }
    }

    Ok(())
}

fn open_url(url: &str) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("start").arg(url).spawn();
}
