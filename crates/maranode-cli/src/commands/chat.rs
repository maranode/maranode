use anyhow::Result;
use colored::Colorize;
use futures_util::StreamExt;

pub async fn run(
    prompt: &str,
    model: &str,
    host: &str,
    use_rag: bool,
    collection: Option<String>,
) -> Result<()> {
    let url = format!("{}/v1/chat/completions", host.trim_end_matches('/'));

    let mut body = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
        "stream": true,
    });

    if use_rag {
        let mut rag = serde_json::Map::new();
        if let Some(c) = collection {
            rag.insert("collection".into(), serde_json::json!(c));
        }
        body["rag"] = serde_json::Value::Object(rag);
    }

    let client = reqwest::Client::new();
    let resp = client.post(&url).json(&body).send().await.map_err(|e| {
        anyhow::anyhow!(
            "{} Could not reach daemon at {}: is it running?\n  {}",
            "✗".red().bold(),
            host.cyan(),
            e
        )
    })?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        // try to read error message from JSON body
        let msg = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| v["error"].as_str().map(str::to_string))
            .unwrap_or(text);
        anyhow::bail!("{} {}: {}", "✗".red().bold(), status, msg);
    }

    let mut stream = resp.bytes_stream();
    let mut buf = String::new();
    let mut sources: Option<serde_json::Value> = None;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        buf.push_str(&String::from_utf8_lossy(&chunk));

        // process full SSE lines from buffer
        while let Some(pos) = buf.find('\n') {
            let line = buf[..pos].to_string();
            buf = buf[pos + 1..].to_string();

            let line = line.trim_end_matches('\r');
            if let Some(data) = line.strip_prefix("data: ") {
                if data == "[DONE]" {
                    break;
                }
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(data) {
                    if let Some(token) = val["choices"][0]["delta"]["content"].as_str() {
                        print!("{}", token);
                        use std::io::Write;
                        let _ = std::io::stdout().flush();
                    }
                    // save RAG sources from chunk when present
                    if val["sources"].is_array() {
                        sources = Some(val["sources"].clone());
                    }
                }
            }
        }
    }

    println!();

    if let Some(srcs) = sources {
        if let Some(arr) = srcs.as_array() {
            if !arr.is_empty() {
                println!("\n{}", "Sources:".bold().underline());
                for s in arr {
                    println!(
                        "  {} {} {}",
                        format!("[{}]", s["index"].as_u64().unwrap_or(0)).cyan(),
                        s["source"].as_str().unwrap_or("?"),
                        format!("(score {:.3})", s["score"].as_f64().unwrap_or(0.0)).dimmed(),
                    );
                }
            }
        }
    }

    Ok(())
}
