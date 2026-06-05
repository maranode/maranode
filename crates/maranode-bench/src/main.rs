use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use clap::Parser;
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use serde::{Deserialize, Serialize};
use tracing::info;
use uuid::Uuid;

use maranode_common::models::{ChatMessage, ChatRole, ModelId};
use maranode_inference::engine::InferenceEngine;
use maranode_inference::types::{InferenceRequest, InferenceResponse};
use maranode_inference::{DevicePreference, LlamaCppEngine};

#[derive(Parser, Debug)]
#[command(
    name = "maranode-bench",
    version,
    about = "Measure inference throughput, TTFT, and latency percentiles"
)]
struct Args {
    #[arg(long, short = 'm')]
    model: PathBuf,

    /// inference device: auto (default), cpu, gpu, or npu
    #[arg(long, short = 'd', default_value = "auto", env = "MARANODE_DEVICE")]
    device: String,

    /// number of warmup runs; results are not counted
    #[arg(long, default_value = "3")]
    warmup: u32,

    /// number of timed measurement runs
    #[arg(long, short = 'n', default_value = "10")]
    runs: u32,

    /// maximum tokens to generate in each run
    #[arg(long, default_value = "128")]
    max_tokens: u32,

    #[arg(long)]
    prompt: Option<String>,

    #[arg(long, short = 'o', default_value = "table")]
    output: OutputFormat,

    #[arg(long)]
    save: Option<PathBuf>,

    #[arg(long)]
    compare: Option<PathBuf>,

    #[arg(long)]
    quiet: bool,
}

#[derive(Debug, Clone, clap::ValueEnum)]
enum OutputFormat {
    Table,
    Json,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RunResult {
    run: u32,
    tokens_in: u32,
    tokens_out: u32,
    total_ms: u64,
    ttft_ms: u64,
    prompt_tps: f64,
    gen_tps: f64,
    device: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Summary {
    model: String,
    device: String,
    warmup_runs: u32,
    measurement_runs: u32,
    max_tokens: u32,
    mean_gen_tps: f64,
    mean_prompt_tps: f64,
    mean_ttft_ms: f64,
    p50_ms: f64,
    p95_ms: f64,
    p99_ms: f64,
    runs: Vec<RunResult>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let log_level = if args.quiet { "warn" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_level)),
        )
        .with_target(false)
        .init();

    if !args.model.exists() {
        bail!("Model file not found: {}", args.model.display());
    }
    if args.runs == 0 {
        bail!("--runs must be at least 1");
    }

    let warmup = args.warmup.max(1);
    let pref = parse_device(&args.device)?;

    info!("Initialising engine (device={})", args.device);
    let engine = LlamaCppEngine::new(pref).context("initialising LlamaCppEngine")?;

    let device_label = engine.device().to_string();

    info!("Loading model: {}", args.model.display());
    engine
        .load_model("bench", &args.model)
        .await
        .context("loading model")?;

    let prompt = args.prompt.unwrap_or_else(default_prompt);

    {
        let pb = progress_bar(warmup, "Warming up");
        for i in 0..warmup {
            let req = build_request(&args.model, &prompt, args.max_tokens);
            engine
                .generate(req)
                .await
                .with_context(|| format!("warmup run {}", i + 1))?;
            pb.inc(1);
        }
        pb.finish_and_clear();
    }

    let mut results: Vec<RunResult> = Vec::with_capacity(args.runs as usize);
    {
        let pb = progress_bar(args.runs, "Benchmarking");
        for run_n in 1..=args.runs {
            let req = build_request(&args.model, &prompt, args.max_tokens);
            let t0 = Instant::now();
            let resp: InferenceResponse = engine
                .generate(req)
                .await
                .with_context(|| format!("run {}", run_n))?;
            let total_ms = t0.elapsed().as_millis() as u64;

            let r = compute_run(run_n, &resp, total_ms, &device_label);
            pb.set_message(format!("{:.1} tok/s", r.gen_tps));
            results.push(r);
            pb.inc(1);
        }
        pb.finish_and_clear();
    }

    let summary = make_summary(
        &args.model,
        &device_label,
        warmup,
        args.runs,
        args.max_tokens,
        &results,
    );

