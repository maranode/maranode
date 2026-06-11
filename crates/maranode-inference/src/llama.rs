//! GGUF inference with llama-cpp-2. One context per request; engine is not Sync.

use std::collections::HashMap;
use std::num::NonZeroU32;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, info, warn};

struct ModelCache {
    by_path: HashMap<String, (Arc<LlamaModel>, Instant)>,
    id_to_path: HashMap<String, String>,
}

use maranode_common::models::{ChatMessage, ChatRole, InferenceDevice};
use llama_cpp_2::{
    context::params::{LlamaContextParams, LlamaPoolingType},
    llama_backend::LlamaBackend,
    llama_batch::LlamaBatch,
    model::{params::LlamaModelParams, AddBos, LlamaModel, Special},
    sampling::LlamaSampler,
    token::LlamaToken,
};

use crate::engine::{async_trait, InferenceEngine};
use crate::types::{FinishReason, InferenceRequest, InferenceResponse, Token};

const MAX_N_CTX: u32 = 32768;
const ALL_LAYERS_ON_GPU: u32 = 9999;
/// must be the same as `LlamaContextParams::n_batch` (llama.cpp checks this)
const N_BATCH: u32 = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevicePreference {
    Auto,
    Cpu,
    Gpu,
    Npu,
    RyzenAi,
}

pub struct LlamaCppEngine {
    backend: Arc<LlamaBackend>,
    cache: Arc<Mutex<ModelCache>>,
    device: InferenceDevice,
    n_gpu_layers: u32,
    max_loaded_models: usize,
}

impl std::fmt::Debug for LlamaCppEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LlamaCppEngine")
            .field("device", &self.device)
            .field("n_gpu_layers", &self.n_gpu_layers)
            .field("max_loaded_models", &self.max_loaded_models)
            .finish_non_exhaustive()
    }
}

fn shared_backend() -> Result<Arc<LlamaBackend>> {
    use std::sync::{Mutex as StdMutex, OnceLock};
    static SLOT: OnceLock<StdMutex<Option<Arc<LlamaBackend>>>> = OnceLock::new();

    let slot = SLOT.get_or_init(|| StdMutex::new(None));
    let mut guard = slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(b) = guard.as_ref() {
        return Ok(Arc::clone(b));
    }
    let backend = Arc::new(LlamaBackend::init().context("initialising llama.cpp backend")?);
    *guard = Some(Arc::clone(&backend));
    Ok(backend)
}

impl LlamaCppEngine {
    pub fn new(pref: DevicePreference, max_loaded_models: usize) -> Result<Self> {
        let backend = shared_backend()?;

        let (device, n_gpu_layers) = resolve_device(pref)?;

        #[cfg(feature = "openvino")]
        if device == InferenceDevice::Npu {
            validate_openvino_runtime()?;
        }

        #[cfg(feature = "ryzenai")]
        if device == InferenceDevice::RyzenAi {
            validate_ryzenai_runtime()?;
        }

        info!(
            "llama.cpp backend initialised (device={}, n_gpu_layers={}, max_loaded_models={})",
            device, n_gpu_layers, max_loaded_models
        );

        Ok(Self {
            backend,
            cache: Arc::new(Mutex::new(ModelCache {
                by_path: HashMap::new(),
                id_to_path: HashMap::new(),
            })),
            device,
            n_gpu_layers,
            max_loaded_models,
        })
    }

