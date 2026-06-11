use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::Subcommand;
use colored::Colorize;
use chrono::Utc;

use maranode_common::baseline::{output_sha256, Baseline, TestVector};

#[derive(Subcommand)]
pub enum BaselineCommand {
    /// create a new unsigned baseline file from a list of prompt:expected_sha256 pairs
    Create {
        /// model sha256 this baseline covers
        #[arg(long)]
        model_sha256: String,

        /// human-readable model id, e.g. llama3:8b
        #[arg(long)]
        model_id: String,

        /// number of vector mismatches tolerated before drift is declared (default: 0)
        #[arg(long, default_value_t = 0)]
        max_mismatches: usize,

        /// output path for the unsigned baseline file
        #[arg(long, short)]
        output: PathBuf,

        /// pairs of "prompt=expected_text" — expected_sha256 is computed automatically
        /// repeat --vector for each test case
        #[arg(long = "vector", value_name = "PROMPT=EXPECTED_TEXT")]
        vectors: Vec<String>,
    },

    /// sign a baseline file with the local baseline signing key
    Sign {
        /// path to the unsigned (or already-signed) baseline file
        baseline: PathBuf,

        /// output path. default: overwrites input file
        #[arg(long, short)]
        output: Option<PathBuf>,
    },

    /// verify the signature and print baseline summary
    Verify {
        /// path to the baseline file
        baseline: PathBuf,
    },

    /// list baseline files in the baselines directory
    List,

    /// fetch a baseline from the public registry by model sha256
    Fetch {
        /// first 12+ characters of the model sha256
        model_sha256: String,

        /// output directory. default: <data_dir>/baselines/
        #[arg(long, short)]
        output_dir: Option<PathBuf>,
    },

    /// run a baseline check against a model file without loading it into the daemon
    /// (requires a running daemon)
    Check {
        /// model name:tag
        model: String,

        /// baseline file to use. if omitted, looks in <data_dir>/baselines/<model_sha256>.mrn-baseline
        #[arg(long)]
        baseline: Option<PathBuf>,

        /// daemon address
        #[arg(long, default_value = "http://127.0.0.1:11984")]
        host: String,
    },
}

pub async fn run(cmd: BaselineCommand, data_dir: &Path, host: &str) -> Result<()> {
    match cmd {
        BaselineCommand::Create {
            model_sha256,
            model_id,
            max_mismatches,
            output,
            vectors,
        } => {
            let mut parsed: Vec<TestVector> = Vec::new();
            for v in &vectors {
                if let Some((prompt, expected_text)) = v.split_once('=') {
                    parsed.push(TestVector {
                        prompt: prompt.to_string(),
                        temperature: 0.0,
                        seed: 42,
                        max_tokens: 512,
                        expected_sha256: output_sha256(expected_text),
                    });
                } else {
                    anyhow::bail!(
                        "vector '{}' must be in form PROMPT=EXPECTED_TEXT",
                        v
                    );
                }
            }

            if parsed.is_empty() {
                anyhow::bail!("at least one --vector is required");
            }

            let b = Baseline {
                schema_version: 1,
                model_sha256,
                model_id,
                created_at: Utc::now(),
                vectors: parsed,
                max_mismatches,
                signer_pubkey: String::new(),
                signature: String::new(),
            };

            b.save(&output)?;
            println!(
                "{} Unsigned baseline written ({} vectors) → {}",
                "✓".green().bold(),
                b.vectors.len(),
                output.display(),
            );
            println!(
                "  Run {} to sign it.",
                format!("maranode baseline sign {}", output.display()).cyan()
            );
        }

        BaselineCommand::Sign { baseline, output } => {
            let b = Baseline::load(&baseline)?;
            let key = Baseline::load_or_create_signing_key(data_dir)?;
            let signed = b.sign(&key)?;
            let dest = output.unwrap_or(baseline);
            signed.save(&dest)?;
            println!(
                "{} Baseline signed → {}",
                "✓".green().bold(),
                dest.display()
            );
            println!(
                "  pubkey: {}",
                &signed.signer_pubkey.get(..20).unwrap_or(&signed.signer_pubkey).dimmed()
            );
        }

        BaselineCommand::Verify { baseline } => {
            let b = Baseline::load(&baseline)?;
            match b.verify() {
                Ok(()) => {
                    println!(
                        "{} Signature valid — {} vectors, model_sha256={}…",
                        "✓".green().bold(),
                        b.vectors.len(),
                        &b.model_sha256.get(..16).unwrap_or(&b.model_sha256).dimmed(),
                    );
                    println!("  model_id      : {}", b.model_id.cyan());
                    println!("  max_mismatches: {}", b.max_mismatches);
                    println!("  created_at    : {}", b.created_at.to_rfc3339().dimmed());
                    println!(
                        "  signer_pubkey : {}",
                        b.signer_pubkey.get(..20).unwrap_or(&b.signer_pubkey).dimmed()
                    );
                }
                Err(e) => {
                    eprintln!("{} Signature INVALID: {}", "✗".red().bold(), e);
                    std::process::exit(1);
                }
            }
        }

        BaselineCommand::List => {
            let baselines_dir = data_dir.join("baselines");
            if !baselines_dir.exists() {
                println!("{} No baselines directory at {}", "·".dimmed(), baselines_dir.display());
                return Ok(());
            }
            let mut found = false;
            for entry in std::fs::read_dir(&baselines_dir)? {
                let entry = entry?;
                let name = entry.file_name().to_string_lossy().to_string();
                if !name.ends_with(".mrn-baseline") {
                    continue;
                }
                found = true;
                match Baseline::load(&entry.path()) {
                    Ok(b) => {
                        let sig_ok = b.verify().is_ok();
                        println!(
                            "  {} {}  {}  {} vectors{}",
                            if sig_ok { "✓".green().bold() } else { "!".yellow().bold() },
                            b.model_id.cyan(),
                            b.model_sha256.get(..16).unwrap_or(&b.model_sha256).dimmed(),
                            b.vectors.len(),
                            if sig_ok { "" } else { " (signature invalid)" },
                        );
                    }
                    Err(e) => println!("  {} {} — parse error: {}", "?".yellow(), name, e),
                }
            }
            if !found {
                println!("{} No baseline files found.", "·".dimmed());
            }
        }

        BaselineCommand::Fetch { model_sha256, output_dir } => {
            fetch_baseline(&model_sha256, output_dir.as_deref(), data_dir).await?;
        }

        BaselineCommand::Check { model, baseline, host } => {
            check_via_daemon(&model, baseline.as_deref(), &host).await?;
        }
    }

    Ok(())
}