    match args.output {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&summary)?),
        OutputFormat::Table => print_table(&summary),
    }

    if let Some(path) = &args.save {
        std::fs::write(path, serde_json::to_string_pretty(&summary)?)
            .with_context(|| format!("saving results to {}", path.display()))?;
        println!("{} Results saved to {}", "✓".green().bold(), path.display());
    }

    if let Some(path) = &args.compare {
        let baseline_json = std::fs::read_to_string(path)
            .with_context(|| format!("reading baseline from {}", path.display()))?;
        let baseline: Summary =
            serde_json::from_str(&baseline_json).context("parsing baseline JSON")?;
        print_comparison(&baseline, &summary);
    }

    Ok(())
}

fn parse_device(s: &str) -> Result<DevicePreference> {
    match s.to_lowercase().as_str() {
        "auto" | "" => Ok(DevicePreference::Auto),
        "cpu" => Ok(DevicePreference::Cpu),
        "gpu" => Ok(DevicePreference::Gpu),
        "npu" => Ok(DevicePreference::Npu),
        other => bail!("unknown device '{}': expected: auto, cpu, gpu, npu", other),
    }
}

fn default_prompt() -> String {
    "Explain the concept of neural network quantisation to a software engineer \
     who is familiar with floating-point arithmetic but new to machine learning. \
     Focus on Q4_K_M and Q8_0 formats and their trade-offs."
        .to_owned()
}

fn build_request(model_path: &PathBuf, prompt: &str, max_tokens: u32) -> InferenceRequest {
    let stem = model_path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    InferenceRequest {
        request_id: Uuid::new_v4().to_string(),
        model: ModelId::new(&stem, "bench"),
        model_path: model_path.clone(),
        messages: vec![ChatMessage {
            role: ChatRole::User,
            content: prompt.to_owned(),
        }],
        temperature: 0.0,
        max_tokens,
        stop_sequences: vec![],
        stream: false,
    }
}

fn compute_run(run: u32, resp: &InferenceResponse, total_ms: u64, device: &str) -> RunResult {
    let n_in = resp.tokens_in as f64;
    let n_out = resp.tokens_out as f64;
    let n_total = n_in + n_out;

    let ttft_ms = if n_total > 0.0 {
        (total_ms as f64 * n_in / n_total) as u64
    } else {
        total_ms
    };

    let total_s = total_ms as f64 / 1_000.0;
    let decode_s = if n_out > 0.0 {
        total_s * n_out / n_total
    } else {
        total_s
    };
    let prefill_s = total_s - decode_s;

    RunResult {
        run,
        tokens_in: resp.tokens_in,
        tokens_out: resp.tokens_out,
        total_ms,
        ttft_ms,
        prompt_tps: if prefill_s > 1e-9 {
            n_in / prefill_s
        } else {
            0.0
        },
        gen_tps: if decode_s > 1e-9 {
            n_out / decode_s
        } else {
            0.0
        },
        device: device.to_owned(),
    }
}

fn make_summary(
    model: &PathBuf,
    device: &str,
    warmup: u32,
    measurement_runs: u32,
    max_tokens: u32,
    results: &[RunResult],
) -> Summary {
    let n = results.len() as f64;
    let mean_gen_tps = results.iter().map(|r| r.gen_tps).sum::<f64>() / n;
    let mean_prompt_tps = results.iter().map(|r| r.prompt_tps).sum::<f64>() / n;
    let mean_ttft_ms = results.iter().map(|r| r.ttft_ms as f64).sum::<f64>() / n;

    let mut sorted: Vec<f64> = results.iter().map(|r| r.total_ms as f64).collect();
    sorted.sort_by(f64::total_cmp);

    Summary {
        model: model.display().to_string(),
        device: device.to_owned(),
        warmup_runs: warmup,
        measurement_runs,
        max_tokens,
        mean_gen_tps,
        mean_prompt_tps,
        mean_ttft_ms,
        p50_ms: percentile(&sorted, 50.0),
        p95_ms: percentile(&sorted, 95.0),
        p99_ms: percentile(&sorted, 99.0),
        runs: results.to_vec(),
    }
}

fn percentile(sorted: &[f64], pct: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    if sorted.len() == 1 {
        return sorted[0];
    }
    let idx = pct / 100.0 * (sorted.len() - 1) as f64;
    let lo = idx.floor() as usize;
    let hi = idx.ceil() as usize;
    sorted[lo] + (idx - lo as f64) * (sorted[hi] - sorted[lo])
}

