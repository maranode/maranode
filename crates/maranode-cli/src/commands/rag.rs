use std::path::PathBuf;

use anyhow::Result;
use clap::Subcommand;
use colored::Colorize;

#[derive(Subcommand)]
pub enum RagCommand {
    /// add document to RAG collection.
    /// supported types: .txt, .md, .csv, .log, .rst, .pdf
    Add {
        path: PathBuf,
        #[arg(long)]
        collection: Option<String>,
        #[arg(long)]
        source: Option<String>,
    },
    List,
    Search {
        query: String,
        #[arg(long)]
        collection: Option<String>,
        #[arg(long)]
        top_k: Option<usize>,
    },
}

pub async fn run(cmd: RagCommand, host: &str) -> Result<()> {
    let host = host.trim_end_matches('/');
    let client = reqwest::Client::new();

    match cmd {
        RagCommand::Add {
            path,
            collection,
            source,
        } => {
            let filename = path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string());

            let source_label = source.unwrap_or_else(|| filename.clone());

            println!("{} Uploading '{}'…", "·".dimmed(), path.display());

            let bytes = std::fs::read(&path)
                .map_err(|e| anyhow::anyhow!("reading {}: {}", path.display(), e))?;

            let file_part = reqwest::multipart::Part::bytes(bytes)
                .file_name(filename)
                .mime_str("application/octet-stream")?;

            let mut form = reqwest::multipart::Form::new()
                .part("file", file_part)
                .text("source", source_label.clone());

            if let Some(c) = collection {
                form = form.text("collection", c);
            }

            let url = format!("{host}/v1/rag/documents/upload");
            let resp = client
                .post(&url)
                .multipart(form)
                .send()
                .await
                .map_err(|e| {
                    anyhow::anyhow!(
                        "{} Could not reach daemon at {}: {}",
                        "✗".red().bold(),
                        host.cyan(),
                        e
                    )
                })?;

            let json = parse(resp).await?;
            println!(
                "{} Ingested '{}' into '{}' ({} chunks)",
                "✓".green().bold(),
                source_label.bold(),
                json["collection"].as_str().unwrap_or("?").cyan(),
                json["chunks"].as_u64().unwrap_or(0),
            );
        }

        RagCommand::List => {
            let url = format!("{host}/v1/rag/collections");
            let resp = client.get(&url).send().await.map_err(|e| {
                anyhow::anyhow!(
                    "{} Could not reach daemon at {}: {}",
                    "✗".red().bold(),
                    host.cyan(),
                    e
                )
            })?;
            let json = parse(resp).await?;
            let collections = json.as_array().cloned().unwrap_or_default();
            if collections.is_empty() {
                println!(
                    "{} No collections. Add a document with: {}",
                    "·".dimmed(),
                    "maranode rag add <file>".cyan(),
                );
            } else {
                println!(
                    "{}  {}  {}  {}  {}",
                    pad("NAME", 20).bold().underline(),
                    pad("EMBEDDING MODEL", 28).bold().underline(),
                    pad_right("DIM", 5).bold().underline(),
                    pad_right("DOCS", 6).bold().underline(),
                    pad_right("CHUNKS", 7).bold().underline(),
                );
                for c in collections {
                    let model_col = pad(c["embedding_model"].as_str().unwrap_or("?"), 28);
                    let docs_col = pad_right(&c["documents"].as_u64().unwrap_or(0).to_string(), 6);
                    let chunks_col = pad_right(&c["chunks"].as_u64().unwrap_or(0).to_string(), 7);
                    println!(
                        "{}  {}  {}  {}  {}",
                        pad(c["name"].as_str().unwrap_or("?"), 20),
                        model_col.dimmed(),
                        pad_right(&c["dim"].as_u64().unwrap_or(0).to_string(), 5),
                        docs_col.cyan(),
                        chunks_col.dimmed(),
                    );
                }
            }
        }

        RagCommand::Search {
            query,
            collection,
            top_k,
        } => {
            let mut body = serde_json::json!({ "query": query });
            if let Some(c) = &collection {
                body["collection"] = serde_json::json!(c);
            }
            if let Some(k) = top_k {
                body["top_k"] = serde_json::json!(k);
            }

            let url = format!("{host}/v1/rag/search");
            let resp = client.post(&url).json(&body).send().await.map_err(|e| {
                anyhow::anyhow!(
                    "{} Could not reach daemon at {}: {}",
                    "✗".red().bold(),
                    host.cyan(),
                    e
                )
            })?;
            let json = parse(resp).await?;
            let hits = json.as_array().cloned().unwrap_or_default();
            if hits.is_empty() {
                println!("{} No matching chunks found.", "·".dimmed());
            } else {
                for (i, h) in hits.iter().enumerate() {
                    println!(
                        "{}  {}  {}",
                        format!("[{}]", i + 1).cyan().bold(),
                        format!("{:.3}", h["score"].as_f64().unwrap_or(0.0)).yellow(),
                        h["source"].as_str().unwrap_or("?").bold(),
                    );
                    println!(
                        "    {}",
                        truncate(h["text"].as_str().unwrap_or(""), 200).dimmed()
                    );
                }
            }
        }
    }

    Ok(())
}

async fn parse(resp: reqwest::Response) -> Result<serde_json::Value> {
    if resp.status() == reqwest::StatusCode::NOT_IMPLEMENTED {
        anyhow::bail!("RAG is not enabled on the daemon. Restart it with --rag.");
    }
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if let Ok(j) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(msg) = j["error"]["message"].as_str() {
                anyhow::bail!("{} {}", "✗".red().bold(), msg);
            }
        }
        anyhow::bail!("{} Daemon returned {}: {}", "✗".red().bold(), status, text);
    }
    Ok(resp.json().await?)
}

fn truncate(s: &str, max_chars: usize) -> String {
    let mut out: String = s.chars().take(max_chars).collect();
    if s.chars().count() > max_chars {
        out.push('…');
    }
    out.replace('\n', " ")
}

fn pad(s: &str, width: usize) -> String {
    format!("{:<width$}", s, width = width)
}

fn pad_right(s: &str, width: usize) -> String {
    format!("{:>width$}", s, width = width)
}
