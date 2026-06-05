use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Subcommand;
use colored::Colorize;

use maranode_store::ModelStore;

#[derive(Debug, Clone)]
pub struct QuantFormat {
    pub name: &'static str,
    pub bits_per_weight: f64,
    pub blurb: &'static str,
    pub default_pick: bool,
}

const FORMATS: &[QuantFormat] = &[
    QuantFormat {
        name: "F32",
        bits_per_weight: 32.0,
        blurb: "full precision",
        default_pick: false,
    },
    QuantFormat {
        name: "F16",
        bits_per_weight: 16.0,
        blurb: "half, still large",
        default_pick: false,
    },
    QuantFormat {
        name: "Q8_0",
        bits_per_weight: 8.5,
        blurb: "near lossless",
        default_pick: false,
    },
    QuantFormat {
        name: "Q6_K",
        bits_per_weight: 6.6,
        blurb: "high quality",
        default_pick: false,
    },
    QuantFormat {
        name: "Q5_K_M",
        bits_per_weight: 5.7,
        blurb: "high quality, needs ram",
        default_pick: false,
    },
    QuantFormat {
        name: "Q5_K_S",
        bits_per_weight: 5.5,
        blurb: "q5 small",
        default_pick: false,
    },
    QuantFormat {
        name: "Q5_0",
        bits_per_weight: 5.5,
        blurb: "legacy q5",
        default_pick: false,
    },
    QuantFormat {
        name: "Q5_1",
        bits_per_weight: 5.8,
        blurb: "legacy q5 + scale",
        default_pick: false,
    },
    QuantFormat {
        name: "Q4_K_M",
        bits_per_weight: 4.8,
        blurb: "usual default",
        default_pick: true,
    },
    QuantFormat {
        name: "Q4_K_S",
        bits_per_weight: 4.6,
        blurb: "slightly smaller q4",
        default_pick: false,
    },
    QuantFormat {
        name: "Q4_0",
        bits_per_weight: 4.5,
        blurb: "legacy q4",
        default_pick: false,
    },
    QuantFormat {
        name: "Q4_1",
        bits_per_weight: 4.8,
        blurb: "legacy q4 + scale",
        default_pick: false,
    },
    QuantFormat {
        name: "Q3_K_L",
        bits_per_weight: 3.6,
        blurb: "low ram",
        default_pick: false,
    },
    QuantFormat {
        name: "Q3_K_M",
        bits_per_weight: 3.4,
        blurb: "low ram",
        default_pick: false,
    },
    QuantFormat {
        name: "Q3_K_S",
        bits_per_weight: 3.0,
        blurb: "very low ram",
        default_pick: false,
    },
    QuantFormat {
        name: "Q2_K",
        bits_per_weight: 2.6,
        blurb: "smallest, noisy",
        default_pick: false,
    },
    QuantFormat {
        name: "IQ4_XS",
        bits_per_weight: 4.3,
        blurb: "imatrix q4",
        default_pick: false,
    },
    QuantFormat {
        name: "IQ3_M",
        bits_per_weight: 3.7,
        blurb: "imatrix q3",
        default_pick: false,
    },
    QuantFormat {
        name: "IQ2_M",
        bits_per_weight: 2.7,
        blurb: "imatrix q2",
        default_pick: false,
    },
];

fn gguf_file_type_name(file_type: u32) -> &'static str {
    match file_type {
        0 => "F32",
        1 => "F16",
        2 => "Q4_0",
        3 => "Q4_1",
        7 => "Q8_0",
        8 => "Q5_0",
        9 => "Q5_1",
        10 => "Q2_K",
        11 => "Q3_K_S",
        12 => "Q3_K_M",
        13 => "Q3_K_L",
        14 => "Q4_K_S",
        15 => "Q4_K_M",
        16 => "Q5_K_S",
        17 => "Q5_K_M",
        18 => "Q6_K",
        19 => "IQ2_XXS",
        20 => "IQ2_XS",
        21 => "Q2_K_S",
        22 => "IQ3_XS",
        23 => "IQ3_S",
        24 => "IQ3_M",
        25 => "IQ4_NL",
        26 => "IQ4_XS",
        27 => "IQ1_S",
        28 => "IQ1_M",
        _ => "unknown",
    }
}

fn lookup_format(name: &str) -> Option<&'static QuantFormat> {
    FORMATS.iter().find(|f| f.name.eq_ignore_ascii_case(name))
}

const GGUF_MAGIC: u32 = 0x4655_4747;

#[derive(Debug)]
pub struct GgufMeta {
    pub version: u32,
    pub tensor_count: u64,
    pub kv_count: u64,
    pub file_type: Option<u32>,
    pub quantization_name: String,
    pub context_length: Option<u64>,
    pub param_count_billions: Option<f64>,
    pub architecture: Option<String>,
}

pub fn read_gguf_meta(path: &Path) -> Result<GgufMeta> {
    let data = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    parse_gguf_meta(&data)
}

