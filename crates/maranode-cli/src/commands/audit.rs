use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::Subcommand;
use colored::Colorize;

use crate::errors::did_you_mean;
use maranode_audit::bundle::create_bundle;
use maranode_audit::export::{export_gdpr, export_hipaa, export_iso27001, export_soc2, ExportFilter};
use maranode_audit::key::load_or_generate;
use maranode_audit::log::{default_key_path, default_log_path};
use maranode_audit::retention::prune_log;
use maranode_audit::verify::verify_log;

#[derive(Subcommand)]
pub enum AuditCommand {
    /// check HMAC chain integrity of audit log
    Verify,

    Tail {
        #[arg(short, default_value_t = 20)]
        n: usize,
    },

    /// export audit log as CSV for compliance
    Export {
        /// export format: gdpr, hipaa, soc2, or iso27001
        #[arg(long)]
        format: String,

        /// only rows for this workspace actor
        #[arg(long)]
        workspace: Option<String>,

        /// start time in RFC 3339, e.g. 2024-01-01T00:00:00Z
        #[arg(long)]
        from: Option<String>,

        /// end time in RFC 3339
        #[arg(long)]
        to: Option<String>,

        /// output CSV path. default: audit_<format>.csv
        #[arg(long, short)]
        output: Option<PathBuf>,
    },

    /// create ZIP bundle with log, integrity report, and manifest
    Bundle {
        /// output ZIP path. default: audit_bundle.zip
        #[arg(long, short)]
        output: Option<PathBuf>,
    },

    /// remove audit log entries older than N days
    Prune {
        #[arg(long)]
        retain_days: u32,

        /// really delete rows. without flag, only count stale entries
        #[arg(long)]
        confirm: bool,
    },
}

pub async fn run(cmd: AuditCommand, data_dir: &Path) -> Result<()> {
    let log_path = default_log_path(data_dir);
    let key_path = default_key_path(data_dir);

    match cmd {
        AuditCommand::Verify => {
            if !log_path.exists() {
                println!(
                    "{} No audit log found at {}",
                    "·".dimmed(),
                    log_path.display()
                );
                return Ok(());
            }
            let key = load_or_generate(&key_path)?;
            let result = verify_log(&log_path, &key)?;

            if result.ok {
                println!(
                    "{} Audit log {}: {} entries, HMAC chain intact.",
                    "✓".green().bold(),
                    "OK".green().bold(),
                    result.entries_checked,
                );
            } else {
                eprintln!(
                    "{} Audit log {} detected!",
                    "✗".red().bold(),
                    "INTEGRITY VIOLATION".red().bold(),
                );
                if let Some(v) = result.first_violation {
                    eprintln!("  At sequence {}: {}", v.seq.to_string().yellow(), v.detail);
                }
                std::process::exit(1);
            }
        }

        AuditCommand::Tail { n } => {
            if !log_path.exists() {
                println!("{} No audit log found.", "·".dimmed());
                return Ok(());
            }
            let content = std::fs::read_to_string(&log_path)?;
            let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
            let start = lines.len().saturating_sub(n);
            for line in &lines[start..] {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                    println!(
                        "{}  {}  {}  {}",
                        v["ts"].as_str().unwrap_or("?").dimmed(),
                        format!("seq={}", v["seq"].as_u64().unwrap_or(0)).yellow(),
                        format!("actor={}", v["actor"].as_str().unwrap_or("?")).cyan(),
                        v["event"].as_str().unwrap_or("?").bold(),
                    );
                }
            }
        }

        AuditCommand::Export {
            format,
            workspace,
            from,
            to,
            output,
        } => {
            if !log_path.exists() {
                println!("{} No audit log found.", "·".dimmed());
                return Ok(());
            }

            let filter = ExportFilter {
                workspace,
                from: from
                    .as_deref()
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .map(|d| d.with_timezone(&chrono::Utc)),
                to: to
                    .as_deref()
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .map(|d| d.with_timezone(&chrono::Utc)),
            };

            const FORMATS: &[&str] = &["gdpr", "hipaa", "soc2", "iso27001"];
            let csv = match format.as_str() {
                "gdpr" => export_gdpr(&log_path, &filter)?,
                "hipaa" => export_hipaa(&log_path, &filter)?,
                "soc2" => export_soc2(&log_path, &filter)?,
                "iso27001" => export_iso27001(&log_path, &filter)?,
                other => {
                    let hint = did_you_mean(other, FORMATS)
                        .map(|s| format!("  Did you mean {}?", s.cyan()))
                        .unwrap_or_default();
                    anyhow::bail!(
                        "unknown format '{}'.{}\n  Valid formats: {}",
                        other.yellow(),
                        hint,
                        FORMATS.join(", "),
                    )
                }
            };

            let dest = output.unwrap_or_else(|| PathBuf::from(format!("audit_{}.csv", format)));
            std::fs::write(&dest, &csv)?;
            println!(
                "{} Exported {} ({} bytes) → {}",
                "✓".green().bold(),
                format.cyan(),
                csv.len(),
                dest.display(),
            );
        }

        AuditCommand::Bundle { output } => {
            if !log_path.exists() {
                println!("{} No audit log found.", "·".dimmed());
                return Ok(());
            }
            let dest = output.unwrap_or_else(|| PathBuf::from("audit_bundle.zip"));
            create_bundle(&log_path, &key_path, &dest, None)?;
            let size = std::fs::metadata(&dest)?.len();
            println!(
                "{} Bundle created ({} bytes) → {}",
                "✓".green().bold(),
                size,
                dest.display(),
            );
        }

        AuditCommand::Prune {
            retain_days,
            confirm,
        } => {
            if !log_path.exists() {
                println!("{} No audit log found.", "·".dimmed());
                return Ok(());
            }
            if !confirm {
                // dry run: count old entries only
                let cutoff = chrono::Utc::now() - chrono::Duration::days(retain_days as i64);
                let content = std::fs::read_to_string(&log_path)?;
                let stale = content
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
                    .filter(|v| {
                        v["ts"]
                            .as_str()
                            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                            .map(|t| t.with_timezone(&chrono::Utc) < cutoff)
                            .unwrap_or(false)
                    })
                    .count();
                println!(
                    "{} Dry run: {} entries older than {} days would be pruned. Re-run with --confirm to apply.",
                    "·".dimmed(),
                    stale.to_string().yellow(),
                    retain_days,
                );
                return Ok(());
            }
            let pruned = prune_log(&log_path, retain_days)?;
            println!(
                "{} Pruned {} entries older than {} days.",
                "✓".green().bold(),
                pruned.to_string().yellow(),
                retain_days,
            );
        }
    }

    Ok(())
}
