use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::Subcommand;
use colored::Colorize;

#[derive(Subcommand)]
pub enum VerifyCommand {
    /// check if network air-gap is active
    Network,
    /// print full health JSON from daemon
    Health,
    /// build runtime integrity attestation report
    Attest {
        /// save JSON report to file instead of stdout
        #[arg(long, short = 'o')]
        output: Option<PathBuf>,
        /// data dir to find audit log and HMAC key
        #[arg(long, env = "MARANODE_DATA_DIR")]
        data_dir: Option<PathBuf>,
    },
}

pub async fn run(cmd: VerifyCommand, host: &str) -> Result<()> {
    match cmd {
        VerifyCommand::Network => verify_network(host).await?,
        VerifyCommand::Health => show_health(host).await?,
        VerifyCommand::Attest { output, data_dir } => {
            let data = data_dir.unwrap_or_else(maranode_common::paths::default_data_dir);
            cmd_attest(&data, output.as_deref())?;
        }
    }
    Ok(())
}

fn cmd_attest(data_dir: &Path, output: Option<&Path>) -> Result<()> {
    use maranode_attestation::report::AttestationReport;
    use maranode_audit::log::{default_key_path, default_log_path};

    println!("{} Generating attestation report…", "·".dimmed());

    let log_path = default_log_path(data_dir);
    let key_path = default_key_path(data_dir);
    let log_opt = log_path.exists().then_some(log_path.as_path());
    let key_opt = key_path.exists().then_some(key_path.as_path());

    let report = AttestationReport::generate(log_opt, key_opt)?;
    let json = serde_json::to_string_pretty(&report)?;

    // binary hash section
    println!(
        "\n  {} Binary",
        "●".cyan().bold(),
    );
    println!("    path    {}", report.binary.path.dimmed());
    println!("    sha256  {}", report.binary.sha256.yellow());
    println!("    size    {} bytes", report.binary.size_bytes);

    // TPM PCR section
    println!("\n  {} TPM", "●".cyan().bold());
    match &report.tpm {
        maranode_attestation::TpmResult::Available { pcrs } => {
            println!("    status  {}", "available".green().bold());
            for (idx, val) in pcrs {
                println!("    PCR{:<3} {}", idx, val.dimmed());
            }
        }
        maranode_attestation::TpmResult::Error { reason } => {
            println!("    status  {}", "error".red());
            println!("    reason  {}", reason);
        }
        maranode_attestation::TpmResult::Unavailable => {
            println!("    status  {}", "unavailable (no TPM device or non-Linux)".yellow());
        }
    }

    // audit log chain section
    if let Some(al) = &report.audit_log {
        println!("\n  {} Audit log", "●".cyan().bold());
        println!("    path    {}", al.path.dimmed());
        println!("    entries {}", al.entries);
        let chain = if al.chain_ok {
            "ok".green().bold().to_string()
        } else {
            "VIOLATED".red().bold().to_string()
        };
        println!("    chain   {}", chain);
        if let Some(detail) = &al.chain_detail {
            println!("    detail  {}", detail.dimmed());
        }
    }

    println!("\n  {} Report hash", "●".cyan().bold());
    if let Some(h) = &report.report_sha256 {
        println!("    {}", h.yellow());
    }

    match output {
        Some(path) => {
            std::fs::write(path, &json)?;
            println!(
                "\n{} Report written to {}",
                "✓".green().bold(),
                path.display().to_string().cyan(),
            );
        }
        None => {
            println!("\n{}\n{}", "── full report ──".dimmed(), json);
        }
    }

    Ok(())
}

async fn show_health(host: &str) -> Result<()> {
    let url = format!("{}/health", host.trim_end_matches('/'));
    match reqwest::get(&url).await {
        Ok(resp) => {
            let json: serde_json::Value = resp.json().await?;
            println!("{}", serde_json::to_string_pretty(&json)?);
        }
        Err(e) => {
            eprintln!(
                "{} Could not reach daemon at {}\n  {}",
                "✗".red().bold(),
                host.cyan(),
                e
            );
            std::process::exit(1);
        }
    }
    Ok(())
}

