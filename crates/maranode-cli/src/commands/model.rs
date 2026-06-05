use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use clap::Subcommand;
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};

use maranode_common::models::{ModelId, ModelType};
use maranode_store::ModelStore;

use crate::errors::did_you_mean;

#[derive(Subcommand)]
pub enum ModelCommand {
    /// download GGUF model from Hugging Face or direct URL
    Pull {
        /// model source: owner/repo/file.gguf (Hugging Face) or full https URL
        source: String,
        /// local model name in store
        #[arg(long)]
        name: String,
        /// model tag, e.g. 3b or latest
        #[arg(long, default_value = "latest")]
        tag: String,
        /// quantization label, e.g. Q4_K_M
        #[arg(long)]
        quant: Option<String>,
        /// model type: llm (default) or embedding
        #[arg(long = "type", default_value = "llm", value_name = "TYPE")]
        model_type: String,
    },
    /// import GGUF model from local file path
    Import {
        path: PathBuf,
        #[arg(long)]
        name: String,
        #[arg(long)]
        tag: String,
        #[arg(long)]
        quant: Option<String>,
        #[arg(long = "type", default_value = "llm", value_name = "TYPE")]
        model_type: String,
    },
    List,
    Remove {
        model: String,
    },
    /// inspect and recommend GGUF quantization settings
    Quant {
        #[command(subcommand)]
        action: crate::commands::quant::QuantCommand,
    },
}

