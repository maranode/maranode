use anyhow::{Context, Result};
use clap::Subcommand;
use colored::Colorize;
use serde::Deserialize;

#[derive(Subcommand)]
pub enum AdminCommand {
    ConfigReload,
}

#[derive(Debug, Deserialize)]
struct ReloadResponse {
    path: Option<String>,
    applied: Vec<String>,
    requires_restart: Vec<String>,
}

pub async fn reload_config(host: &str, admin_key: Option<&str>) -> Result<()> {
    let base = host.trim_end_matches('/');
    let url = format!("{base}/v1/admin/config/reload");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let mut req = client.post(&url);
    if let Some(key) = admin_key.filter(|k| !k.is_empty()) {
        req = req.bearer_auth(key);
    }

    let resp = req
        .send()
        .await
        .with_context(|| format!("POST {url}"))?
        .error_for_status()
        .context("config reload rejected (is auth.admin_key set?)")?;

    let body: ReloadResponse = resp.json().await.context("parse reload response")?;

    if let Some(path) = &body.path {
        println!("{} reloaded {}", "●".green().bold(), path.bold());
    } else {
        println!("{} config reloaded", "●".green().bold());
    }

    if !body.applied.is_empty() {
        println!("  applied: {}", body.applied.join(", "));
    }
    if !body.requires_restart.is_empty() {
        println!(
            "  {} {}",
            "requires daemon restart:".yellow().bold(),
            body.requires_restart.join(", ")
        );
    }

    Ok(())
}
