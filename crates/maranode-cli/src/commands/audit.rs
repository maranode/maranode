use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::Subcommand;
use colored::Colorize;

use crate::errors::did_you_mean;
use maranode_audit::bundle::create_bundle;
use maranode_audit::export::{
    export_cef, export_gdpr, export_hipaa, export_iso27001, export_leef, export_soc2, ExportFilter,
};
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

    /// re-run inference for a record and compare output hash to the stored receipt
    Replay {
        /// request_id to replay
        record_id: String,
    },

    /// verify RAG source hashes from a stored receipt against the live RAG store
    VerifySources {
        /// request_id of the inference whose sources to verify
        record_id: String,
    },

    /// export the deletion certificate for a shredded workspace
    ExportCert {
        /// workspace slug (e.g. "default")
        workspace: String,

        /// output file path. default: deletion_cert_<workspace>.txt
        #[arg(long, short)]
        output: Option<PathBuf>,
    },

    /// forward audit log to a SIEM over syslog (CEF over TCP/UDP RFC 5424)
    Forward {
        /// syslog destination, e.g. siem.corp.local:514 or 10.0.0.5:6514
        #[arg(long)]
        target: String,

        /// transport: tcp or udp (default: tcp)
        #[arg(long, default_value = "tcp")]
        transport: String,

        /// start time in RFC 3339; only forward events after this time
        #[arg(long)]
        from: Option<String>,

        /// end time in RFC 3339
        #[arg(long)]
        to: Option<String>,
    },

    /// show isolation probe timeline from the audit log
    IsolationReport {
        /// start time in RFC 3339, e.g. 2024-01-01T00:00:00Z
        #[arg(long)]
        from: Option<String>,

        /// end time in RFC 3339
        #[arg(long)]
        to: Option<String>,

        /// only show events where isolation was broken
        #[arg(long)]
        drift_only: bool,
    },
}

