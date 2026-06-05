//! air-gap firewall rules using the iptables command

use anyhow::{Context, Result};

use super::state::IsolationConfig;

const CHAIN_COMMENT: &str = "maranode-managed";

pub fn apply_air_gap(config: &IsolationConfig) -> Result<()> {
    run_iptables(&["-P", "OUTPUT", "DROP"])?;
    run_iptables(&["-P", "INPUT", "DROP"])?;
    run_iptables(&["-P", "FORWARD", "DROP"])?;

    insert_if_absent(&["-A", "INPUT", "-i", "lo", "-j", "ACCEPT"])?;
    insert_if_absent(&["-A", "OUTPUT", "-o", "lo", "-j", "ACCEPT"])?;

    insert_if_absent(&[
        "-A",
        "INPUT",
        "-m",
        "state",
        "--state",
        "ESTABLISHED,RELATED",
        "-j",
        "ACCEPT",
    ])?;

    let port = config.api_port.to_string();
    for source in &config.api_allowed_sources {
        insert_if_absent(&[
            "-A",
            "INPUT",
            "-p",
            "tcp",
            "--dport",
            &port,
            "-s",
            source,
            "-j",
            "ACCEPT",
            "-m",
            "comment",
            "--comment",
            CHAIN_COMMENT,
        ])?;
    }

    Ok(())
}

pub fn apply_whitelist(config: &IsolationConfig) -> Result<()> {
    apply_air_gap(config)?;

    for entry in &config.whitelist {
        let port = entry.port.to_string();
        insert_if_absent(&[
            "-A",
            "OUTPUT",
            "-p",
            "tcp",
            "--dport",
            &port,
            "-d",
            &entry.host,
            "-j",
            "ACCEPT",
            "-m",
            "comment",
            "--comment",
            CHAIN_COMMENT,
        ])?;
    }

    Ok(())
}

pub fn check_rules_present(_config: &IsolationConfig) -> Result<bool> {
    let output = std::process::Command::new("iptables-save")
        .output()
        .context("running iptables-save")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.contains(CHAIN_COMMENT))
}

pub fn remove_rules() -> Result<()> {
    let _ = run_iptables(&["-F"]);
    let _ = run_iptables(&["-P", "INPUT", "ACCEPT"]);
    let _ = run_iptables(&["-P", "OUTPUT", "ACCEPT"]);
    let _ = run_iptables(&["-P", "FORWARD", "ACCEPT"]);
    Ok(())
}

fn run_iptables(args: &[&str]) -> Result<()> {
    let status = std::process::Command::new("iptables")
        .args(args)
        .status()
        .context("running iptables")?;

    if !status.success() {
        anyhow::bail!("iptables {:?} exited with status {}", args, status);
    }
    Ok(())
}

fn insert_if_absent(add_args: &[&str]) -> Result<()> {
    let check_args: Vec<&str> = add_args
        .iter()
        .map(|a| if *a == "-A" { "-C" } else { a })
        .collect();

    let status = std::process::Command::new("iptables")
        .args(&check_args)
        .status()
        .context("running iptables -C")?;

    if status.success() {
        return Ok(());
    }

    run_iptables(add_args)
}
