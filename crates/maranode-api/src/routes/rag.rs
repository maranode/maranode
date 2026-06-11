//! `/v1/rag/*` routes for RAG and document handling

use std::sync::Arc;

use anyhow::Result;
use axum::{
    extract::{Multipart, Path, State},
    http::HeaderMap,
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use maranode_common::events::AuditEvent;
use maranode_common::models::{ChatMessage, ChatRole};
use maranode_inference::types::InferenceRequest;
use maranode_rag::{extract::extract_text, RagEngine, SummarizeFn};

use crate::error::{ApiError, ApiResult};
use crate::state::{AppState, RagIngestPolicy};
use crate::user_ctx::authorize_privileged;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/rag/documents", post(ingest_document))
        .route("/v1/rag/documents/upload", post(upload_document))
        .route(
            "/v1/rag/documents/:id",
            get(get_document).delete(delete_document),
        )
        .route("/v1/rag/documents/:id/summary", get(get_document_summary))
        .route("/v1/rag/documents/:id/summarize", post(resummary_document))
        .route("/v1/rag/extract", post(extract_document))
        .route("/v1/rag/collections", get(list_collections))
        .route("/v1/rag/collections/:name", delete(delete_collection))
        .route("/v1/rag/collections/:name/documents", get(list_documents))
        .route("/v1/rag/search", post(search))
}

fn require_rag(state: &AppState) -> ApiResult<Arc<RagEngine>> {
    state
        .rag
        .clone()
        .ok_or_else(|| ApiError::not_implemented("RAG is not enabled (start daemon with --rag)"))
}

fn bearer_key(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
}

async fn require_rag_admin(headers: &HeaderMap, state: &AppState) -> ApiResult<()> {
    authorize_privileged(headers, state, |r| r.can_ingest_rag()).await
}

fn check_ingest_permission(headers: &HeaderMap, state: &AppState) -> ApiResult<()> {
    use maranode_common::secure::ct_eq_str;

    let runtime = state.rt();

    // anyone policy is public by design, no key
    if runtime.rag_ingest_policy == RagIngestPolicy::Anyone {
        return Ok(());
    }

    // admin-only and allowlist need auth.admin_key configured
    let Some(admin_key) = &runtime.admin_key else {
        return Err(ApiError::forbidden(
            "RAG ingest policy requires authentication, but no admin key is configured",
        ));
    };
    let key = bearer_key(headers);
    match &runtime.rag_ingest_policy {
        RagIngestPolicy::Anyone => Ok(()),
        RagIngestPolicy::AdminOnly => {
            if key.is_some_and(|k| ct_eq_str(k, admin_key)) {
                Ok(())
            } else {
                Err(ApiError::forbidden("RAG ingest requires the admin key"))
            }
        }
        RagIngestPolicy::Allowlist => {
            let is_admin = key.is_some_and(|k| ct_eq_str(k, admin_key));
            let in_list =
                key.is_some_and(|k| runtime.rag_ingest_allowlist.iter().any(|a| ct_eq_str(a, k)));
            if is_admin || in_list {
                Ok(())
            } else {
                Err(ApiError::forbidden("RAG ingest requires an authorized key"))
            }
        }
    }
}

/// make summarizer that uses inference engine
fn make_summarizer(state: &AppState) -> Option<Arc<dyn SummarizeFn>> {
    Some(Arc::new(EngineSummarizer {
        engine: state.engine.clone(),
        store: state.store.clone(),
    }))
}

struct EngineSummarizer {
    engine: Arc<dyn maranode_inference::engine::InferenceEngine>,
    store: maranode_store::ModelStore,
}

#[async_trait::async_trait]
impl SummarizeFn for EngineSummarizer {
    async fn summarize(&self, text: &str) -> Result<String> {
        let models = self.store.list().await?;
        let model = models
            .into_iter()
            .find(|m| matches!(m.model_type, maranode_common::models::ModelType::Llm))
            .ok_or_else(|| anyhow::anyhow!("no LLM model available for summarization"))?;

        let model_path = self.store.blob_path_verified(&model.model_id).await?;

        let prompt = format!(
            "Summarize the following document in 3-5 sentences. \
             Focus on the main topic, key facts, and any important conclusions. \
             Be concise.\n\nDOCUMENT:\n{}\n\nSUMMARY:",
            text
        );
        let req = InferenceRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            model: model.model_id,
            model_path,
            messages: vec![ChatMessage {
                role: ChatRole::User,
                content: prompt,
            }],
            max_tokens: 256,
            temperature: 0.3,
            stop_sequences: vec![],
            stream: false,
        };
        let resp = self.engine.generate(req).await?;
        Ok(resp.content.trim().to_string())
    }
}