fn progress_bar(total: u32, label: &str) -> ProgressBar {
    let pb = ProgressBar::new(total as u64);
    pb.set_style(
        ProgressStyle::with_template(
            "{spinner:.cyan} {msg:15} [{bar:35.green/dim}] {pos}/{len} {wide_msg}",
        )
        .unwrap()
        .progress_chars("█▉▊▋▌▍▎▏ "),
    );
    pb.set_message(label.to_string());
    pb.enable_steady_tick(Duration::from_millis(80));
    pb
}

fn print_table(s: &Summary) {
    let sep = "─".repeat(74);
    println!("\n{}", sep.dimmed());
    println!(" {}", "maranode-bench".bold());
    println!("{}", sep.dimmed());
    println!("  {}  {}", "Model :".dimmed(), s.model.cyan());
    println!("  {}  {}", "Device:".dimmed(), s.device.cyan().bold());
    println!(
        "  {}  warmup {} · runs {} · max_tokens {}",
        "Config:".dimmed(),
        s.warmup_runs,
        s.measurement_runs,
        s.max_tokens
    );
    println!("{}", sep.dimmed());

    println!(
        "  {:>4}  {:>9}  {:>8}  {:>10}  {:>9}  {}",
        "Run".bold(),
        "Total ms".bold(),
        "TTFT ms".bold(),
        "Prompt t/s".bold(),
        "Gen t/s".bold(),
        "Tokens".bold()
    );
    println!("  {}", "─".repeat(68).dimmed());

    for r in &s.runs {
        println!(
            "  {:>4}  {:>9}  {:>8}  {:>10.1}  {:>9.1}  {}→{}",
            r.run,
            r.total_ms,
            r.ttft_ms,
            r.prompt_tps,
            format!("{:.1}", r.gen_tps).green(),
            r.tokens_in,
            r.tokens_out,
        );
    }

    println!("{}", sep.dimmed());
    println!("  {}", "Aggregate".bold().underline());
    println!(
        "    Generation throughput  {:>8.1} tok/s",
        format!("{:.1}", s.mean_gen_tps).green().bold()
    );
    println!(
        "    Prompt throughput      {:>8.1} tok/s",
        s.mean_prompt_tps
    );
    println!("    Mean TTFT              {:>8.1} ms", s.mean_ttft_ms);
    println!("    P50 latency            {:>8.1} ms", s.p50_ms);
    println!("    P95 latency            {:>8.1} ms", s.p95_ms);
    println!("    P99 latency            {:>8.1} ms", s.p99_ms);
    println!("{}\n", sep.dimmed());
}

fn print_comparison(baseline: &Summary, current: &Summary) {
    let sep = "─".repeat(74);
    println!("{}", sep.dimmed());
    println!(" {}", "Comparison".bold());
    println!("{}", sep.dimmed());
    println!(
        "  {}  {} ({})",
        "Baseline:".dimmed(),
        baseline.model.dimmed(),
        baseline.device.cyan()
    );
    println!(
        "  {}   {} ({})",
        "Current: ".dimmed(),
        current.model.dimmed(),
        current.device.cyan().bold()
    );
    println!("{}", sep.dimmed());

    diff_row(
        "Generation t/s",
        baseline.mean_gen_tps,
        current.mean_gen_tps,
        true,
    );
    diff_row(
        "Prompt t/s",
        baseline.mean_prompt_tps,
        current.mean_prompt_tps,
        true,
    );
    diff_row(
        "Mean TTFT ms",
        baseline.mean_ttft_ms,
        current.mean_ttft_ms,
        false,
    );
    diff_row("P50 ms", baseline.p50_ms, current.p50_ms, false);
    diff_row("P95 ms", baseline.p95_ms, current.p95_ms, false);
    diff_row("P99 ms", baseline.p99_ms, current.p99_ms, false);
    println!("{}\n", sep.dimmed());
}

fn diff_row(label: &str, baseline: f64, current: f64, higher_is_better: bool) {
    let pct = if baseline > 1e-9 {
        (current - baseline) / baseline * 100.0
    } else {
        0.0
    };

    let better = if higher_is_better {
        pct > 0.0
    } else {
        pct < 0.0
    };
    let worse = if higher_is_better {
        pct < 0.0
    } else {
        pct > 0.0
    };

    let pct_str = if better {
        format!("{:+.1}%", pct).green().bold().to_string()
    } else if worse {
        format!("{:+.1}%", pct).red().to_string()
    } else {
        format!("{:+.1}%", pct).dimmed().to_string()
    };

    println!(
        "  {:<22}  baseline {:>8.1}  current {:>8.1}  {}",
        label, baseline, current, pct_str
    );
}
