use std::path::{Path, PathBuf};

use anyhow::Result;
use colored::Colorize;

use maranode_rag::chunk::lang_for;
use maranode_rag::symbols::{outline, search};

pub fn run(path: &Path, query: Option<&str>) -> Result<()> {
    let mut files: Vec<PathBuf> = Vec::new();
    collect(path, &mut files)?;
    if files.is_empty() {
        println!("{} no source files found at {}", "·".dimmed(), path.display());
        return Ok(());
    }

    let mut total = 0usize;
    for file in &files {
        let lang = match lang_for(&file.to_string_lossy()) {
            Some(l) => l,
            None => continue,
        };
        let text = match std::fs::read_to_string(file) {
            Ok(t) => t,
            Err(_) => continue,
        };
        let syms = match query {
            Some(q) => search(&text, lang, q),
            None => outline(&text, lang),
        };
        if syms.is_empty() {
            continue;
        }
        println!("\n{}", file.display().to_string().bold());
        for s in &syms {
            println!(
                "  {}  [{}] {}",
                format!("L{}", s.line).dimmed(),
                s.kind.as_str().cyan(),
                s.name
            );
            total += 1;
        }
    }

    println!("\n{} {} symbol(s)", "Total:".bold(), total);
    Ok(())
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