fn u32_le(data: &[u8], off: usize) -> Option<u32> {
    let b: [u8; 4] = data.get(off..off + 4)?.try_into().ok()?;
    Some(u32::from_le_bytes(b))
}

fn u64_le(data: &[u8], off: usize) -> Option<u64> {
    let b: [u8; 8] = data.get(off..off + 8)?.try_into().ok()?;
    Some(u64::from_le_bytes(b))
}

fn i32_le(data: &[u8], off: usize) -> Option<i32> {
    let b: [u8; 4] = data.get(off..off + 4)?.try_into().ok()?;
    Some(i32::from_le_bytes(b))
}

fn parse_gguf_meta(data: &[u8]) -> Result<GgufMeta> {
    if data.len() < 24 {
        anyhow::bail!("file too small for gguf");
    }
    if u32_le(data, 0) != Some(GGUF_MAGIC) {
        anyhow::bail!("not a gguf file");
    }
    let version = u32_le(data, 4).unwrap();
    if !(2..=3).contains(&version) {
        anyhow::bail!("unsupported gguf version {}", version);
    }
    let tensor_count = u64_le(data, 8).unwrap();
    let kv_count = u64_le(data, 16).unwrap();

    let mut pos = 24usize;
    let mut file_type = None;
    let mut context_length = None;
    let mut architecture = None;

    for _ in 0..kv_count {
        let key_len = match u64_le(data, pos) {
            Some(n) => n as usize,
            None => break,
        };
        pos += 8;
        let key = match data.get(pos..pos + key_len) {
            Some(b) => std::str::from_utf8(b).unwrap_or("").to_string(),
            None => break,
        };
        pos += key_len;

        let val_type = match u32_le(data, pos) {
            Some(v) => v,
            None => break,
        };
        pos += 4;

        match val_type {
            8 => {
                let v = match u32_le(data, pos) {
                    Some(v) => v,
                    None => break,
                };
                pos += 4;
                if key == "general.file_type" {
                    file_type = Some(v);
                }
            }
            4 => {
                let v = match u64_le(data, pos) {
                    Some(v) => v,
                    None => break,
                };
                pos += 8;
                if key.ends_with(".context_length") {
                    context_length = Some(v);
                }
            }
            0 | 1 | 7 => pos += 1,
            2 | 3 => pos += 2,
            5 => {
                let _ = i32_le(data, pos);
                pos += 4;
            }
            6 => pos += 4,
            9 => pos += 8,
            10 => pos += 8,
            11 => {
                let slen = match u64_le(data, pos) {
                    Some(n) => n as usize,
                    None => break,
                };
                pos += 8;
                if key == "general.architecture" {
                    architecture = data
                        .get(pos..pos + slen)
                        .and_then(|b| std::str::from_utf8(b).ok())
                        .map(str::to_string);
                }
                pos += slen;
            }
            12 => {
                let elem_type = match u32_le(data, pos) {
                    Some(v) => v,
                    None => break,
                };
                pos += 4;
                let arr_len = match u64_le(data, pos) {
                    Some(n) => n as usize,
                    None => break,
                };
                pos += 8;
                if elem_type == 11 {
                    for _ in 0..arr_len {
                        let slen = match u64_le(data, pos) {
                            Some(n) => n as usize,
                            None => break,
                        };
                        pos += 8 + slen;
                    }
                } else {
                    let elem_size = match elem_type {
                        0 | 1 | 7 => 1,
                        2 | 3 => 2,
                        4 | 5 | 6 | 8 => 4,
                        9 | 10 => 8,
                        _ => break,
                    };
                    pos += arr_len * elem_size;
                }
            }
            _ => break,
        }
    }

    let quant_name = file_type
        .map(gguf_file_type_name)
        .unwrap_or("unknown")
        .to_string();

    let param_count_billions = file_type.and_then(|ft| {
        let fmt = lookup_format(gguf_file_type_name(ft))?;
        let params = (data.len() as f64 * 8.0) / (fmt.bits_per_weight * 1e9);
        Some(params)
    });

    Ok(GgufMeta {
        version,
        tensor_count,
        kv_count,
        file_type,
        quantization_name: quant_name,
        context_length,
        param_count_billions,
        architecture,
    })
}

pub struct SizeRow {
    pub format: &'static QuantFormat,
    pub size_gb: f64,
    pub fits: bool,
}

pub fn size_table(params_b: f64, ram_gb: f64) -> Vec<SizeRow> {
    FORMATS
        .iter()
        .filter(|f| f.bits_per_weight <= 16.0)
        .map(|f| {
            let size_gb = params_b * f.bits_per_weight / 8.0;
            let fits = size_gb * 1.1 <= ram_gb;
            SizeRow {
                format: f,
                size_gb,
                fits,
            }
        })
        .collect()
}

#[derive(Subcommand)]
pub enum QuantCommand {
    /// show GGUF quantization info for file or stored model
    Inspect {
        target: String,
        #[arg(long, env = "MARANODE_DATA_DIR")]
        data_dir: Option<PathBuf>,
    },
    /// recommend quantization from parameter count and RAM
    Recommend {
        #[arg(long, short = 'p')]
        params: f64,
        #[arg(long, short = 'r')]
        ram: f64,
    },
    List,
}

