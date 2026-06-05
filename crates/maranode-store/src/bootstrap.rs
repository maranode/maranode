//! first run; scan models folder, ask user, download default models if needed

use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};

use maranode_common::models::{ModelId, ModelManifest, ModelType};
use anyhow::{Context, Result};
use tracing::{info, warn};

use crate::defaults::{default_embedding, default_llm, DefaultModelSpec};
use crate::download::download_file;
use crate::ModelStore;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelCoverage {
    pub has_llm: bool,
    pub has_embedding: bool,
}

#[derive(Debug, Clone)]
pub struct BootstrapOptions {
    pub models_dir: PathBuf,
    pub skip: bool,
    pub yes: bool,
}

impl Default for BootstrapOptions {
    fn default() -> Self {
        Self {
            models_dir: maranode_common::paths::default_models_dir(),
            skip: false,
            yes: false,
        }
    }
}

pub async fn maybe_bootstrap(store: &ModelStore, opts: &BootstrapOptions) -> Result<()> {
    if opts.skip {
        return Ok(());
    }

    info!("Checking models directory: {}", opts.models_dir.display());

    import_local_gguf(store, &opts.models_dir).await?;
    materialize_defaults_from_store(store, &opts.models_dir).await?;

    let dir = scan_models_dir(&opts.models_dir);
    if dir.has_llm && dir.has_embedding {
        ensure_store_has_defaults(store, &opts.models_dir).await?;
        return Ok(());
    }

    let missing = missing_defaults(&dir);
    if missing.is_empty() {
        return Ok(());
    }

    if !prompt_download(&opts.models_dir, &missing, opts.yes)? {
        warn!(
            "Starting without default models in {}. Import or download manually:\n  \
             maranode model import /path/to/model.gguf --name NAME --tag TAG [--type embedding]",
            opts.models_dir.display()
        );
        return Ok(());
    }

    tokio::fs::create_dir_all(&opts.models_dir).await?;

    for spec in &missing {
        download_and_import(store, &opts.models_dir, spec).await?;
    }

    info!("Default models downloaded and imported");
    Ok(())
}

async fn materialize_defaults_from_store(store: &ModelStore, models_dir: &Path) -> Result<()> {
    for spec in [default_llm(), default_embedding()] {
        let dest = models_dir.join(spec.local_filename);
        if dest.exists() {
            continue;
        }
        if store.get(&spec.model_id).await?.is_none() {
            continue;
        }
        let src = store
            .blob_path(&spec.model_id)
            .await
            .with_context(|| format!("resolving blob for {}", spec.model_id))?;
        tokio::fs::create_dir_all(models_dir).await?;
        link_or_copy(&src, &dest)?;
        info!(
            "Linked {} into {} (already in model store)",
            spec.model_id,
            dest.display()
        );
    }
    Ok(())
}

async fn ensure_store_has_defaults(store: &ModelStore, models_dir: &Path) -> Result<()> {
    for spec in [default_llm(), default_embedding()] {
        if store.get(&spec.model_id).await?.is_some() {
            continue;
        }
        let path = models_dir.join(spec.local_filename);
        if !path.exists() {
            continue;
        }
        import_gguf(
            store,
            &path,
            &spec.model_id,
            spec.model_type.clone(),
            Some(spec.quant.to_string()),
        )
        .await?;
    }
    Ok(())
}

fn link_or_copy(src: &Path, dest: &Path) -> Result<()> {
    if let Err(e) = std::fs::hard_link(src, dest) {
        std::fs::copy(src, dest)
            .with_context(|| format!("copying {} → {}: {}", src.display(), dest.display(), e))?;
    }
    Ok(())
}

async fn store_coverage(store: &ModelStore) -> Result<ModelCoverage> {
    let models = store.list().await?;
    Ok(coverage_from_manifests(&models))
}

fn coverage_from_manifests(models: &[ModelManifest]) -> ModelCoverage {
    let mut cov = ModelCoverage {
        has_llm: false,
        has_embedding: false,
    };
    for m in models {
        match m.model_type {
            ModelType::Llm => cov.has_llm = true,
            ModelType::Embedding => cov.has_embedding = true,
        }
    }
    cov
}

fn scan_models_dir(dir: &Path) -> ModelCoverage {
    let mut cov = ModelCoverage {
        has_llm: false,
        has_embedding: false,
    };
    for path in find_gguf_files(dir) {
        match classify_gguf_path(&path) {
            ModelType::Llm => cov.has_llm = true,
            ModelType::Embedding => cov.has_embedding = true,
        }
    }
    cov
}

fn missing_defaults(coverage: &ModelCoverage) -> Vec<DefaultModelSpec> {
    let mut out = Vec::new();
    if !coverage.has_llm {
        out.push(default_llm());
    }
    if !coverage.has_embedding {
        out.push(default_embedding());
    }
    out
}

