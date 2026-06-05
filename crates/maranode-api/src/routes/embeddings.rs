use axum::{extract::State, routing::post, Json, Router};

use maranode_common::models::ModelId;

use crate::error::{ApiError, ApiResult};
use crate::openai::{EmbeddingData, EmbeddingRequest, EmbeddingResponse, EmbeddingUsage};
use crate::state::AppState;
use crate::workspace_ctx::WorkspaceCtx;

const MAX_EMBEDDING_BATCH: usize = 512;

pub fn router() -> Router<AppState> {
    Router::new().route("/v1/embeddings", post(embeddings))
}

async fn embeddings(
    State(state): State<AppState>,
    workspace: WorkspaceCtx,
    Json(req): Json<EmbeddingRequest>,
) -> ApiResult<Json<EmbeddingResponse>> {
    let ws = workspace.workspace();

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

    let model_path = state
        .store
        .blob_path_verified(&model_id)
        .await
        .map_err(|e| ApiError::not_found(e.to_string()))?;

    let inputs = req.input.into_vec();
    if inputs.is_empty() {
        return Err(ApiError::bad_request("`input` must not be empty"));
    }
    if inputs.len() > MAX_EMBEDDING_BATCH {
        return Err(ApiError::bad_request(format!(
            "too many inputs ({}); maximum is {}",
            inputs.len(),
            MAX_EMBEDDING_BATCH
        )));
    }
    let total_chars: usize = inputs.iter().map(|s| s.len()).sum();

    let vectors = state
        .engine
        .embed(&model_path, &inputs)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let data = vectors
        .into_iter()
        .enumerate()
        .map(|(index, embedding)| EmbeddingData {
            object: "embedding",
            index,
            embedding,
        })
        .collect();

    let approx_tokens = (total_chars / 4) as u32;

    Ok(Json(EmbeddingResponse {
        object: "list",
        data,
        model: req.model,
        usage: EmbeddingUsage {
            prompt_tokens: approx_tokens,
            total_tokens: approx_tokens,
        },
    }))
}