async fn fetch_baseline(
    model_sha256: &str,
    output_dir: Option<&Path>,
    data_dir: &Path,
) -> Result<()> {
    let registry_url = "https://baselines.maranode.ai/registry.json";

    println!("{} Fetching registry from {}", "·".dimmed(), registry_url);

    let resp = reqwest::Client::new()
        .get(registry_url)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("cannot reach registry: {}", e))?;

    if !resp.status().is_success() {
        anyhow::bail!("registry returned {}", resp.status());
    }

    let registry: serde_json::Value = resp.json().await?;
    let entries = registry["baselines"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("registry has no 'baselines' array"))?;

    let entry = entries
        .iter()
        .find(|e| {
            e["model_sha256"]
                .as_str()
                .map(|s| s.starts_with(model_sha256))
                .unwrap_or(false)
        })
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no baseline found for sha256 prefix '{}' in registry",
                model_sha256
            )
        })?;

    let url = entry["url"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("registry entry missing url"))?;
    let full_sha256 = entry["model_sha256"]
        .as_str()
        .unwrap_or(model_sha256);

    println!("{} Downloading {} …", "·".dimmed(), url.dimmed());

    let bytes = reqwest::get(url)
        .await
        .map_err(|e| anyhow::anyhow!("download failed: {}", e))?
        .bytes()
        .await?;

    let baseline: Baseline = serde_json::from_slice(&bytes)?;
    baseline.verify()?;

    let dir = output_dir
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| data_dir.join("baselines"));
    std::fs::create_dir_all(&dir)?;

    let dest = dir.join(format!("{}.mrn-baseline", full_sha256));
    baseline.save(&dest)?;

    println!(
        "{} Baseline verified and saved ({} vectors) → {}",
        "✓".green().bold(),
        baseline.vectors.len(),
        dest.display(),
    );

    Ok(())
}

async fn check_via_daemon(model: &str, baseline_path: Option<&Path>, host: &str) -> Result<()> {
    let url = format!("{}/v1/baseline/check", host.trim_end_matches('/'));

    let mut body = serde_json::json!({ "model": model });
    if let Some(p) = baseline_path {
        let b = Baseline::load(p)?;
        body["baseline"] = serde_json::to_value(&b)?;
    }

    let resp = reqwest::Client::new()
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("cannot reach daemon at {}: {}", host, e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("daemon returned {}: {}", status, text);
    }

    let result: serde_json::Value = resp.json().await?;
    let passed = result["vectors_passed"].as_u64().unwrap_or(0);
    let failed = result["vectors_failed"].as_u64().unwrap_or(0);
    let total = passed + failed;
    let ok = result["ok"].as_bool().unwrap_or(false);

    if ok {
        println!(
            "{} Baseline check passed: {}/{} vectors OK",
            "✓".green().bold(), passed, total
        );
    } else {
        eprintln!(
            "{} Baseline check FAILED: {}/{} vectors failed",
            "✗".red().bold(), failed, total
        );
        std::process::exit(1);
    }

    Ok(())
}
