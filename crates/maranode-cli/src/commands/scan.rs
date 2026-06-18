use std::path::{Path, PathBuf};

use anyhow::Result;
use colored::Colorize;

use maranode_rag::chunk::lang_for;
use maranode_rag::codescan::{scan, severity_counts, Severity};

pub fn run(path: &Path, min_severity: &str) -> Result<()> {
    let min = sev_rank(parse_sev(min_severity));

    let mut files: Vec<PathBuf> = Vec::new();
    collect(path, &mut files)?;
    if files.is_empty() {
        println!("{} no source files found at {}", "·".dimmed(), path.display());
        return Ok(());
    }

    let mut flagged_files = 0usize;
    let mut totals = (0usize, 0usize, 0usize);

    for file in &files {
        let text = match std::fs::read_to_string(file) {
            Ok(t) => t,
            Err(_) => continue,
        };
        let findings: Vec<_> = scan(&text)
            .into_iter()
            .filter(|f| sev_rank(f.severity) >= min)
            .collect();
        if findings.is_empty() {
            continue;
        }
        flagged_files += 1;
        println!("\n{}", file.display().to_string().bold());
        for f in &findings {
            let sev = match f.severity {
                Severity::High => f.severity.as_str().red().bold(),
                Severity::Medium => f.severity.as_str().yellow().bold(),
                Severity::Low => f.severity.as_str().normal(),
            };
            println!("  {}  [{}] {}", format!("L{}", f.line).dimmed(), sev, f.title);
            println!("      {}", f.snippet.dimmed());
        }
        let c = severity_counts(&findings);
        totals.0 += c.0;
        totals.1 += c.1;
        totals.2 += c.2;
    }

    if flagged_files == 0 {
        println!("{} no findings at or above '{}'", "✓".green().bold(), min_severity);
    } else {
        println!(
            "\n{} {} file(s): {} high, {} medium, {} low",
            "Summary".bold(),
            flagged_files,
            totals.0.to_string().red(),
            totals.1.to_string().yellow(),
            totals.2,
        );
    }
    Ok(())
}

fn parse_sev(s: &str) -> Severity {
    match s.to_lowercase().as_str() {
        "high" => Severity::High,
        "medium" | "med" => Severity::Medium,
        _ => Severity::Low,
    }
}

fn sev_rank(s: Severity) -> u8 {
    match s {
        Severity::High => 3,
        Severity::Medium => 2,
        Severity::Low => 1,
    }
}

fn collect(path: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    if path.is_file() {
        out.push(path.to_path_buf());
        return Ok(());
    }
    if path.is_dir() {
        for entry in std::fs::read_dir(path)? {
            let p = entry?.path();
            if p.is_dir() {
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if matches!(
                    name,
                    ".git" | "target" | "node_modules" | ".venv" | "dist" | "build"
                ) {
                    continue;
                }
                collect(&p, out)?;
            } else if lang_for(&p.to_string_lossy()).is_some() {
                out.push(p);
            }
        }
    }
    Ok(())
}
