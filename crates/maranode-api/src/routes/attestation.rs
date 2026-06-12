//! attestation endpoints. no auth required — designed for third-party verifiers.
//!
//! GET /v1/attestation/report   — live attestation report, Ed25519-signed
//! GET /v1/attestation/public-key — Ed25519 public key for verifying reports
//!
//! Nonce param prevents replay: caller sends ?nonce=<hex>, same value is mixed
//! into the signed payload. Signature covers (report_sha256 || nonce) as UTF-8.
//! A verifier needs only the public key and the two response fields to check authenticity.

use axum::{extract::{Query, State}, routing::{get, post}, Json, Router};
use serde::{Deserialize, Serialize};

use maranode_attestation::report::AttestationReport;
use maranode_attestation::{get_tee_report, measure_tee_perf};
use ed25519_dalek::Verifier;
use maranode_audit::log::{default_key_path, default_log_path};
use maranode_audit::sign;

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/attestation/report", get(get_report))
        .route("/v1/attestation/public-key", get(get_public_key))
        .route("/v1/attestation/tee", get(get_tee))
        .route("/v1/attestation/tee/verify", post(verify_tee))
        .route("/v1/attestation/tee/perf", get(get_tee_perf))
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
    #[serde(skip_serializing_if = "Option::is_none")]
    ed25519_sig: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ed25519_pubkey: Option<String>,
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

#[derive(Deserialize)]
struct TeeQuery {
    nonce: Option<String>,
}

#[derive(Serialize)]
struct TeeReportResp {
    tee_type: String,
    report_hash: String,
    measurement: String,
    is_synthetic: bool,
    nonce: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    ed25519_sig: Option<String>,
}

async fn get_tee(
    State(state): State<AppState>,
    Query(q): Query<TeeQuery>,
) -> ApiResult<Json<TeeReportResp>> {
    let nonce_str = q.nonce.unwrap_or_default();
    let report = get_tee_report(nonce_str.as_bytes());

    let ed25519_sig = sign::load_or_create(&state.data_dir).ok().map(|sk| {
        let payload = format!("{}{}", report.report_hash, nonce_str);
        let sig = sign::sign(&sk, payload.as_bytes());
        hex::encode(sig)
    });

    Ok(Json(TeeReportResp {
        tee_type: report.tee_type.to_string(),
        report_hash: report.report_hash,
        measurement: report.measurement,
        is_synthetic: report.is_synthetic,
        nonce: nonce_str,
        ed25519_sig,
    }))
}

#[derive(Deserialize)]
struct VerifyTeeReq {
    report_hash: String,
    nonce: String,
    ed25519_sig: String,
    ed25519_pubkey: String,
}

#[derive(Serialize)]
struct VerifyTeeResp {
    valid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

async fn verify_tee(
    Json(req): Json<VerifyTeeReq>,
) -> ApiResult<Json<VerifyTeeResp>> {
    use ed25519_dalek::{Signature, VerifyingKey};

    let pubkey_bytes = hex::decode(&req.ed25519_pubkey)
        .map_err(|_| ApiError::bad_request("invalid pubkey hex"))?;
    let vk = VerifyingKey::from_bytes(
        pubkey_bytes.as_slice().try_into()
            .map_err(|_| ApiError::bad_request("pubkey must be 32 bytes"))?,
    ).map_err(|_| ApiError::bad_request("invalid ed25519 pubkey"))?;

    let sig_bytes = hex::decode(&req.ed25519_sig)
        .map_err(|_| ApiError::bad_request("invalid signature hex"))?;
    let sig = Signature::from_bytes(
        sig_bytes.as_slice().try_into()
            .map_err(|_| ApiError::bad_request("signature must be 64 bytes"))?,
    );

    let payload = format!("{}{}", req.report_hash, req.nonce);
    match vk.verify_strict(payload.as_bytes(), &sig) {
        Ok(_) => Ok(Json(VerifyTeeResp { valid: true, reason: None })),
        Err(e) => Ok(Json(VerifyTeeResp { valid: false, reason: Some(e.to_string()) })),
    }
}

async fn get_tee_perf() -> Json<maranode_attestation::TeePerf> {
    Json(measure_tee_perf())
}