async fn verify_network(host: &str) -> Result<()> {
    println!("{} Verifying network isolation…\n", "·".dimmed());

    let daemon_result = check_daemon_flag(host).await;
    match &daemon_result {
        Ok(true) => println!(
            "  {} Daemon reports air-gap {}",
            "✓".green(),
            "ACTIVE".green().bold()
        ),
        Ok(false) => println!(
            "  {} Daemon reports air-gap {}",
            "⚠".yellow(),
            "INACTIVE".yellow().bold()
        ),
        Err(e) => println!("  {} Daemon unreachable: {}", "?".dimmed(), e),
    }

    let iptables_result = check_iptables();
    match &iptables_result {
        Ok(summary) => println!("  {} iptables: {}", "✓".green(), summary),
        Err(e) => println!("  {} iptables check skipped: {}", "·".dimmed(), e),
    }

    // try TCP connect to public IPs (1.1.1.1:443, 8.8.8.8:53, etc.).
    // If connect works, outbound traffic is not blocked.
    let probes = [
        ("1.1.1.1", 443u16, "Cloudflare (1.1.1.1:443)"),
        ("8.8.8.8", 53u16, "Google DNS (8.8.8.8:53)"),
        ("93.184.216.34", 80, "example.com (93.184.216.34:80)"),
    ];

    let mut any_reachable = false;
    for (ip, port, label) in &probes {
        let addr = format!("{}:{}", ip, port);
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            tokio::net::TcpStream::connect(&addr),
        )
        .await;

        match result {
            Ok(Ok(_)) => {
                println!(
                    "  {} {} is reachable: outbound traffic NOT blocked",
                    "✗".red().bold(),
                    label
                );
                any_reachable = true;
            }
            Ok(Err(_)) | Err(_) => {
                println!(
                    "  {} {} unreachable (blocked or timeout)",
                    "✓".green(),
                    label
                );
            }
        }
    }

    println!();
    let air_gap_on = daemon_result.unwrap_or(false);
    if !any_reachable && air_gap_on {
        println!(
            "{} Air-gap isolation is {}. All outbound probes blocked.",
            "✓".green().bold(),
            "VERIFIED".green().bold(),
        );
    } else if any_reachable {
        println!(
            "{} Air-gap isolation is {}. Outbound traffic is NOT blocked.",
            "✗".red().bold(),
            "NOT ENFORCED".red().bold(),
        );
        std::process::exit(1);
    } else {
        println!(
            "{} Outbound probes blocked but air-gap flag is off. \
             Check daemon configuration.",
            "⚠".yellow(),
        );
    }

    Ok(())
}

async fn check_daemon_flag(host: &str) -> Result<bool> {
    let url = format!("{}/health", host.trim_end_matches('/'));
    let resp = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()?
        .get(&url)
        .send()
        .await?;
    let json: serde_json::Value = resp.json().await?;
    Ok(json["air_gap"].as_bool().unwrap_or(false))
}

fn check_iptables() -> Result<String> {
    // iptables check runs on Linux only. Skip on other OS.
    #[cfg(not(target_os = "linux"))]
    return Err(anyhow::anyhow!("not on Linux"));

    #[cfg(target_os = "linux")]
    {
        let out = std::process::Command::new("iptables-save")
            .output()
            .map_err(|e| anyhow::anyhow!("iptables-save: {}", e))?;

        if !out.status.success() {
            return Err(anyhow::anyhow!("iptables-save failed (run as root?)"));
        }

        let rules = String::from_utf8_lossy(&out.stdout);
        let has_drop_output = rules
            .lines()
            .any(|l| l.contains("-P OUTPUT DROP") || l.contains("-P FORWARD DROP"));
        let maranode_rules = rules
            .lines()
            .filter(|l| l.contains("maranode") || l.contains("11984"))
            .count();

        if has_drop_output {
            Ok(format!(
                "OUTPUT policy is DROP, {} maranode-related rule(s) present",
                maranode_rules
            ))
        } else {
            Ok(format!(
                "OUTPUT policy is ACCEPT (not air-gapped), {} maranode-related rule(s)",
                maranode_rules
            ))
        }
    }
}