fn prompt_download(
    models_dir: &Path,
    missing: &[DefaultModelSpec],
    auto_yes: bool,
) -> Result<bool> {
    if auto_yes || std::env::var("MARANODE_YES_BOOTSTRAP").ok().as_deref() == Some("1") {
        return Ok(true);
    }

    let is_tty = io::stdin().is_terminal() && io::stderr().is_terminal();
    if !is_tty {
        warn!(
            "No models installed and not a TTY: skipping automatic download. \
             Set MARANODE_YES_BOOTSTRAP=1 or pass --yes-bootstrap to download defaults."
        );
        return Ok(false);
    }

    eprintln!();
    eprintln!(
        "No GGUF models found in {} (need one chat + one embedding model).",
        models_dir.display()
    );
    eprintln!();
    eprintln!("Download default models from Hugging Face?");
    for spec in missing {
        let kind = match spec.model_type {
            ModelType::Llm => "chat",
            ModelType::Embedding => "RAG embeddings",
        };
        eprintln!(
            "  • {}:{}  ({}, {})",
            spec.model_id.name, spec.model_id.tag, kind, spec.size_hint
        );
    }
    eprintln!();
    eprint!("Download now? [Y/n] ");
    io::stderr().flush()?;

    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    let answer = line.trim().to_lowercase();
    Ok(answer.is_empty() || answer == "y" || answer == "yes")
}

async fn download_and_import(
    store: &ModelStore,
    models_dir: &Path,
    spec: &DefaultModelSpec,
) -> Result<()> {
    let dest = models_dir.join(spec.local_filename);
    if !dest.exists() {
        info!(
            "Downloading {}:{} from {} …",
            spec.model_id.name, spec.model_id.tag, spec.hf_repo
        );
        eprintln!("  → {}", spec.download_url());
        download_file(&spec.download_url(), &dest)
            .await
            .with_context(|| format!("downloading {}", spec.model_id))?;
    } else {
        info!("Using existing file {}", dest.display());
    }

    import_gguf(
        store,
        &dest,
        &spec.model_id,
        spec.model_type.clone(),
        Some(spec.quant.to_string()),
    )
    .await
}

async fn import_local_gguf(store: &ModelStore, models_dir: &Path) -> Result<()> {
    for path in find_gguf_files(models_dir) {
        if let Some(spec) = spec_for_local_file(&path) {
            if store.get(&spec.model_id).await?.is_some() {
                continue;
            }
            info!("Importing {} from {}", spec.model_id, path.display());
            import_gguf(
                store,
                &path,
                &spec.model_id,
                spec.model_type,
                Some(spec.quant.to_string()),
            )
            .await?;
        }
    }
    Ok(())
}

async fn import_gguf(
    store: &ModelStore,
    path: &Path,
    model_id: &ModelId,
    model_type: ModelType,
    quant: Option<String>,
) -> Result<()> {
    if store.get(model_id).await?.is_some() {
        return Ok(());
    }
    store
        .import_from_file(path, model_id.clone(), quant, model_type)
        .await
        .with_context(|| format!("importing {}", model_id))?;
    Ok(())
}

fn spec_for_local_file(path: &Path) -> Option<DefaultModelSpec> {
    let name = path.file_name()?.to_string_lossy().to_lowercase();
    let model_type = classify_gguf_name(&name);

    if name.contains("qwen2.5") && name.contains("7b") {
        return Some(default_llm());
    }
    if name.contains("bge-m3") || name.contains("bge_m3") {
        return Some(default_embedding());
    }

    // Unknown file: use filename stem as model id and guess type from name
    let stem = path.file_stem()?.to_string_lossy();
    let tag = "latest".to_string();
    let id_name = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .to_lowercase();

    Some(DefaultModelSpec {
        model_id: ModelId::new(id_name, tag),
        model_type,
        hf_repo: "",
        hf_filename: "",
        local_filename: "",
        quant: "unknown",
        size_hint: "",
    })
}

fn classify_gguf_path(path: &Path) -> ModelType {
    classify_gguf_name(&path.to_string_lossy().to_lowercase())
}

fn classify_gguf_name(name: &str) -> ModelType {
    if name.contains("/rag/")
        || name.contains("/embed")
        || name.contains("/embedding")
        || name.contains("bge")
        || name.contains("embed")
        || name.contains("nomic")
        || name.contains("-e5")
        || name.contains("gte-")
    {
        ModelType::Embedding
    } else {
        ModelType::Llm
    }
}

fn find_gguf_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if !dir.is_dir() {
        return out;
    }
    walk_gguf(dir, &mut out);
    out.sort();
    out
}

fn walk_gguf(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_gguf(&path, out);
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("gguf"))
        {
            out.push(path);
        }
    }
}