pub async fn run(cmd: QuantCommand, data_dir: &Path) -> Result<()> {
    match cmd {
        QuantCommand::Inspect {
            target,
            data_dir: dir_override,
        } => {
            let dir = dir_override.as_deref().unwrap_or(data_dir);
            cmd_inspect(&resolve_target(&target, dir).await?)
        }
        QuantCommand::Recommend { params, ram } => cmd_recommend(params, ram),
        QuantCommand::List => cmd_list(),
    }
}

async fn resolve_target(target: &str, data_dir: &Path) -> Result<PathBuf> {
    let p = Path::new(target);
    if p.exists() {
        return Ok(p.to_path_buf());
    }
    if target.ends_with(".gguf") {
        anyhow::bail!("file not found: {target}");
    }
    let model_id = maranode_common::models::ModelId::parse(target)
        .ok_or_else(|| anyhow::anyhow!("'{target}' is not a path or name:tag"))?;
    let store = ModelStore::open(data_dir)?;
    let manifest = store
        .get(&model_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("model '{target}' not in store"))?;
    Ok(PathBuf::from(&manifest.blob_path))
}

fn cmd_inspect(path: &Path) -> Result<()> {
    let meta = read_gguf_meta(path).with_context(|| format!("{}", path.display()))?;

    println!("{}", path.display().to_string().cyan().bold());
    println!(
        "  gguf v{}  tensors {}  kv {}",
        meta.version, meta.tensor_count, meta.kv_count
    );
    if let Some(arch) = &meta.architecture {
        println!("  arch {}", arch.cyan());
    }
    if let Some(ctx) = meta.context_length {
        println!("  ctx {}", ctx);
    }

    println!("  quant {}", meta.quantization_name.yellow().bold());
    if let Some(fmt) = lookup_format(&meta.quantization_name) {
        println!("  {:.1} bpw  {}", fmt.bits_per_weight, fmt.blurb);
        if fmt.default_pick {
            println!("  {}", "(default pick for new downloads)".dimmed());
        }
    }

    if let Some(est) = meta.param_count_billions {
        println!("  ~{:.1}B params (from file size)", est);
        println!();
        for row in size_table(est, f64::MAX) {
            let tag = if row.format.name == meta.quantization_name {
                " current".yellow().to_string()
            } else if row.format.default_pick {
                " default".green().to_string()
            } else {
                String::new()
            };
            println!("  {:10} {:5.1} GB{}", row.format.name, row.size_gb, tag);
        }
    }

    Ok(())
}

fn cmd_recommend(params_b: f64, ram_gb: f64) -> Result<()> {
    println!("{:.1}B model, {:.0} GB ram", params_b, ram_gb);
    println!(
        "  {:<10} {:>7} {:>4}  {}",
        "format".bold(),
        "gb".bold(),
        "ok".bold(),
        "note".bold(),
    );

    let rows = size_table(params_b, ram_gb);
    for row in &rows {
        let ok = if row.fits { "yes".green() } else { "no".red() };
        let name = if row.format.default_pick {
            row.format.name.yellow().bold().to_string()
        } else {
            row.format.name.into()
        };
        let note = if !row.fits {
            "won't fit".dimmed().to_string()
        } else if row.format.default_pick {
            row.format.blurb.green().to_string()
        } else {
            row.format.blurb.dimmed().to_string()
        };
        println!("  {:<10} {:>7.1} {:>4}  {}", name, row.size_gb, ok, note);
    }

    println!();
    let fitting: Vec<_> = rows.iter().filter(|r| r.fits).collect();
    if let Some(pick) = fitting.iter().find(|r| r.format.default_pick) {
        println!(
            "use {} ({:.1} GB)",
            pick.format.name.yellow().bold(),
            pick.size_gb
        );
    } else if let Some(pick) = fitting.first() {
        println!(
            "best fit: {} ({:.1} GB)",
            pick.format.name.bold(),
            pick.size_gb
        );
        if let Some(q4) = rows.iter().find(|r| r.format.name == "Q4_K_M") {
            println!(
                "  q4_k_m wants {:.1} GB: add ram or shrink the model",
                q4.size_gb
            );
        }
    } else {
        println!(
            "nothing fits in {:.0} GB: need a smaller base model",
            ram_gb
        );
    }

    Ok(())
}

fn cmd_list() -> Result<()> {
    for f in FORMATS {
        let star = if f.default_pick { " *" } else { "" };
        let name = format!("{}{}", f.name, star);
        println!(
            "  {:<10} {:>4.1}  {}",
            if f.default_pick {
                name.yellow().bold().to_string()
            } else {
                name
            },
            f.bits_per_weight,
            f.blurb.dimmed()
        );
    }
    println!();
    println!("  {} = default (q4_k_m)", "*".yellow());
    Ok(())
}
