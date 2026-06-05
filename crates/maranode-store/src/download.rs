//! stream download from huggingface

use std::path::Path;

use anyhow::{Context, Result};
use tokio::io::AsyncWriteExt;

pub async fn download_file(url: &str, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let part = dest.with_extension("part");
    if part.exists() {
        tokio::fs::remove_file(&part).await.ok();
    }

    let client = reqwest::Client::builder()
        .user_agent("maranode/0.1 (+https://maranode.com)")
        .build()?;

    let resp = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("GET {url}"))?
        .error_for_status()
        .with_context(|| format!("download failed for {url}"))?;

    let total = resp.content_length();
    let mut stream = resp.bytes_stream();
    let mut file = tokio::fs::File::create(&part).await?;
    let mut downloaded: u64 = 0;
    let mut last_pct = u8::MAX;

    use futures_util::StreamExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("reading download chunk")?;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;

        if let Some(total) = total {
            let pct = ((downloaded as f64 / total as f64) * 100.0) as u8;
            if pct / 5 != last_pct / 5 {
                last_pct = pct;
                eprint!(
                    "\r  {pct:3}% ({:.1} / {:.1} MB)",
                    downloaded as f64 / 1_048_576.0,
                    total as f64 / 1_048_576.0
                );
            }
        } else {
            eprint!("\r  {:.1} MB downloaded", downloaded as f64 / 1_048_576.0);
        }
    }

    file.flush().await?;
    drop(file);
    eprintln!();

    tokio::fs::rename(&part, dest)
        .await
        .with_context(|| format!("finalising {}", dest.display()))?;

    Ok(())
}
