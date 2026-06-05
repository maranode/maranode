use axum::{extract::State, routing::get, Json, Router};
use serde_json::{json, Value};

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/health", get(health_handler))
}

async fn health_handler(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "ok":      true,
        "version": state.version,
        "air_gap": state.rt().air_gap,
    }))
}
