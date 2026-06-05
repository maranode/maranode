//! GET /v1/attestation/report
//!
//! Returns live attestation report: binary SHA-256, TPM PCR values (if available),
//! and audit log chain status. Report body is HMAC-SHA256 signed with instance
//! audit key so external party who has that key can check report is real.
//!
//! Optional `?nonce=<hex>` query is included in signed payload so old report
//! cannot be replayed. Verifier sends random nonce and checks same nonce in response.
//! Endpoint needs no auth. It is for callers who do not trust the operator.

use axum::{extract::{Query, State}, routing::get, Json, Router};
use serde::{Deserialize, Serialize};

use maranode_attestation::report::AttestationReport;
use maranode_audit::log::{default_key_path, default_log_path};

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/v1/attestation/report", get(get_report))
}

#[derive(Deserialize)]
struct ReportQuery {
    /// nonce from caller (hex string), mixed into HMAC so response stays fresh
    nonce: Option<String>,
}

#[derive(Serialize)]
struct SignedReport {
    #[serde(flatten)]
    report: AttestationReport,
    nonce: String,
    /// HMAC-SHA256(audit_key, canonical_json || nonce)
    hmac: String,
    signed: bool,
}

async fn get_report(
    State(state): State<AppState>,
    Query(q): Query<ReportQuery>,
) -> ApiResult<Json<SignedReport>> {
    let log_path = default_log_path(&state.data_dir);
    let key_path = default_key_path(&state.data_dir);

    let log_opt = log_path.exists().then_some(log_path.as_path());
    let key_opt = key_path.exists().then_some(key_path.as_path());

    let report = AttestationReport::generate(log_opt, key_opt)
        .map_err(|e| ApiError::internal(format!("attestation failed: {e}")))?;

    let nonce = q.nonce.unwrap_or_default();

    let (hmac, signed) = match maranode_audit::key::load(&key_path) {
        Ok(key) => {
            // signed bytes. full report json then nonce string
            let report_json = serde_json::to_string(&report)
                .map_err(|e| ApiError::internal(e.to_string()))?;
            let payload = format!("{}{}", report_json, nonce);
            let h = maranode_audit::chain::compute(&key, payload.as_bytes());
            (h, true)
        }
        Err(_) => (String::new(), false),
    };

    Ok(Json(SignedReport {
        report,
        nonce,
        hmac,
        signed,
    }))
}