#[derive(Debug, Serialize)]
struct IngestResponse {
    document_id: String,
    collection: String,
    chunks: usize,
    pages: u32,
    summary: Option<String>,
}

#[derive(Debug, Serialize)]
struct DocumentView {
    id: String,
    source: String,
    sha256: String,
    chunks: usize,
    ingested_at: String,
    title: Option<String>,
    author: Option<String>,
    page_count: u32,
    summary: Option<String>,
}

impl From<maranode_rag::DocumentInfo> for DocumentView {
    fn from(d: maranode_rag::DocumentInfo) -> Self {
        Self {
            id: d.id,
            source: d.source,
            sha256: d.sha256,
            chunks: d.chunks,
            ingested_at: d.ingested_at,
            title: d.title,
            author: d.author,
            page_count: d.page_count,
            summary: d.summary,
        }
    }
}

#[derive(Debug, Deserialize)]
struct IngestRequest {
    collection: Option<String>,
    source: String,
    text: String,
}

async fn ingest_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<IngestRequest>,
) -> ApiResult<Json<IngestResponse>> {
    check_ingest_permission(&headers, &state)?;
    let rag = require_rag(&state)?;
    let collection = req
        .collection
        .unwrap_or_else(|| rag.default_collection().to_string());

    if req.text.trim().is_empty() {
        return Err(ApiError::bad_request("`text` must not be empty"));
    }

    let stats = rag
        .ingest(&collection, &req.source, &req.text)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let _ = state
        .audit
        .append(
            "api",
            AuditEvent::RagDocumentIngested {
                collection: stats.collection.clone(),
                source: req.source.clone(),
                chunks: stats.chunks,
            },
        )
        .await;

    Ok(Json(IngestResponse {
        document_id: stats.document_id,
        collection: stats.collection,
        chunks: stats.chunks,
        pages: stats.pages,
        summary: stats.summary,
    }))
}

async fn upload_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> ApiResult<Json<IngestResponse>> {
    check_ingest_permission(&headers, &state)?;
    let rag = require_rag(&state)?;

    let mut file_bytes: Option<Vec<u8>> = None;
    let mut filename: Option<String> = None;
    let mut collection: Option<String> = None;
    let mut source: Option<String> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::bad_request(format!("multipart error: {e}")))?
    {
        match field.name() {
            Some("file") => {
                filename = field.file_name().map(str::to_string);
                file_bytes = Some(
                    field
                        .bytes()
                        .await
                        .map_err(|e| ApiError::bad_request(format!("reading file: {e}")))?
                        .to_vec(),
                );
            }
            Some("collection") => {
                collection = Some(
                    field
                        .text()
                        .await
                        .map_err(|e| ApiError::bad_request(format!("reading collection: {e}")))?,
                );
            }
            Some("source") => {
                source = Some(
                    field
                        .text()
                        .await
                        .map_err(|e| ApiError::bad_request(format!("reading source: {e}")))?,
                );
            }
            _ => {}
        }
    }

    let bytes = file_bytes.ok_or_else(|| ApiError::bad_request("missing `file` field"))?;
    let fname = filename.unwrap_or_else(|| "upload".into());
    let source = source.unwrap_or_else(|| fname.clone());
    let collection = collection.unwrap_or_else(|| rag.default_collection().to_string());

    if bytes.is_empty() {
        return Err(ApiError::bad_request("uploaded file is empty"));
    }

    let summarizer = make_summarizer(&state);

    let stats = rag
        .ingest_bytes(&collection, &source, &bytes, &fname, summarizer.as_deref())
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;

    let _ = state
        .audit
        .append(
            "api",
            AuditEvent::RagDocumentIngested {
                collection: stats.collection.clone(),
                source: source.clone(),
                chunks: stats.chunks,
            },
        )
        .await;

    Ok(Json(IngestResponse {
        document_id: stats.document_id,
        collection: stats.collection,
        chunks: stats.chunks,
        pages: stats.pages,
        summary: stats.summary,
    }))
}

#[derive(Debug, Serialize)]
struct ExtractResponse {
    filename: String,
    chars: usize,
    text: String,
}

async fn extract_document(mut multipart: Multipart) -> ApiResult<Json<ExtractResponse>> {
    let mut file_bytes: Option<Vec<u8>> = None;
    let mut filename: Option<String> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::bad_request(format!("multipart error: {e}")))?
    {
        if field.name() == Some("file") {
            filename = field.file_name().map(str::to_string);
            file_bytes = Some(
                field
                    .bytes()
                    .await
                    .map_err(|e| ApiError::bad_request(format!("reading file: {e}")))?
                    .to_vec(),
            );
        }
    }

    let bytes = file_bytes.ok_or_else(|| ApiError::bad_request("missing `file` field"))?;
    let fname = filename.unwrap_or_else(|| "upload".into());
    if bytes.is_empty() {
        return Err(ApiError::bad_request("uploaded file is empty"));
    }

    let text = extract_text(&bytes, &fname).map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok(Json(ExtractResponse {
        chars: text.chars().count(),
        filename: fname,
        text,
    }))
}