    async fn get_or_load(&self, path: &Path) -> Result<Arc<LlamaModel>> {
        let key = path.to_string_lossy().into_owned();

        {
            let mut cache = self.cache.lock().await;
            if let Some(entry) = cache.by_path.get_mut(&key) {
                entry.1 = Instant::now();
                debug!("model cache hit: {}", key);
                return Ok(Arc::clone(&entry.0));
            }
        }

        info!(
            "loading GGUF model: {} (n_gpu_layers={})",
            key, self.n_gpu_layers
        );
        let backend = Arc::clone(&self.backend);
        let path_buf = path.to_path_buf();
        let n_gpu_layers = self.n_gpu_layers;

        let model = tokio::task::spawn_blocking(move || -> Result<LlamaModel> {
            let params = LlamaModelParams::default().with_n_gpu_layers(n_gpu_layers);
            LlamaModel::load_from_file(&backend, &path_buf, &params)
                .with_context(|| format!("loading GGUF: {}", path_buf.display()))
        })
        .await
        .context("thread join for model load")??;

        let model = Arc::new(model);

        let mut cache = self.cache.lock().await;
        cache.by_path.insert(key.clone(), (Arc::clone(&model), Instant::now()));

        if self.max_loaded_models > 0 && cache.by_path.len() > self.max_loaded_models {
            evict_lru(&mut cache, &key);
        }

        info!("model loaded and cached: {}", key);
        Ok(model)
    }
}

fn evict_lru(cache: &mut ModelCache, just_loaded: &str) {
    let victim = cache
        .by_path
        .iter()
        .filter(|(k, _)| k.as_str() != just_loaded)
        .min_by_key(|(_, (_, t))| *t)
        .map(|(k, _)| k.clone());

    if let Some(k) = victim {
        cache.id_to_path.retain(|_, v| v != &k);
        cache.by_path.remove(&k);
        info!("evicted model from cache (LRU): {}", k);
    }
}

fn resolve_device(pref: DevicePreference) -> Result<(InferenceDevice, u32)> {
    let compiled_gpu: Option<InferenceDevice> = {
        #[cfg(feature = "metal")]
        {
            Some(InferenceDevice::Metal)
        }
        #[cfg(all(feature = "cuda", not(feature = "metal")))]
        {
            Some(InferenceDevice::Gpu)
        }
        #[cfg(all(feature = "rocm", not(any(feature = "metal", feature = "cuda"))))]
        {
            Some(InferenceDevice::Gpu)
        }
        #[cfg(all(
            feature = "vulkan",
            not(any(feature = "metal", feature = "cuda", feature = "rocm"))
        ))]
        {
            Some(InferenceDevice::Gpu)
        }
        #[cfg(not(any(
            feature = "metal",
            feature = "cuda",
            feature = "rocm",
            feature = "vulkan"
        )))]
        {
            None
        }
    };

    let compiled_npu: Option<InferenceDevice> = {
        #[cfg(feature = "openvino")]
        {
            Some(InferenceDevice::Npu)
        }
        #[cfg(not(feature = "openvino"))]
        {
            None
        }
    };

    let compiled_ryzenai: Option<InferenceDevice> = {
        #[cfg(feature = "ryzenai")]
        {
            Some(InferenceDevice::RyzenAi)
        }
        #[cfg(not(feature = "ryzenai"))]
        {
            None
        }
    };

    match pref {
        DevicePreference::Auto => {
            // device order when auto: GPU, then Intel NPU, then AMD Ryzen AI, then CPU
            if let Some(dev) = compiled_gpu {
                Ok((dev, ALL_LAYERS_ON_GPU))
            } else if let Some(dev) = compiled_npu {
                Ok((dev, ALL_LAYERS_ON_GPU))
            } else if let Some(dev) = compiled_ryzenai {
                Ok((dev, ALL_LAYERS_ON_GPU))
            } else {
                Ok((InferenceDevice::Cpu, 0))
            }
        }

        DevicePreference::Cpu => Ok((InferenceDevice::Cpu, 0)),

        DevicePreference::Gpu => {
            compiled_gpu
                .map(|dev| (dev, ALL_LAYERS_ON_GPU))
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "GPU device requested but no GPU backend was compiled in. \
                     Rebuild with one of: --features metal, --features cuda, \
                     --features rocm, or --features vulkan."
                    )
                })
        }

        DevicePreference::Npu => {
            compiled_npu
                .map(|dev| (dev, ALL_LAYERS_ON_GPU))
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "NPU device requested but OpenVINO backend was not compiled in. \
                     Rebuild with: LLAMA_CMAKE_ARGS=\"-DGGML_OPENVINO=ON\" \
                     cargo build --features openvino"
                    )
                })
        }

        DevicePreference::RyzenAi => compiled_ryzenai
            .map(|dev| (dev, ALL_LAYERS_ON_GPU))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "AMD Ryzen AI device requested but the ryzenai backend was not compiled in. \
                     Rebuild with: LLAMA_CMAKE_ARGS=\"-DGGML_RYZEN_AI=ON\" \
                     cargo build --features ryzenai"
                )
            }),
    }
}

