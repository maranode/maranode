use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Instant;

use axum::{
    extract::State,
    response::{
        sse::{Event, Sse},
        IntoResponse, Response,
    },
    routing::post,
    Json, Router,
};
use chrono::Utc;
use sha2::{Digest, Sha256};
use tokio::sync::{mpsc, Mutex};
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;
use uuid::Uuid;

use maranode_audit::AuditLog;
use maranode_common::events::AuditEvent;
use maranode_common::models::{ChatMessage, ChatRole, ModelId};
use maranode_inference::types::InferenceRequest;

use crate::error::{ApiError, ApiResult};
use crate::openai::{
    ChatChoice, ChatCompletionChunk, ChatCompletionRequest, ChatCompletionResponse, ChunkChoice,
    ChunkDelta, OaiMessage, RagSource, UsageInfo,
};
use crate::state::{AppState, WorkspaceUsage};
use crate::workspace_ctx::WorkspaceCtx;

struct UsageGuard {
    usage: Arc<Mutex<HashMap<String, WorkspaceUsage>>>,
    slug: String,
    model_key: String,
}

impl Drop for UsageGuard {
    fn drop(&mut self) {
        let usage = Arc::clone(&self.usage);
        let slug = self.slug.clone();
        let model_key = self.model_key.clone();
        tokio::spawn(async move {
            usage.lock().await
                .entry(slug)
                .or_default()
                .release(&model_key);
        });
    }
}

pub fn router() -> Router<AppState> {
    Router::new().route("/v1/chat/completions", post(chat_completions))
}

async fn chat_completions(
    State(state): State<AppState>,
    workspace: WorkspaceCtx,
    Json(req): Json<ChatCompletionRequest>,
) -> Response {
    match run(state, workspace, req).await {
        Ok(r) => r,
        Err(e) => e.into_response(),
    }
}

const MAX_MESSAGES: usize = 512;

