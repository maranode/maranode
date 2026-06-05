use anyhow::Result;
use colored::Colorize;

pub async fn run(host: &str) -> Result<()> {
    let base = host.trim_end_matches('/');
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;

    let health: serde_json::Value = client
        .get(format!("{}/health", base))
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("cannot reach daemon at {}: {}", base.cyan(), e))?
        .json()
        .await?;

    let stats = match client.get(format!("{}/stats", base)).send().await {
        Ok(r) => r.json::<serde_json::Value>().await.unwrap_or_default(),
        Err(_) => serde_json::Value::Null,
    };

    let models = match client
        .get(format!("{}/v1/models/details", base))
        .send()
        .await
    {
        Ok(r) => r.json::<serde_json::Value>().await.unwrap_or_default(),
        Err(_) => serde_json::Value::Null,
    };
    let model_count = models.as_array().map(|a| a.len()).unwrap_or(0);

    let version = health["version"].as_str().unwrap_or("?");
    let air_gap = health["air_gap"].as_bool().unwrap_or(false);
    let requests = stats["requests"].as_u64().unwrap_or(0);
    let errors = stats["errors"].as_u64().unwrap_or(0);
    let tokens_out = stats["tokens_out"].as_u64().unwrap_or(0);
    let avg_latency = stats["avg_latency_ms"].as_u64().unwrap_or(0);
    let queue_depth = stats["queue_depth"].as_u64().unwrap_or(0);
    let queue_max = stats["queue_max"].as_u64().unwrap_or(0);
    let uptime_secs = stats["uptime_secs"].as_u64().unwrap_or(0);

    let uptime = format_uptime(uptime_secs);

    println!("{} {}", "●".green().bold(), "maranoded".bold());
    println!("  Version    {}", version.cyan());
    println!("  Uptime     {}", uptime.cyan());
    println!(
        "  Air-gap    {}",
        if air_gap {
            "active".green().bold()
        } else {
            "off".yellow().bold()
        }
    );
    println!("  Models     {}", model_count.to_string().cyan());
    println!();
    println!("{}", "Statistics".bold());
    println!(
        "  Requests   {} ({} errors)",
        requests.to_string().cyan(),
        errors
    );
    println!("  Tokens out {}", tokens_out.to_string().cyan());
    println!("  Avg lat.   {} ms", avg_latency.to_string().cyan());
    println!(
        "  Queue      {}/{}",
        queue_depth.to_string().cyan(),
        queue_max
    );

    Ok(())
}

fn format_uptime(secs: u64) -> String {
    let d = secs / 86400;
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    if d > 0 {
        format!("{}d {}h {}m", d, h, m)
    } else if h > 0 {
        format!("{}h {}m", h, m)
    } else {
        format!("{}m {}s", m, secs % 60)
    }
}