#[cfg(feature = "openvino")]
fn validate_openvino_runtime() -> Result<()> {
    // OpenVINO library file name for each operating system
    #[cfg(target_os = "linux")]
    const LIB_NAME: &str = "libopenvino_c.so";
    #[cfg(target_os = "macos")]
    const LIB_NAME: &str = "libopenvino_c.dylib";
    #[cfg(target_os = "windows")]
    const LIB_NAME: &str = "openvino_c.dll";

    let mut dirs: Vec<std::path::PathBuf> = Vec::new();

    if let Ok(d) = std::env::var("OPENVINO_DIR") {
        dirs.push(std::path::PathBuf::from(&d).join("runtime/lib/intel64"));
        dirs.push(std::path::PathBuf::from(&d).join("runtime/bin/intel64/Release"));
    }

    dirs.extend([
        "/opt/intel/openvino_2024/runtime/lib/intel64".into(),
        "/opt/intel/openvino_2023/runtime/lib/intel64".into(),
        "/opt/intel/openvino/runtime/lib/intel64".into(),
        "/usr/lib".into(),
        "/usr/local/lib".into(),
    ]);

    if let Ok(lp) = std::env::var("LD_LIBRARY_PATH") {
        dirs.extend(lp.split(':').map(std::path::PathBuf::from));
    }
    if let Ok(lp) = std::env::var("DYLD_LIBRARY_PATH") {
        dirs.extend(lp.split(':').map(std::path::PathBuf::from));
    }

    let found = dirs.iter().any(|d| d.join(LIB_NAME).exists());

    if found {
        info!("OpenVINO runtime found ({})", LIB_NAME);
        Ok(())
    } else {
        anyhow::bail!(
            "OpenVINO runtime not found ({LIB_NAME}). \
             Install OpenVINO from https://docs.openvino.ai, then source its \
             environment script before starting maranoded:\n\
             \tsource /opt/intel/openvino/setupvars.sh\n\
             Or set OPENVINO_DIR to the installation root."
        )
    }
}

#[cfg(feature = "ryzenai")]
fn validate_ryzenai_runtime() -> Result<()> {
    #[cfg(target_os = "linux")]
    const LIB_NAME: &str = "libaie_controller.so";
    #[cfg(target_os = "windows")]
    const LIB_NAME: &str = "aie_controller.dll";

    let mut dirs: Vec<std::path::PathBuf> = Vec::new();

    if let Ok(d) = std::env::var("RYZENAI_INSTALL_PATH") {
        dirs.push(std::path::PathBuf::from(&d).join("lib"));
        dirs.push(std::path::PathBuf::from(&d).join("bin"));
    }

    dirs.extend([
        "/opt/amd/ryzenai/lib".into(),
        "/opt/xilinx/xrt/lib".into(),
        "/usr/lib".into(),
        "/usr/local/lib".into(),
    ]);

    if let Ok(lp) = std::env::var("LD_LIBRARY_PATH") {
        dirs.extend(lp.split(':').map(std::path::PathBuf::from));
    }

    let found = dirs.iter().any(|d| d.join(LIB_NAME).exists());

    if found {
        info!("AMD Ryzen AI runtime found ({})", LIB_NAME);
        Ok(())
    } else {
        anyhow::bail!(
            "AMD Ryzen AI runtime not found ({LIB_NAME}). \
             Install the AMD Ryzen AI SDK from https://ryzenai.docs.amd.com, \
             then set RYZENAI_INSTALL_PATH to the installation root."
        )
    }
}