async fn run(
    state: AppState,
    workspace: WorkspaceCtx,
    req: ChatCompletionRequest,
) -> ApiResult<Response> {
    let ws = workspace.workspace();
    let request_id = Uuid::new_v4().to_string();

    if req.messages.is_empty() {
        return Err(ApiError::bad_request("`messages` must not be empty"));
    }
    if req.messages.len() > MAX_MESSAGES {
        return Err(ApiError::bad_request(format!(
            "too many messages ({}); maximum is {}",
            req.messages.len(),
            MAX_MESSAGES
        )));
    }

    let model_id = ModelId::parse(&req.model).ok_or_else(|| {
        ApiError::bad_request(format!(
            "invalid model identifier '{}': expected <name>:<tag>",
            req.model
        ))
    })?;

    if !ws.allows_model(&req.model) {
        return Err(ApiError::forbidden(format!(
            "model '{}' is not in the allowlist for workspace '{}'",
            req.model, ws.slug
        )));
    }

    let ws_audit = state.workspace_audits.lock().await.get(&ws.slug).cloned();
    let audit_log: &AuditLog = ws_audit.as_ref().unwrap_or(&state.audit);

    if let Some(rpm) = ws.rate_limit_rpm {
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let mut limiter = state.rate_limiter.lock().await;
        if limiter.len() > 1024 {
            limiter.retain(|_, (_, start)| now_secs.saturating_sub(*start) < 120);
        }
        let entry = limiter.entry(ws.slug.clone()).or_insert((0, now_secs));
        if now_secs.saturating_sub(entry.1) >= 60 {
            *entry = (1, now_secs);
        } else {
            entry.0 = entry.0.saturating_add(1);
            if entry.0 > rpm {
                return Err(ApiError::service_unavailable(format!(
                    "workspace '{}' rate limit of {} rpm exceeded",
                    ws.slug, rpm
                )));
            }
        }
    }

    let manifest = state
        .store
        .get(&model_id)
        .await
        .map_err(|e| ApiError::not_found(e.to_string()))?
        .ok_or_else(|| ApiError::not_found(format!("model '{}' not found", model_id)))?;
    let model_path = std::path::PathBuf::from(&manifest.blob_path);
    let model_size_bytes = manifest.size_bytes;
    let model_key = model_id.to_string();

    {
        let mut usage_map = state.workspace_usage.lock().await;
        let entry = usage_map.entry(ws.slug.clone()).or_default();

        if let Some(max) = ws.max_concurrent_requests {
            if entry.concurrent >= max {
                return Err(ApiError::service_unavailable(format!(
                    "workspace '{}' concurrent request limit of {} exceeded",
                    ws.slug, max
                )));
            }
        }

        if let Some(max) = ws.max_models {
            let is_new = !entry.active_models.contains_key(&model_key);
            if is_new && entry.model_count() >= max {
                return Err(ApiError::service_unavailable(format!(
                    "workspace '{}' simultaneous model limit of {} exceeded",
                    ws.slug, max
                )));
            }
        }

        if let Some(max) = ws.max_memory_bytes {
            let is_new = !entry.active_models.contains_key(&model_key);
            if is_new && entry.memory_bytes().saturating_add(model_size_bytes) > max {
                return Err(ApiError::service_unavailable(format!(
                    "workspace '{}' memory quota of {} bytes would be exceeded",
                    ws.slug, max
                )));
            }
        }

        entry.acquire(&model_key, model_size_bytes);
    }

    let _usage_guard = UsageGuard {
        usage: Arc::clone(&state.workspace_usage),
        slug: ws.slug.clone(),
        model_key: model_key.clone(),
    };

    let mut messages: Vec<ChatMessage> = req
        .messages
        .iter()
        .map(|m| ChatMessage {
            role: match m.role.as_str() {
                "system" => ChatRole::System,
                "assistant" => ChatRole::Assistant,
                _ => ChatRole::User,
            },
            content: m.content.clone(),
        })
        .collect();

    let effective_rag = req.rag.as_ref();

    let mut rag_sources: Option<Vec<RagSource>> = None;
    if let Some(rag_opts) = effective_rag {
        let rag = state.rag.clone().ok_or_else(|| {
            ApiError::not_implemented(
                "RAG was requested but is not enabled on this server (start the daemon with --rag)",
            )
        })?;

        let query = req
            .messages
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .map(|m| m.content.clone())
            .ok_or_else(|| ApiError::bad_request("RAG requested but no user message to ground"))?;

        let (retrieved, audit_collection) = match &rag_opts.collection {
            Some(col) => {
                let hits = rag
                    .retrieve(col, &query, rag_opts.top_k)
                    .await
                    .map_err(|e| ApiError::internal(e.to_string()))?;
                (hits, col.clone())
            }
            None => {
                let hits = rag
                    .retrieve_all_collections(&query, rag_opts.top_k)
                    .await
                    .map_err(|e| ApiError::internal(e.to_string()))?;
                (hits, "*".to_string())
            }
        };

        let query_sha256 = hex::encode(Sha256::digest(query.as_bytes()));
        let _ = audit_log
            .append(
                "api",
                AuditEvent::RagRetrieval {
                    collection: audit_collection,
                    query_sha256,
                    hits: retrieved.len(),
                },
            )
            .await;

        match rag.build_context_prompt(&retrieved) {
            Some(context_prompt) => {
                rag_sources = Some(
                    retrieved
                        .iter()
                        .enumerate()
                        .map(|(i, c)| RagSource {
                            index: i + 1,
                            source: c.source.clone(),
                            score: c.score,
                            text: c.text.clone(),
                            title: c.title.clone(),
                            author: c.author.clone(),
                            page_number: c.page_number,
                        })
                        .collect(),
                );
                messages.insert(
                    0,
                    ChatMessage {
                        role: ChatRole::System,
                        content: context_prompt,
                    },
                );
            }
            None if rag_opts.require_context => {
                let msg = "This information is not in the provided documents.";
                if req.stream {
                    let id = format!("chatcmpl-{}", request_id);
                    let created = Utc::now().timestamp();
                    let model = req.model.clone();
                    let first = make_chunk_event(&id, &model, created, Some("assistant"), Some(""), None, None);
                    let content_ev = make_chunk_event(&id, &model, created, None, Some(msg), None, None);
                    let stop_ev = make_chunk_event(&id, &model, created, None, None, Some("stop"), Some(Vec::new()));
                    let done_ev = Event::default().data("[DONE]");
                    let stream = tokio_stream::iter([
                        Ok::<Event, Infallible>(first),
                        Ok(content_ev),
                        Ok(stop_ev),
                        Ok(done_ev),
                    ]);
                    return Ok(Sse::new(stream).into_response());
                }
                let resp = ChatCompletionResponse {
                    id: format!("chatcmpl-{}", request_id),
                    object: "chat.completion",
                    created: Utc::now().timestamp(),
                    model: req.model,
                    choices: vec![ChatChoice {
                        index: 0,
                        message: OaiMessage {
                            role: "assistant".into(),
                            content: msg.into(),
                        },
                        finish_reason: "stop".into(),
                    }],
                    usage: UsageInfo {
                        prompt_tokens: 0,
                        completion_tokens: 0,
                        total_tokens: 0,
                    },
                    sources: Some(Vec::new()),
                };
                return Ok(Json(resp).into_response());
            }
            None => {}
        }
    }

    let effective_system_prompt = ws
        .system_prompt
        .clone()
        .or_else(|| state.rt().system_prompt.clone());
    if let Some(sp) = effective_system_prompt {
        messages.insert(
            0,
            ChatMessage {
                role: ChatRole::System,
                content: sp,
            },
        );
    }

    let mut hasher = Sha256::new();
    for msg in &messages {
        hasher.update(format!("{:?}", msg.role).as_bytes());
        hasher.update(msg.content.as_bytes());
    }
    let prompt_sha256 = hex::encode(hasher.finalize());

    let engine_device = state.engine.device();
    let _ = audit_log
        .append(
            "api",
            AuditEvent::InferenceStart {
                request_id: request_id.clone(),
                model: model_id.clone(),
                prompt_sha256,
                device: engine_device,
            },
        )
        .await;

    // llama.cpp fails when prompt is longer than n_ctx; cut system message as temporary fix
    const MAX_PROMPT_CHARS: usize = 6000;
    let total_chars: usize = messages.iter().map(|m| m.content.len()).sum();
    if total_chars > MAX_PROMPT_CHARS {
        let rest_chars: usize = messages.iter().skip(1).map(|m| m.content.len()).sum();
        let budget = MAX_PROMPT_CHARS.saturating_sub(rest_chars).max(200);
        if let Some(sys) = messages.first_mut() {
            if sys.role == ChatRole::System && sys.content.len() > budget {
                sys.content = sys.content.chars().take(budget).collect();
                sys.content.push_str("\n\n[context truncated]");
            }
        }
    }

    let inference_req = InferenceRequest {
        request_id: request_id.clone(),
        model: model_id.clone(),
        model_path,
        messages,
        temperature: req.temperature,
        max_tokens: req.max_tokens,
        stop_sequences: req.stop.unwrap_or_default(),
        stream: req.stream,
    };

    if req.stream {
        let (tx, rx) = mpsc::channel::<Result<maranode_inference::types::Token, anyhow::Error>>(128);
        state.engine.generate_stream(inference_req, tx).await;

        let id = format!("chatcmpl-{}", request_id);
        let model = req.model.clone();
        let created = Utc::now().timestamp();

        let first = make_chunk_event(&id, &model, created, Some("assistant"), Some(""), None, None);

        
        let guard_hold = Some(_usage_guard);
        let stream_sources = rag_sources.clone();

        let stream = tokio_stream::once(Ok::<Event, Infallible>(first))
            .chain(ReceiverStream::new(rx).map(move |result| {
                let id = id.clone();
                let model = model.clone();
                // keep guard_hold in closure so usage guard lives until stream done
                let _ = guard_hold.as_ref();
                match result {
                    Ok(token) => {
                        let finish = token.is_last.then(|| "stop".to_string());
                        let sources = if token.is_last { stream_sources.clone() } else { None };
                        let content = if token.is_last {
                            None
                        } else {
                            Some(token.text.as_str())
                        };
                        Ok::<Event, Infallible>(make_chunk_event(
                            &id,
                            &model,
                            created,
                            None,
                            content,
                            finish.as_deref(),
                            sources,
                        ))
                    }
                    Err(e) => {
                        tracing::error!(detail = %e, "streaming inference error");
                        Ok(Event::default().data("[ERROR] inference failed"))
                    }
                }
            }))
            .chain(tokio_stream::once(Ok(Event::default().data("[DONE]"))));

        return Ok(Sse::new(stream).into_response());
    }

    let start = Instant::now();
    let resp = state.engine.generate(inference_req).await.map_err(|e| {
        let api_err = if e.to_string().starts_with("server busy") {
            ApiError::service_unavailable(e.to_string())
        } else {
            ApiError::internal(e.to_string())
        };
        state.stats.record_error();
        let _ = tokio::spawn({
            let audit = audit_log.clone();
            let rid = request_id.clone();
            let mid = model_id.clone();
            let err = e.to_string();
            async move {
                let _ = audit
                    .append(
                        "api",
                        AuditEvent::InferenceFailed {
                            request_id: rid,
                            model: mid,
                            reason: err,
                        },
                    )
                    .await;
            }
        });
        api_err
    })?;

    let duration_ms = start.elapsed().as_millis() as u64;
    state
        .stats
        .record_ok(resp.tokens_in, resp.tokens_out, duration_ms);
    let _ = audit_log
        .append(
            "api",
            AuditEvent::InferenceComplete {
                request_id: request_id.clone(),
                model: model_id.clone(),
                tokens_in: resp.tokens_in,
                tokens_out: resp.tokens_out,
                duration_ms,
                device: resp.device,
            },
        )
        .await;

    Ok(Json(ChatCompletionResponse {
        id: format!("chatcmpl-{}", request_id),
        object: "chat.completion",
        created: Utc::now().timestamp(),
        model: req.model,
        choices: vec![ChatChoice {
            index: 0,
            message: OaiMessage {
                role: "assistant".into(),
                content: resp.content,
            },
            finish_reason: "stop".into(),
        }],
        usage: UsageInfo {
            prompt_tokens: resp.tokens_in,
            completion_tokens: resp.tokens_out,
            total_tokens: resp.tokens_in.saturating_add(resp.tokens_out),
        },
        sources: rag_sources,
    })
    .into_response())
}

fn make_chunk_event(
    id: &str,
    model: &str,
    created: i64,
    role: Option<&str>,
    content: Option<&str>,
    finish_reason: Option<&str>,
    sources: Option<Vec<crate::openai::RagSource>>,
) -> Event {
    let chunk = ChatCompletionChunk {
        id: id.to_string(),
        object: "chat.completion.chunk",
        created,
        model: model.to_string(),
        choices: vec![ChunkChoice {
            index: 0,
            delta: ChunkDelta {
                role: role.map(str::to_string),
                content: content.map(str::to_string),
            },
            finish_reason: finish_reason.map(str::to_string),
        }],
        sources,
    };
    Event::default().data(serde_json::to_string(&chunk).unwrap_or_default())
}
