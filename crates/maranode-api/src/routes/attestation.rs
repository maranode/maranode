//! attestation endpoints. no auth required — designed for third-party verifiers.
//!
//! GET /v1/attestation/report   — live attestation report, Ed25519-signed
//! GET /v1/attestation/public-key — Ed25519 public key for verifying reports
//!
//! Nonce param prevents replay: caller sends ?nonce=<hex>, same value is mixed
//! into the signed payload. Signature covers (report_sha256 || nonce) as UTF-8.
//! A verifier needs only the public key and the two response fields to check authenticity.

use axum::{extract::{Query, State}, routing::get, Json, Router};
use serde::{Deserialize, Serialize};

use maranode_attestation::report::AttestationReport;
use maranode_audit::log::{default_key_path, default_log_path};
use maranode_audit::sign;

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/attestation/report", get(get_report))
        .route("/v1/attestation/public-key", get(get_public_key))
}

#[derive(Deserialize)]
struct ReportQuery {
    nonce: Option<String>,
}

#[derive(Serialize)]
struct SignedReport {
    #[serde(flatten)]
    report: AttestationReport,
    nonce: String,
    /// Ed25519 signature over "<report_sha256><nonce>" as UTF-8 bytes, hex-encoded
    #[serde(skip_serializing_if = "Option::is_none")]
    ed25519_sig: Option<String>,
    /// hex-encoded Ed25519 public key used to produce ed25519_sig
    #[serde(skip_serializing_if = "Option::is_none")]
    ed25519_pubkey: Option<String>,
    /// legacy HMAC-SHA256 with audit key; kept for existing integrations
    #[serde(skip_serializing_if = "String::is_empty")]
    hmac: String,
}

#[derive(Serialize)]
struct PublicKeyResp {
    algorithm: &'static str,
    public_key_hex: String,
    usage: &'static str,
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

    let (ed25519_sig, ed25519_pubkey) = match sign::load_or_create(&state.data_dir) {
        Ok(sk) => {
            let report_hash = report.report_sha256.as_deref().unwrap_or("");
            let payload = format!("{}{}", report_hash, nonce);
            let sig_bytes = sign::sign(&sk, payload.as_bytes());
            let pubkey_hex = hex::encode(sk.verifying_key().to_bytes());
            (Some(hex::encode(sig_bytes)), Some(pubkey_hex))
        }
        Err(e) => {
            tracing::warn!("attestation signing key unavailable: {e}");
            (None, None)
        }
    };

    let hmac = match maranode_audit::key::load(&key_path) {
        Ok(key) => {
            let report_json = serde_json::to_string(&report)
                .map_err(|e| ApiError::internal(e.to_string()))?;
            let payload = format!("{}{}", report_json, nonce);
            maranode_audit::chain::compute(&key, payload.as_bytes())
        }
        Err(_) => String::new(),
    };

    Ok(Json(SignedReport {
        report,
        nonce,
        ed25519_sig,
        ed25519_pubkey,
        hmac,
    }))
}

async fn get_public_key(
    State(state): State<AppState>,
) -> ApiResult<Json<PublicKeyResp>> {
    let sk = sign::load_or_create(&state.data_dir)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(PublicKeyResp {
        algorithm: "ed25519",
        public_key_hex: hex::encode(sk.verifying_key().to_bytes()),
        usage: "verify ed25519_sig in /v1/attestation/report: sign(report_sha256 || nonce)",
    }))
}
