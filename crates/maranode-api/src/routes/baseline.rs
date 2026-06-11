use axum::{extract::State, routing::post, Json, Router};
use serde::{Deserialize, Serialize};

use maranode_common::baseline::{output_sha256, Baseline};
use maranode_common::events::AuditEvent;
use maranode_common::models::{ChatMessage, ChatRole, ModelId};
use maranode_inference::InferenceRequest;

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/v1/baseline/check", post(check_baseline))
}

#[derive(Debug, Deserialize)]
struct CheckRequest {
    model: String,
    baseline: Option<Baseline>,
}

#[derive(Debug, Serialize)]
struct CheckResponse {
    model: String,
    vectors_run: usize,
    vectors_passed: usize,
    vectors_failed: usize,
    ok: bool,
}

async fn check_baseline(
    State(state): State<AppState>,
    Json(req): Json<CheckRequest>,
) -> ApiResult<Json<CheckResponse>> {
    let model_id = ModelId::parse(&req.model)
        .ok_or_else(|| ApiError::bad_request(format!("invalid model id '{}'", req.model)))?;

    let manifest = state
        .store
        .get(&model_id)
        .await
        .map_err(|e| ApiError::not_found(e.to_string()))?
        .ok_or_else(|| ApiError::not_found(format!("model '{}' not found", model_id)))?;

    let baseline = match req.baseline {
        Some(b) => b,
        None => {
            let baselines_dir = state.data_dir.join("baselines");
            let path = baselines_dir.join(format!("{}.mrn-baseline", manifest.sha256));
            if !path.exists() {
                return Err(ApiError::not_found(format!(
                    "no baseline found for model '{}' (sha256={}…)",
                    model_id,
                    &manifest.sha256[..12]
                )));
            }
            Baseline::load(&path).map_err(|e| ApiError::bad_request(e.to_string()))?
        }
    };

    baseline
        .verify()
        .map_err(|e| ApiError::bad_request(format!("baseline signature invalid: {e}")))?;

    let model_path = std::path::PathBuf::from(&manifest.blob_path);
    let mut passed = 0usize;
    let mut failed = 0usize;

    for (i, vec) in baseline.vectors.iter().enumerate() {
        let inference_req = InferenceRequest {
            request_id: format!("baseline-{}-{}", model_id, i),
            model: model_id.clone(),
            model_path: model_path.clone(),
            messages: vec![ChatMessage {
                role: ChatRole::User,
                content: vec.prompt.clone(),
            }],
            temperature: vec.temperature,
            max_tokens: vec.max_tokens,
            stop_sequences: vec![],
            stream: false,
            seed: Some(vec.seed),
            deterministic: true,
        };

        match state.engine.generate(inference_req).await {
            Ok(resp) => {
                let got = output_sha256(&resp.content);
                if got == vec.expected_sha256 {
                    passed += 1;
                } else {
                    failed += 1;
                }
            }
            Err(_) => {
                failed += 1;
            }
        }
    }

    let ok = failed <= baseline.max_mismatches;

    let _ = state
        .audit
        .append(
            "api",
            AuditEvent::ModelBaselineChecked {
                model_id: model_id.to_string(),
                model_sha256: manifest.sha256.clone(),
                vectors_run: baseline.vectors.len(),
                vectors_passed: passed,
                vectors_failed: failed,
                baseline_signer: baseline.signer_pubkey.clone(),
            },
        )
        .await;

    if !ok {
        let _ = state
            .audit
            .append(
                "api",
                AuditEvent::ModelDriftDetected {
                    model_id: model_id.to_string(),
                    model_sha256: manifest.sha256,
                    vectors_failed: failed,
                    action_taken: "api_check".into(),
                },
            )
            .await;
    }

    Ok(Json(CheckResponse {
        model: req.model,
        vectors_run: baseline.vectors.len(),
        vectors_passed: passed,
        vectors_failed: failed,
        ok,
    }))
}
