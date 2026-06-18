use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use maranode_common::events::AuditEvent;
use maranode_common::user::Permission;

use crate::dlp::{self, DlpConfig, ForcepointCfg, PurviewCfg, SymantecCfg};
use crate::error::ApiError;
use crate::state::AppState;
use crate::user_ctx::UserCtx;

#[derive(Debug, Deserialize)]
pub struct SyncRequest {
    pub provider: String,
    // optional per-request override creds; falls back to AppState.dlp config
    pub purview: Option<PurviewCfg>,
    pub forcepoint: Option<ForcepointCfg>,
    pub symantec: Option<SymantecCfg>,
}

#[derive(Debug, Serialize)]
pub struct SyncResponse {
    pub provider: String,
    pub labels_imported: usize,
    pub collections: Vec<CollectionImport>,
}

#[derive(Debug, Serialize)]
pub struct CollectionImport {
    pub collection: String,
    pub label: String,
}

async fn dlp_sync(
    State(state): State<AppState>,
    user: UserCtx,
    Json(req): Json<SyncRequest>,
) -> Result<Json<SyncResponse>, ApiError> {
    user.require(Permission::DlpManage)?;
    let dlp_cfg = state.dlp.as_ref();

    let imported = match req.provider.to_lowercase().as_str() {
        "purview" => {
            let cfg = req
                .purview
                .as_ref()
                .or(dlp_cfg.purview.as_ref())
                .ok_or_else(|| ApiError::bad_request("purview config not found"))?;
            dlp::purview::sync(cfg)
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?
        }
        "forcepoint" => {
            let cfg = req
                .forcepoint
                .as_ref()
                .or(dlp_cfg.forcepoint.as_ref())
                .ok_or_else(|| ApiError::bad_request("forcepoint config not found"))?;
            dlp::forcepoint::sync(cfg)
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?
        }
        "symantec" => {
            let cfg = req
                .symantec
                .as_ref()
                .or(dlp_cfg.symantec.as_ref())
                .ok_or_else(|| ApiError::bad_request("symantec config not found"))?;
            dlp::symantec::sync(cfg)
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?
        }
        other => return Err(ApiError::bad_request(format!("unknown DLP provider: {other}"))),
    };

    // apply imported labels into live classification policy
    {
        let mut policy = state.classification.write().await;
        for item in &imported {
            policy.set_collection_label(&item.collection, item.label, true);
        }
        let _ = policy.save(&state.data_dir);
    }

    let count = imported.len();
    let collections: Vec<CollectionImport> = imported
        .iter()
        .map(|i| CollectionImport {
            collection: i.collection.clone(),
            label: format!("{:?}", i.label),
        })
        .collect();

    let _ = state
        .audit
        .append(
            "dlp",
            AuditEvent::DlpSyncCompleted {
                provider: req.provider.clone(),
                labels_imported: count,
            },
        )
        .await;

    Ok(Json(SyncResponse {
        provider: req.provider,
        labels_imported: count,
        collections,
    }))
}

pub fn router() -> Router<AppState> {
    Router::new().route("/v1/dlp/sync", post(dlp_sync))
}
