use std::sync::atomic::Ordering;

use axum::{extract::State, routing::get, Json, Router};
use serde_json::{json, Value};

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/stats", get(stats_handler))
}

async fn stats_handler(State(state): State<AppState>) -> Json<Value> {
    let s = &state.stats;
    let requests = s.requests.load(Ordering::Relaxed);
    let errors = s.errors.load(Ordering::Relaxed);
    let tokens_in = s.tokens_in.load(Ordering::Relaxed);
    let tokens_out = s.tokens_out.load(Ordering::Relaxed);
    let duration_ms = s.duration_ms.load(Ordering::Relaxed);
    let uptime_secs = s.started_at.elapsed().as_secs();

    let ok_requests = requests.saturating_sub(errors);
    let avg_latency = if ok_requests > 0 {
        duration_ms / ok_requests
    } else {
        0
    };

    let queue_depth = state.engine.queue_depth();
    let queue_max = state.engine.max_queue_depth();

    Json(json!({
        "uptime_secs":    uptime_secs,
        "requests":       requests,
        "errors":         errors,
        "tokens_in":      tokens_in,
        "tokens_out":     tokens_out,
        "avg_latency_ms": avg_latency,
        "queue_depth":    queue_depth,
        "queue_max":      queue_max,
    }))
}
