use std::io::Write;
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
use maranode_audit::sign;
use maranode_audit::verify::verify_log;
use maranode_common::events::{AuditEntry, AuditEvent};

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

    /// create a backup ZIP of all audit log files and keys
    Backup {
        /// output ZIP path. default: audit_backup_<timestamp>.zip
        #[arg(long, short)]
        output: Option<PathBuf>,

        /// also include all workspace audit logs found in <data_dir>/workspaces/
        #[arg(long)]
        workspaces: bool,
    },

    /// restore audit files from a backup ZIP
    Restore {
        /// path to backup ZIP created by `maranode audit backup`
        #[arg(long, short)]
        from: PathBuf,

        /// overwrite existing files without prompting
        #[arg(long)]
        force: bool,
    },

    /// extract signed inference receipt for a given request id
    Prove {
        /// request_id from the chat response (X-Request-Id header or receipt field)
        record_id: String,
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
            let signing_key = sign::load_or_create(data_dir).ok();
            create_bundle(&log_path, &key_path, &dest, None, signing_key.as_ref())?;
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

        AuditCommand::Backup { output, workspaces } => {
            let ts = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
            let dest = output.unwrap_or_else(|| PathBuf::from(format!("audit_backup_{}.zip", ts)));
            backup_audit(data_dir, &dest, workspaces)?;
            let size = std::fs::metadata(&dest)?.len();
            println!(
                "{} Backup written ({} bytes) → {}",
                "✓".green().bold(),
                size,
                dest.display(),
            );
        }

        AuditCommand::Restore { from, force } => {
            restore_audit(data_dir, &from, force)?;
        }

        AuditCommand::Prove { record_id } => {
            if !log_path.exists() {
                anyhow::bail!("no audit log found at {}", log_path.display());
            }

            let content = std::fs::read_to_string(&log_path)?;
            let receipt = content
                .lines()
                .filter(|l| !l.trim().is_empty())
                .filter_map(|l| serde_json::from_str::<AuditEntry>(l).ok())
                .find_map(|entry| {
                    if let AuditEvent::InferenceReceipt { receipt } = entry.event {
                        if receipt.request_id == record_id {
                            return Some(receipt);
                        }
                    }
                    None
                });

            match receipt {
                Some(r) => {
                    println!("{}", serde_json::to_string_pretty(&r)?);
                }
                None => {
                    eprintln!(
                        "{} No inference receipt found for request id {}",
                        "✗".red().bold(),
                        record_id.yellow(),
                    );
                    std::process::exit(1);
                }
            }
        }
    }

    Ok(())
}

fn backup_audit(data_dir: &Path, dest: &Path, include_workspaces: bool) -> Result<()> {
    use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

    let f = std::fs::File::create(dest)?;
    let mut zip = ZipWriter::new(f);
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    let add = |zip: &mut ZipWriter<std::fs::File>, name: &str, path: &Path| -> Result<()> {
        if path.exists() {
            zip.start_file(name, opts)?;
            zip.write_all(&std::fs::read(path)?)?;
        }
        Ok(())
    };

    add(&mut zip, "audit.jsonl", &default_log_path(data_dir))?;
    add(&mut zip, "audit.key", &default_key_path(data_dir))?;
    add(
        &mut zip,
        "bundle_signing.key",
        &sign::signing_key_path(data_dir),
    )?;
    add(
        &mut zip,
        "bundle_signing.pub",
        &sign::verifying_key_path(data_dir),
    )?;

    if include_workspaces {
        let ws_root = data_dir.join("workspaces");
        if ws_root.is_dir() {
            for entry in std::fs::read_dir(&ws_root)? {
                let entry = entry?;
                if !entry.file_type()?.is_dir() {
                    continue;
                }
                let slug = entry.file_name().to_string_lossy().to_string();
                let ws_dir = ws_root.join(&slug);
                add(
                    &mut zip,
                    &format!("workspaces/{}/audit.jsonl", slug),
                    &default_log_path(&ws_dir),
                )?;
                add(
                    &mut zip,
                    &format!("workspaces/{}/audit.key", slug),
                    &default_key_path(&ws_dir),
                )?;
            }
        }
    }

    zip.finish()?;
    Ok(())
}

fn restore_audit(data_dir: &Path, src: &Path, force: bool) -> Result<()> {
    use std::io::Read;
    use zip::ZipArchive;

    let f = std::fs::File::open(src)?;
    let mut archive = ZipArchive::new(f)?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name().to_string();
        let dest = data_dir.join(&name);

        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }

        if dest.exists() && !force {
            anyhow::bail!(
                "{} already exists; use --force to overwrite",
                dest.display()
            );
        }

        let mut buf = Vec::new();
        entry.read_to_end(&mut buf)?;

        {
            let mut opts = std::fs::OpenOptions::new();
            opts.write(true).create(true).truncate(true);
            if name.ends_with(".key") {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    opts.mode(0o600);
                }
            }
            let mut out = opts.open(&dest)?;
            out.write_all(&buf)?;
        }

        println!("{} Restored → {}", "✓".green().bold(), dest.display());
    }

    let log_path = default_log_path(data_dir);
    if log_path.exists() {
        let key_path = default_key_path(data_dir);
        let key = load_or_generate(&key_path)?;
        let result = verify_log(&log_path, &key)?;
        if result.ok {
            println!(
                "{} Integrity check {} ({} entries).",
                "✓".green().bold(),
                "passed".green().bold(),
                result.entries_checked,
            );
        } else {
            eprintln!(
                "{} Integrity check {} after restore!",
                "✗".red().bold(),
                "FAILED".red().bold(),
            );
            if let Some(v) = result.first_violation {
                eprintln!("  seq {}: {}", v.seq, v.detail);
            }
        }
    }

    Ok(())
}