#[async_trait]
impl InferenceEngine for LlamaCppEngine {
    async fn generate(&self, req: InferenceRequest) -> Result<InferenceResponse> {
        let model = self.get_or_load(&req.model_path).await?;
        let backend = Arc::clone(&self.backend);
        let device = self.device;

        tokio::task::spawn_blocking(move || generate_sync(&backend, &model, req, device))
            .await
            .context("thread join for inference")?
    }

    async fn generate_stream(&self, req: InferenceRequest, tx: mpsc::Sender<Result<Token>>) {
        let model = match self.get_or_load(&req.model_path).await {
            Ok(m) => m,
            Err(e) => {
                let _ = tx.send(Err(e)).await;
                return;
            }
        };
        let backend = Arc::clone(&self.backend);
        let device = self.device;

        let _ =
            tokio::task::spawn_blocking(move || stream_sync(&backend, &model, req, device, &tx));
    }

    async fn embed(&self, model_path: &Path, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let model = self.get_or_load(model_path).await?;
        let backend = Arc::clone(&self.backend);
        let texts = texts.to_vec();

        tokio::task::spawn_blocking(move || embed_sync(&backend, &model, &texts))
            .await
            .context("thread join for embedding")?
    }

    fn device(&self) -> InferenceDevice {
        self.device
    }

    async fn load_model(&self, model_id: &str, path: &Path) -> Result<()> {
        self.get_or_load(path).await?;
        let key = path.to_string_lossy().into_owned();
        self.cache.lock().await.id_to_path.insert(model_id.to_string(), key);
        Ok(())
    }

    async fn unload_model(&self, model_id: &str) -> Result<()> {
        let mut cache = self.cache.lock().await;
        if let Some(path_key) = cache.id_to_path.remove(model_id) {
            cache.by_path.remove(&path_key);
            info!("unloaded model '{}' ({})", model_id, path_key);
        } else {
            warn!("unload_model('{}') — model not in cache", model_id);
        }
        Ok(())
    }
}

fn generate_sync(
    backend: &LlamaBackend,
    model: &LlamaModel,
    req: InferenceRequest,
    device: InferenceDevice,
) -> Result<InferenceResponse> {
    let start = Instant::now();
    let prompt = build_chatml_prompt(&req.messages);
    debug!("Prompt: {} chars", prompt.len());

    let n_ctx = MAX_N_CTX.min(model.n_ctx_train());

    let prompt_tokens = model
        .str_to_token(&prompt, AddBos::Always)
        .context("tokenising prompt")?;
    let n_prompt = prompt_tokens.len() as u32;

    if n_prompt >= n_ctx {
        anyhow::bail!("prompt ({n_prompt} tokens) ≥ context window ({n_ctx})");
    }

    let ctx_params = LlamaContextParams::default()
        .with_n_ctx(NonZeroU32::new(n_ctx))
        .with_n_batch(N_BATCH);
    let mut ctx = model
        .new_context(backend, ctx_params)
        .context("creating llama context")?;

    let n_prompt = n_prompt as i32;

    let mut batch = LlamaBatch::new(N_BATCH as usize, 1);
    decode_prompt(&mut ctx, &mut batch, &prompt_tokens)?;

    let mut sampler = build_sampler(req.temperature);
    let mut output = String::new();
    let mut n_cur = n_prompt;
    let mut finish_reason = FinishReason::Length;

    loop {
        let token = sampler.sample(&ctx, batch.n_tokens() - 1);
        sampler.accept(token);

        if model.is_eog_token(token) {
            finish_reason = FinishReason::Stop;
            break;
        }

        let piece = model
            .token_to_str(token, Special::Tokenize)
            .unwrap_or_default();
        output.push_str(&piece);

        if req
            .stop_sequences
            .iter()
            .any(|s| output.ends_with(s.as_str()))
        {
            finish_reason = FinishReason::Stop;
            break;
        }

        n_cur += 1;
        if n_cur - n_prompt >= req.max_tokens as i32 {
            break;
        }

        batch.clear();
        batch
            .add(token, n_cur - 1, &[0], true)
            .context("batch add")?;
        ctx.decode(&mut batch).context("decode step")?;
    }

    let tokens_in = n_prompt as u32;
    let tokens_out = (n_cur - n_prompt) as u32;
    let duration_ms = start.elapsed().as_millis() as u64;
    debug!(
        "Done: {}→{} tokens, {}ms",
        tokens_in, tokens_out, duration_ms
    );

    Ok(InferenceResponse {
        request_id: req.request_id,
        model: req.model,
        content: output,
        tokens_in,
        tokens_out,
        duration_ms,
        device,
        finish_reason,
    })
}