pub async fn run(cmd: AuditCommand, data_dir: &Path, host: &str) -> Result<()> {
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

            const FORMATS: &[&str] = &["gdpr", "hipaa", "soc2", "iso27001", "cef", "leef"];
            let (body, ext) = match format.as_str() {
                "gdpr"     => (export_gdpr(&log_path, &filter)?,     "csv"),
                "hipaa"    => (export_hipaa(&log_path, &filter)?,    "csv"),
                "soc2"     => (export_soc2(&log_path, &filter)?,     "csv"),
                "iso27001" => (export_iso27001(&log_path, &filter)?, "csv"),
                "cef"      => (export_cef(&log_path, &filter)?,      "cef"),
                "leef"     => (export_leef(&log_path, &filter)?,     "leef"),
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

            let dest = output.unwrap_or_else(|| PathBuf::from(format!("audit_{}.{}", format, ext)));
            std::fs::write(&dest, &body)?;
            println!(
                "{} Exported {} ({} bytes) → {}",
                "✓".green().bold(),
                format.cyan(),
                body.len(),
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

        AuditCommand::Replay { record_id } => {
            replay_inference(&log_path, &record_id, host).await?;
        }

        AuditCommand::ExportCert { workspace, output } => {
            export_deletion_cert(&log_path, &workspace, output.as_deref())?;
        }

        AuditCommand::VerifySources { record_id } => {
            verify_sources(&log_path, &record_id, data_dir)?;
        }

        AuditCommand::Forward {
            target,
            transport,
            from,
            to,
        } => {
            let filter = ExportFilter {
                workspace: None,
                from: from
                    .as_deref()
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .map(|d| d.with_timezone(&chrono::Utc)),
                to: to
                    .as_deref()
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .map(|d| d.with_timezone(&chrono::Utc)),
            };
            forward_syslog(&log_path, &filter, &target, &transport)?;
        }

        AuditCommand::IsolationReport {
            from,
            to,
            drift_only,
        } => {
            isolation_report(&log_path, from.as_deref(), to.as_deref(), drift_only)?;
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

fn verify_sources(log_path: &Path, record_id: &str, data_dir: &Path) -> Result<()> {
    use maranode_common::receipt::SourceRef;
    use maranode_rag::VectorStore;

    if !log_path.exists() {
        anyhow::bail!("no audit log found at {}", log_path.display());
    }

    let content = std::fs::read_to_string(log_path)?;
    let receipt = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<AuditEntry>(l).ok())
        .find_map(|e| {
            if let AuditEvent::InferenceReceipt { receipt } = e.event {
                if receipt.request_id == record_id {
                    return Some(receipt);
                }
            }
            None
        });

    let receipt = receipt.ok_or_else(|| {
        anyhow::anyhow!("no receipt found for request id {}", record_id)
    })?;

    if receipt.sources.is_empty() {
        println!(
            "{} Receipt for {} has no RAG sources (not grounded).",
            "·".dimmed(),
            record_id.cyan(),
        );
        return Ok(());
    }

    let store = VectorStore::open(data_dir).map_err(|e| {
        anyhow::anyhow!("cannot open RAG store at {}: {}", data_dir.display(), e)
    })?;

    let mut all_ok = true;
    for src in &receipt.sources {
        match store.verify_chunk_hash(&src.chunk_id) {
            Ok((stored_hash, computed, matches)) => {
                if matches {
                    println!(
                        "  {} chunk {} ({}) — hash OK",
                        "✓".green().bold(),
                        src.chunk_id[..8].cyan(),
                        src.source,
                    );
                } else {
                    eprintln!(
                        "  {} chunk {} ({}) — TAMPERED",
                        "✗".red().bold(),
                        src.chunk_id[..8].cyan(),
                        src.source,
                    );
                    eprintln!("    receipt:  {}", src.chunk_hash.yellow());
                    eprintln!("    stored:   {}", stored_hash.yellow());
                    eprintln!("    computed: {}", computed.red());
                    all_ok = false;
                }
            }
            Err(e) => {
                eprintln!(
                    "  {} chunk {} — NOT FOUND ({})",
                    "?".yellow().bold(),
                    src.chunk_id[..8].cyan(),
                    e,
                );
                all_ok = false;
            }
        }
    }

    if all_ok {
        println!(
            "\n{} All {} source(s) verified — no tampering detected.",
            "✓".green().bold(),
            receipt.sources.len(),
        );
    } else {
        eprintln!(
            "\n{} Source verification {}. Some chunks may have been altered since inference.",
            "✗".red().bold(),
            "FAILED".red().bold(),
        );
        std::process::exit(1);
    }

    Ok(())
}

fn export_deletion_cert(log_path: &Path, slug: &str, output: Option<&Path>) -> Result<()> {
    if !log_path.exists() {
        anyhow::bail!("no audit log found at {}", log_path.display());
    }

    let content = std::fs::read_to_string(log_path)?;

    let entry = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<AuditEntry>(l).ok())
        .find(|e| {
            matches!(&e.event, AuditEvent::WorkspaceShredded { slug: s, .. } if s == slug)
        });

    let entry = entry.ok_or_else(|| {
        anyhow::anyhow!(
            "no deletion certificate found for workspace '{}'. \
             run `maranode workspace shred {} --yes` first.",
            slug, slug
        )
    })?;

    let AuditEvent::WorkspaceShredded { slug: _, actor, statement } = &entry.event else {
        unreachable!()
    };

    let text = format!(
        "DELETION CERTIFICATE\n\
         ====================\n\n\
         workspace : {}\n\
         timestamp : {}\n\
         audit seq : {}\n\
         actor     : {}\n\
         hmac      : {}\n\n\
         statement :\n  {}\n\n\
         This certificate is derived from an HMAC-chained audit log entry.\n\
         verify integrity with: maranode audit verify\n",
        slug,
        entry.ts.to_rfc3339(),
        entry.seq,
        actor,
        entry.hmac,
        statement,
    );

    let dest = output
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from(format!("deletion_cert_{}.txt", slug)));

    std::fs::write(&dest, &text)?;
    println!(
        "{} Deletion certificate exported → {}",
        "✓".green().bold(),
        dest.display(),
    );

    Ok(())
}

fn forward_syslog(
    log_path: &Path,
    filter: &ExportFilter,
    target: &str,
    transport: &str,
) -> Result<()> {
    use std::io::Write as IoWrite;
    use std::net::{TcpStream, UdpSocket};

    if !log_path.exists() {
        println!("{} No audit log found.", "·".dimmed());
        return Ok(());
    }

    let cef_body = export_cef(log_path, filter)?;
    let lines: Vec<&str> = cef_body.lines().filter(|l| !l.trim().is_empty()).collect();

    if lines.is_empty() {
        println!("{} No events to forward.", "·".dimmed());
        return Ok(());
    }

    let hostname = hostname_or_unknown();
    let mut sent = 0usize;

    match transport {
        "udp" => {
            let sock = UdpSocket::bind("0.0.0.0:0")?;
            for line in &lines {
                // RFC 5424 syslog header, facility=1 (user), severity=6 (informational)
                let msg = format!(
                    "<14>1 {} {} maranode - - - {}\n",
                    chrono::Utc::now().to_rfc3339(),
                    hostname,
                    line,
                );
                sock.send_to(msg.as_bytes(), target)?;
                sent += 1;
            }
        }
        _ => {
            let mut stream = TcpStream::connect(target)
                .map_err(|e| anyhow::anyhow!("cannot connect to {}: {}", target, e))?;
            for line in &lines {
                let msg = format!(
                    "<14>1 {} {} maranode - - - {}\n",
                    chrono::Utc::now().to_rfc3339(),
                    hostname,
                    line,
                );
                stream.write_all(msg.as_bytes())?;
                sent += 1;
            }
        }
    }

    println!(
        "{} Forwarded {} event(s) to {} via {}",
        "✓".green().bold(),
        sent.to_string().yellow(),
        target.cyan(),
        transport,
    );

    Ok(())
}

fn hostname_or_unknown() -> String {
    std::process::Command::new("hostname")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

fn isolation_report(
    log_path: &Path,
    from: Option<&str>,
    to: Option<&str>,
    drift_only: bool,
) -> Result<()> {
    use maranode_common::events::ProbeResult;

    if !log_path.exists() {
        println!("{} No audit log found.", "·".dimmed());
        return Ok(());
    }

    let from_dt = from
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.with_timezone(&chrono::Utc));
    let to_dt = to
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.with_timezone(&chrono::Utc));

    let content = std::fs::read_to_string(log_path)?;

    let probes: Vec<(AuditEntry, bool, Vec<ProbeResult>, String)> = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<AuditEntry>(l).ok())
        .filter_map(|e| {
            if let AuditEvent::IsolationProbe {
                isolated,
                ref probe_results,
                ref iptables_hash,
            } = e.event
            {
                Some((e.clone(), isolated, probe_results.clone(), iptables_hash.clone()))
            } else {
                None
            }
        })
        .filter(|(e, ..)| {
            if let Some(f) = from_dt {
                if e.ts < f {
                    return false;
                }
            }
            if let Some(t) = to_dt {
                if e.ts > t {
                    return false;
                }
            }
            true
        })
        .filter(|(_, isolated, ..)| if drift_only { !isolated } else { true })
        .collect();

    if probes.is_empty() {
        println!("{} No isolation probe events found in range.", "·".dimmed());
        return Ok(());
    }

    let total = probes.len();
    let drift_count = probes.iter().filter(|(_, isolated, ..)| !isolated).count();

    println!(
        "\n{} Isolation report — {} probe(s)",
        "⬡".cyan().bold(),
        total,
    );

    if drift_count == 0 {
        println!(
            "  {} Air-gap held across all {} probe(s) in range.",
            "✓".green().bold(),
            total,
        );
    } else {
        println!(
            "  {} {} drift event(s) detected out of {} probe(s).",
            "✗".red().bold(),
            drift_count.to_string().red().bold(),
            total,
        );
    }

    println!();

    for (entry, isolated, results, iptables_hash) in &probes {
        let status = if *isolated {
            "OK   ".green().bold()
        } else {
            "DRIFT".red().bold()
        };

        println!(
            "  [{}]  {}  seq={}",
            status,
            entry.ts.format("%Y-%m-%d %H:%M:%SZ").to_string().dimmed(),
            entry.seq.to_string().yellow(),
        );

        if !isolated {
            for r in results {
                if r.reachable {
                    println!(
                        "         {} {}:{}  reachable — egress confirmed",
                        "!".red(),
                        r.host.yellow(),
                        r.port,
                    );
                }
            }
        }

        if !iptables_hash.is_empty() {
            println!(
                "         iptables sha256={}",
                &iptables_hash[..16].dimmed(),
            );
        }
    }

    println!();

    Ok(())
}