pub async fn run(cmd: ModelCommand, data_dir: &Path) -> Result<()> {
    if let ModelCommand::Quant { action } = cmd {
        return crate::commands::quant::run(action, data_dir).await;
    }

    let store = ModelStore::open(data_dir)?;

    match cmd {
        ModelCommand::Quant { .. } => unreachable!(),
        ModelCommand::Pull {
            source,
            name,
            tag,
            quant,
            model_type: type_str,
        } => {
            let model_type = parse_model_type(&type_str);
            let model_id = ModelId::new(&name, &tag);

            // build download URL from source string
            let url = if source.starts_with("https://") || source.starts_with("http://") {
                source.clone()
            } else {
                // hugging Face path: owner/repo/filename
                // two-part owner/repo is not supported here
                let parts: Vec<&str> = source.splitn(3, '/').collect();
                match parts.as_slice() {
                    [owner, repo, file] => {
                        format!(
                            "https://huggingface.co/{}/{}/resolve/main/{}",
                            owner, repo, file
                        )
                    }
                    _ => anyhow::bail!(
                        "invalid source '{}': expected owner/repo/filename.gguf or a full URL",
                        source
                    ),
                }
            };

            println!(
                "{} Pulling {} → {}:{}",
                "·".dimmed(),
                url.cyan(),
                name.bold(),
                tag,
            );

            // show download progress bar
            let pb = ProgressBar::new(0);
            pb.set_style(
                ProgressStyle::with_template(
                    "{spinner:.cyan} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})",
                )
                .unwrap()
                .progress_chars("█▉▊▋▌▍▎▏  "),
            );
            pb.enable_steady_tick(Duration::from_millis(80));

            let pb_arc = Arc::new(Mutex::new(pb));
            let pb_cb = Arc::clone(&pb_arc);

            let manifest = store
                .pull_from_url(
                    &url,
                    model_id,
                    quant,
                    model_type,
                    move |downloaded, total| {
                        let pb = pb_cb.lock().unwrap();
                        if let Some(t) = total {
                            pb.set_length(t);
                        }
                        pb.set_position(downloaded);
                    },
                )
                .await
                .map_err(|e| {
                    pb_arc.lock().unwrap().finish_and_clear();
                    e
                })?;

            pb_arc.lock().unwrap().finish_and_clear();

            println!(
                "{} {}:{}: {} · sha256:{}…",
                "✓".green().bold(),
                name.bold(),
                tag,
                human_size(manifest.size_bytes).cyan(),
                manifest
                    .sha256
                    .get(..16)
                    .unwrap_or(&manifest.sha256)
                    .dimmed(),
            );
        }

        ModelCommand::Import {
            path,
            name,
            tag,
            quant,
            model_type: type_str,
        } => {
            let model_type = parse_model_type(&type_str);

            let pb = ProgressBar::new_spinner();
            pb.set_style(ProgressStyle::with_template("{spinner:.cyan} {msg}").unwrap());
            pb.set_message(format!("Importing {}:{} …", name, tag));
            pb.enable_steady_tick(Duration::from_millis(80));

            let manifest = store
                .import_from_file(&path, ModelId::new(&name, &tag), quant, model_type)
                .await
                .map_err(|e| {
                    pb.finish_and_clear();
                    e
                })?;

            pb.finish_and_clear();

            println!(
                "{} {}:{}: {} · sha256:{}…",
                "✓".green().bold(),
                name.bold(),
                tag,
                human_size(manifest.size_bytes).cyan(),
                manifest
                    .sha256
                    .get(..16)
                    .unwrap_or(&manifest.sha256)
                    .dimmed(),
            );
        }

        ModelCommand::List => {
            let models = store.list().await?;
            if models.is_empty() {
                println!(
                    "{} No models in store. Import one with: {}",
                    "·".dimmed(),
                    "maranode model import".cyan(),
                );
            } else {
                println!(
                    "{}  {}  {}  {}  {}",
                    "NAME".bold().underline(),
                    pad("TYPE", 9).bold().underline(),
                    pad("IMPORTED", 17).bold().underline(),
                    pad_right("SIZE", 10).bold().underline(),
                    "SHA256".bold().underline(),
                );
                for m in models {
                    let type_col = match m.model_type {
                        ModelType::Llm => pad("llm", 9).normal(),
                        ModelType::Embedding => pad("embedding", 9).cyan(),
                    };
                    println!(
                        "{}  {}  {}  {}  {}",
                        pad(&m.model_id.to_string(), 30),
                        type_col,
                        pad(&m.imported_at.format("%Y-%m-%d %H:%M").to_string(), 17),
                        pad_right(&human_size(m.size_bytes), 10),
                        format!("{}…", &m.sha256[..16]).dimmed(),
                    );
                }
            }
        }

        ModelCommand::Remove { model } => {
            let model_id = ModelId::parse(&model).ok_or_else(|| {
                anyhow::anyhow!(
                    "invalid model id '{}': expected format {} (e.g. {})",
                    model.yellow(),
                    "name:tag".cyan(),
                    "llama3.2:3b".cyan(),
                )
            })?;
            let removed = store.remove(&model_id).await?;
            if removed {
                println!("{} Removed {}", "✓".green().bold(), model.bold());
            } else {
                // suggest similar model name from store if typo
                let all = store.list().await?;
                let names: Vec<String> = all.iter().map(|m| m.model_id.to_string()).collect();
                let refs: Vec<&str> = names.iter().map(String::as_str).collect();
                let hint = did_you_mean(&model, &refs)
                    .map(|s| format!("  Did you mean {}?", s.cyan()))
                    .unwrap_or_else(|| {
                        format!(
                            "  Run {} to see all stored models.",
                            "maranode model list".cyan()
                        )
                    });
                eprintln!(
                    "{} Model '{}' not found.{}",
                    "!".yellow().bold(),
                    model.bold(),
                    hint
                );
                std::process::exit(1);
            }
        }
    }

    Ok(())
}

fn parse_model_type(s: &str) -> ModelType {
    match s.to_lowercase().as_str() {
        "embedding" | "embed" => ModelType::Embedding,
        "llm" | "chat" | "" => ModelType::Llm,
        other => {
            let suggestion = did_you_mean(other, &["llm", "embedding"])
                .map(|m| format!(" Did you mean '{}'?", m))
                .unwrap_or_default();
            eprintln!(
                "{} Unknown --type '{}'. Using 'llm'.{}",
                "⚠".yellow().bold(),
                other,
                suggestion,
            );
            ModelType::Llm
        }
    }
}

fn human_size(bytes: u64) -> String {
    const GB: u64 = 1_073_741_824;
    const MB: u64 = 1_048_576;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.0} MB", bytes as f64 / MB as f64)
    } else {
        format!("{} B", bytes)
    }
}

fn pad(s: &str, width: usize) -> String {
    format!("{:<width$}", s, width = width)
}

fn pad_right(s: &str, width: usize) -> String {
    format!("{:>width$}", s, width = width)
}