fn embed_sync(
    backend: &LlamaBackend,
    model: &LlamaModel,
    texts: &[String],
) -> Result<Vec<Vec<f32>>> {
    let n_ctx = MAX_N_CTX.min(model.n_ctx_train());
    let n_embd = model.n_embd() as usize;
    let mut out = Vec::with_capacity(texts.len());

    for text in texts {
        let mut tokens = model
            .str_to_token(text, AddBos::Always)
            .context("tokenising text for embedding")?;
        if tokens.len() > n_ctx as usize {
            tokens.truncate(n_ctx as usize);
        }
        if tokens.is_empty() {
            out.push(vec![0.0; n_embd]);
            continue;
        }
        let seq_len = tokens.len();

        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(n_ctx))
            .with_n_batch(seq_len as u32)
            .with_n_ubatch(seq_len as u32)
            .with_embeddings(true)
            .with_pooling_type(LlamaPoolingType::Unspecified);
        let mut ctx = model
            .new_context(backend, ctx_params)
            .context("creating embedding context")?;

        let mut batch = LlamaBatch::new(seq_len, 1);
        for (i, &tok) in tokens.iter().enumerate() {
            batch
                .add(tok, i as i32, &[0], true)
                .context("adding token to embedding batch")?;
        }

        ctx.encode(&mut batch).context("embedding encode")?;

        let embedding = ctx
            .embeddings_seq_ith(0)
            .context("reading pooled embedding")?;
        out.push(embedding.to_vec());
    }

    Ok(out)
}