async fn get_document(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<DocumentView>> {
    let rag = require_rag(&state)?;
    let doc = rag
        .get_document(&id)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("document not found"))?;
    Ok(Json(DocumentView::from(doc)))
}

#[derive(Serialize)]
struct SummaryResp {
    document_id: String,
    summary: Option<String>,
}

async fn get_document_summary(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<SummaryResp>> {
    let rag = require_rag(&state)?;
    let doc = rag
        .get_document(&id)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("document not found"))?;
    Ok(Json(SummaryResp {
        document_id: id,
        summary: doc.summary,
    }))
}

async fn resummary_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<SummaryResp>> {
    check_ingest_permission(&headers, &state)?;
    let rag = require_rag(&state)?;

    rag.get_document(&id)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("document not found"))?;

    let text = rag
        .get_document_text(&id)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("document has no stored chunks"))?;

    let summarizer = make_summarizer(&state)
        .ok_or_else(|| ApiError::not_implemented("no inference engine available for summarization"))?;

    let summary = summarizer
        .summarize(&text)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    rag.set_summary(&id, &summary)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    Ok(Json(SummaryResp {
        document_id: id,
        summary: Some(summary),
    }))
}

#[derive(Debug, Serialize)]
struct CollectionView {
    name: String,
    embedding_model: String,
    dim: usize,
    documents: usize,
    chunks: usize,
}

async fn list_collections(State(state): State<AppState>) -> ApiResult<Json<Vec<CollectionView>>> {
    let rag = require_rag(&state)?;
    Ok(Json(
        rag.list_collections()
            .map_err(|e| ApiError::internal(e.to_string()))?
            .into_iter()
            .map(|c| CollectionView {
                name: c.name,
                embedding_model: c.embedding_model,
                dim: c.dim,
                documents: c.documents,
                chunks: c.chunks,
            })
            .collect(),
    ))
}

async fn delete_collection(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    require_rag_admin(&headers, &state).await?;
    let rag = require_rag(&state)?;
    if rag
        .delete_collection(&name)
        .map_err(|e| ApiError::internal(e.to_string()))?
    {
        Ok(Json(serde_json::json!({ "deleted": name })))
    } else {
        Err(ApiError::not_found(format!(
            "collection '{}' not found",
            name
        )))
    }
}

async fn list_documents(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<Json<Vec<DocumentView>>> {
    let rag = require_rag(&state)?;
    Ok(Json(
        rag.list_documents(&name)
            .map_err(|e| ApiError::internal(e.to_string()))?
            .into_iter()
            .map(DocumentView::from)
            .collect(),
    ))
}

async fn delete_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    require_rag_admin(&headers, &state).await?;
    let rag = require_rag(&state)?;
    if rag
        .delete_document(&id)
        .map_err(|e| ApiError::internal(e.to_string()))?
    {
        Ok(Json(serde_json::json!({ "deleted": id })))
    } else {
        Err(ApiError::not_found(format!("document '{}' not found", id)))
    }
}

#[derive(Debug, Deserialize)]
struct SearchRequest {
    collection: Option<String>,
    query: String,
    top_k: Option<usize>,
}

#[derive(Debug, Serialize)]
struct SearchHit {
    source: String,
    ordinal: usize,
    score: f32,
    text: String,
    page_number: u32,
    section: Option<String>,
}

async fn search(
    State(state): State<AppState>,
    Json(req): Json<SearchRequest>,
) -> ApiResult<Json<Vec<SearchHit>>> {
    let rag = require_rag(&state)?;
    let collection = req
        .collection
        .unwrap_or_else(|| rag.default_collection().to_string());

    if req.query.trim().is_empty() {
        return Err(ApiError::bad_request("`query` must not be empty"));
    }

    let hits = rag
        .retrieve(&collection, &req.query, req.top_k)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let query_sha256 = hex::encode(Sha256::digest(req.query.as_bytes()));
    let _ = state
        .audit
        .append(
            "api",
            AuditEvent::RagRetrieval {
                collection,
                query_sha256,
                hits: hits.len(),
            },
        )
        .await;

    Ok(Json(
        hits.into_iter()
            .map(|h| SearchHit {
                source: h.source,
                ordinal: h.ordinal,
                score: h.score,
                text: h.text,
                page_number: h.page_number,
                section: h.section,
            })
            .collect(),
    ))
}
