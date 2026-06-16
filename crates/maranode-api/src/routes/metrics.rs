//! Prometheus text exposition at /metrics. opt-in and, by default, behind the admin key.

use std::fmt::Write as _;
use std::sync::atomic::Ordering;

use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    routing::get,
    Router,
};

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/metrics", get(metrics_handler))
}

pub struct MetricsSnapshot {
    pub uptime_seconds: u64,
    pub requests: u64,
    pub errors: u64,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub duration_ms_total: u64,
    pub queue_depth: u64,
    pub queue_max: u64,
    pub audit_seq: u64,
    pub workspaces: u64,
    pub air_gap: bool,
    pub isolation_ok: bool,
    pub audit_frozen: bool,
    pub version: String,
}

async fn metrics_handler(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let rt = state.rt();
    if !rt.metrics_enabled {
        return (StatusCode::NOT_FOUND, "metrics are disabled").into_response();
    }
    if rt.metrics_require_auth {
        if let Some(key) = rt.admin_key.as_deref().filter(|k| !k.is_empty()) {
            let provided = headers
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
                .unwrap_or("");
            if !maranode_common::secure::ct_eq_str(provided, key) {
                return (StatusCode::UNAUTHORIZED, "admin key required").into_response();
            }
        }
    }

    let s = &state.stats;
    let snap = MetricsSnapshot {
        uptime_seconds: s.started_at.elapsed().as_secs(),
        requests: s.requests.load(Ordering::Relaxed),
        errors: s.errors.load(Ordering::Relaxed),
        tokens_in: s.tokens_in.load(Ordering::Relaxed),
        tokens_out: s.tokens_out.load(Ordering::Relaxed),
        duration_ms_total: s.duration_ms.load(Ordering::Relaxed),
        queue_depth: state.engine.queue_depth() as u64,
        queue_max: state.engine.max_queue_depth() as u64,
        audit_seq: state.audit.seq().await,
        workspaces: state.workspace_audits.lock().await.len() as u64,
        air_gap: rt.air_gap,
        isolation_ok: state.isolation_ok.load(Ordering::Relaxed),
        audit_frozen: state.audit_frozen.load(Ordering::Relaxed),
        version: state.version.clone(),
    };

    let body = encode_metrics(&snap);
    (
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        body,
    )
        .into_response()
}

pub fn encode_metrics(m: &MetricsSnapshot) -> String {
    let mut out = String::with_capacity(1024);

    counter(&mut out, "maranode_requests_total", "Total API requests handled.", m.requests);
    counter(&mut out, "maranode_request_errors_total", "API requests that ended in an error.", m.errors);
    counter(&mut out, "maranode_tokens_input_total", "Prompt tokens processed.", m.tokens_in);
    counter(&mut out, "maranode_tokens_output_total", "Generated tokens produced.", m.tokens_out);
    counter(&mut out, "maranode_request_duration_milliseconds_total", "Summed handler duration in milliseconds.", m.duration_ms_total);

    gauge(&mut out, "maranode_uptime_seconds", "Seconds since the daemon started.", m.uptime_seconds);
    gauge(&mut out, "maranode_inference_queue_depth", "Requests waiting in the inference queue.", m.queue_depth);
    gauge(&mut out, "maranode_inference_queue_max", "Maximum inference queue depth.", m.queue_max);
    gauge(&mut out, "maranode_audit_log_sequence", "Sequence number of the last audit entry.", m.audit_seq);
    gauge(&mut out, "maranode_workspaces", "Number of workspaces with an audit log.", m.workspaces);
    gauge(&mut out, "maranode_air_gap_enabled", "1 when air-gap enforcement is on.", m.air_gap as u64);
    gauge(&mut out, "maranode_isolation_ok", "1 when the last isolation probe found no egress.", m.isolation_ok as u64);
    gauge(&mut out, "maranode_audit_frozen", "1 when the audit log is frozen for an incident.", m.audit_frozen as u64);

    let _ = writeln!(out, "# HELP maranode_build_info Build information.");
    let _ = writeln!(out, "# TYPE maranode_build_info gauge");
    let _ = writeln!(out, "maranode_build_info{{version=\"{}\"}} 1", escape(&m.version));

    out
}

fn counter(out: &mut String, name: &str, help: &str, value: u64) {
    line(out, name, "counter", help, value);
}

fn gauge(out: &mut String, name: &str, help: &str, value: u64) {
    line(out, name, "gauge", help, value);
}

fn line(out: &mut String, name: &str, kind: &str, help: &str, value: u64) {
    let _ = writeln!(out, "# HELP {name} {help}");
    let _ = writeln!(out, "# TYPE {name} {kind}");
    let _ = writeln!(out, "{name} {value}");
}

fn escape(label: &str) -> String {
    label.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> MetricsSnapshot {
        MetricsSnapshot {
            uptime_seconds: 12,
            requests: 5,
            errors: 1,
            tokens_in: 100,
            tokens_out: 250,
            duration_ms_total: 900,
            queue_depth: 2,
            queue_max: 32,
            audit_seq: 42,
            workspaces: 3,
            air_gap: true,
            isolation_ok: true,
            audit_frozen: false,
            version: "1.2.3".into(),
        }
    }

    #[test]
    fn emits_type_and_value_lines() {
        let out = encode_metrics(&sample());
        assert!(out.contains("# TYPE maranode_requests_total counter"));
        assert!(out.contains("\nmaranode_requests_total 5\n"));
        assert!(out.contains("maranode_air_gap_enabled 1"));
        assert!(out.contains("maranode_audit_frozen 0"));
        assert!(out.contains("maranode_build_info{version=\"1.2.3\"} 1"));
    }

    #[test]
    fn every_metric_has_help_and_type() {
        let out = encode_metrics(&sample());
        let helps = out.matches("# HELP ").count();
        let types = out.matches("# TYPE ").count();
        assert_eq!(helps, types);
        assert!(helps >= 13);
    }

    #[test]
    fn label_value_is_escaped() {
        let mut m = sample();
        m.version = "a\"b\\c".into();
        let out = encode_metrics(&m);
        assert!(out.contains("maranode_build_info{version=\"a\\\"b\\\\c\"} 1"));
    }
}