fn stream_sync(
    backend: &LlamaBackend,
    model: &LlamaModel,
    req: InferenceRequest,
    device: InferenceDevice,
    tx: &mpsc::Sender<Result<Token>>,
) {
    let _ = device;
    let prompt = build_chatml_prompt(&req.messages);
    let n_ctx = MAX_N_CTX.min(model.n_ctx_train());

    let ctx_params = LlamaContextParams::default()
        .with_n_ctx(NonZeroU32::new(n_ctx))
        .with_n_batch(N_BATCH);

    let mut ctx = match model.new_context(backend, ctx_params) {
        Ok(c) => c,
        Err(e) => {
            let _ = tx.blocking_send(Err(anyhow::anyhow!("context creation: {}", e)));
            return;
        }
    };

    let prompt_tokens = match model.str_to_token(&prompt, AddBos::Always) {
        Ok(t) => t,
        Err(e) => {
            let _ = tx.blocking_send(Err(anyhow::anyhow!("tokenise: {}", e)));
            return;
        }
    };

    if prompt_tokens.len() as u32 >= n_ctx {
        let _ = tx.blocking_send(Err(anyhow::anyhow!(
            "prompt ({} tokens) ≥ context window ({})",
            prompt_tokens.len(),
            n_ctx
        )));
        return;
    }

    let n_prompt = prompt_tokens.len() as i32;
    let mut batch = LlamaBatch::new(N_BATCH as usize, 1);

    if let Err(e) = decode_prompt(&mut ctx, &mut batch, &prompt_tokens) {
        let _ = tx.blocking_send(Err(e));
        return;
    }

    let mut sampler = build_sampler(req.temperature);
    let mut n_cur = n_prompt;
    let mut emitted = String::new();

    loop {
        let token = sampler.sample(&ctx, batch.n_tokens() - 1);
        sampler.accept(token);

        let is_eog = model.is_eog_token(token);
        let n_new = n_cur - n_prompt + 1;

        let text = if is_eog {
            String::new()
        } else {
            model
                .token_to_str(token, Special::Tokenize)
                .unwrap_or_default()
        };

        // apply stop_sequences in streaming path same as in non-streaming path
        let hit_stop = if req.stop_sequences.is_empty() {
            false
        } else {
            emitted.push_str(&text);
            req.stop_sequences
                .iter()
                .any(|s| emitted.ends_with(s.as_str()))
        };

        let is_last = is_eog || hit_stop || n_new >= req.max_tokens as i32;

        if tx.blocking_send(Ok(Token { text, is_last })).is_err() {
            break; // client closed the stream channel
        }
        if is_last {
            break;
        }

        n_cur += 1;
        batch.clear();
        if batch.add(token, n_cur - 1, &[0], true).is_err() {
            break;
        }
        if let Err(e) = ctx.decode(&mut batch) {
            let _ = tx.blocking_send(Err(anyhow::anyhow!("decode step: {}", e)));
            break;
        }
    }
}

/// decode prompt in pieces of N_BATCH tokens.
///
/// last piece stays in `batch` (not cleared) so generation can call
/// `sampler.sample(&ctx, batch.n_tokens() - 1)` with the correct index of the last prompt token.
fn decode_prompt(
    ctx: &mut llama_cpp_2::context::LlamaContext<'_>,
    batch: &mut LlamaBatch,
    tokens: &[LlamaToken],
) -> Result<()> {
    let chunk_size = N_BATCH as usize;
    let n = tokens.len();
    let mut start = 0;
    while start < n {
        let end = (start + chunk_size).min(n);
        for (i, &tok) in tokens[start..end].iter().enumerate() {
            let pos = (start + i) as i32;
            let is_last = start + i == n - 1;
            batch
                .add(tok, pos, &[0], is_last)
                .context("adding token to prompt batch")?;
        }
        ctx.decode(batch).context("prompt decode")?;
        if end < n {
            batch.clear();
        }
        start = end;
    }
    Ok(())
}

/// build ChatML prompt string from message list
/// ```text
/// <|im_start|>system
/// you are a assistant.<|im_end|>
/// <|im_start|>user
/// what is 2+2?<|im_end|>
/// <|im_start|>assistant
/// ```
fn build_chatml_prompt(messages: &[ChatMessage]) -> String {
    let mut buf = String::with_capacity(512);
    for msg in messages {
        let role = match msg.role {
            ChatRole::System => "system",
            ChatRole::User => "user",
            ChatRole::Assistant => "assistant",
        };
        buf.push_str("<|im_start|>");
        buf.push_str(role);
        buf.push('\n');
        buf.push_str(&msg.content);
        buf.push_str("<|im_end|>\n");
    }
    buf.push_str("<|im_start|>assistant\n");
    buf
}

fn build_sampler(temperature: f32) -> LlamaSampler {
    if temperature < 1e-6 {
        LlamaSampler::chain_simple([LlamaSampler::greedy()])
    } else {
        LlamaSampler::chain_simple([
            LlamaSampler::temp(temperature),
            LlamaSampler::top_p(0.95, 1),
            LlamaSampler::min_p(0.05, 1),
            LlamaSampler::dist(42),
        ])
    }
}