async fn replay_inference(log_path: &std::path::Path, record_id: &str, host: &str) -> Result<()> {
    if !log_path.exists() {
        anyhow::bail!("no audit log found at {}", log_path.display());
    }

    let content = std::fs::read_to_string(log_path)?;
    let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();

    // find stored receipt
    let stored = lines
        .iter()
        .filter_map(|l| serde_json::from_str::<AuditEntry>(l).ok())
        .find_map(|entry| {
            if let AuditEvent::InferenceReceipt { receipt } = entry.event {
                if receipt.request_id == record_id {
                    return Some(receipt);
                }
            }
            None
        });

    let stored = stored.ok_or_else(|| {
        anyhow::anyhow!(
            "no inference receipt found for request id {}",
            record_id
        )
    })?;

    // find the matching InferenceStart to get the logged prompt
    let prompt_json = lines
        .iter()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find_map(|v| {
            if v["event"] == "inference_start" && v["request_id"] == record_id {
                v["prompt"].as_str().map(str::to_string)
            } else {
                None
            }
        });

    let prompt_json = prompt_json.ok_or_else(|| {
        anyhow::anyhow!(
            "replay requires log_prompts=true in daemon config; \
             no prompt was recorded for request id {}",
            record_id
        )
    })?;

    let messages: Vec<serde_json::Value> = serde_json::from_str(&prompt_json)
        .map_err(|e| anyhow::anyhow!("could not parse logged prompt: {e}"))?;

    println!(
        "{} Replaying {} (model={}, messages={}) …",
        "·".dimmed(),
        record_id.cyan(),
        stored.model_id.yellow(),
        messages.len(),
    );

    let body = serde_json::json!({
        "model": stored.model_id,
        "messages": messages,
        "max_tokens": stored.decode_params.max_tokens.unwrap_or(2048),
        "deterministic": true,
        "with_receipt": true,
    });

    let url = format!("{}/v1/chat/completions", host.trim_end_matches('/'));
    let resp = reqwest::Client::new()
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("could not reach daemon at {host}: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("daemon returned {status}: {text}");
    }

    let resp_val: serde_json::Value = resp.json().await?;
    let replay_receipt = resp_val
        .get("receipt")
        .ok_or_else(|| anyhow::anyhow!("daemon response contained no receipt field"))?;

    let replay_output_sha256 = replay_receipt["output_sha256"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("replay receipt missing output_sha256"))?;

    println!(
        "  stored  output_sha256: {}",
        stored.output_sha256.cyan()
    );
    println!(
        "  replay  output_sha256: {}",
        replay_output_sha256.cyan()
    );

    if stored.output_sha256 == replay_output_sha256 {
        println!("\n{} Output hash {}.", "✓".green().bold(), "MATCH".green().bold());
    } else {
        eprintln!(
            "\n{} Output hash {}. The run is not bit-exact.",
            "✗".red().bold(),
            "MISMATCH".red().bold(),
        );
        eprintln!(
            "  This usually means the model was run without --features deterministic-kernels \
             or on different hardware."
        );
        std::process::exit(1);
    }

    Ok(())
}
