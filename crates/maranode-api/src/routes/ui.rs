//! serves embedded ui at /ui and static files at /ui/assets/*

use axum::{
    extract::Path,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use rust_embed::Embed;

use crate::state::AppState;

#[derive(Embed)]
#[folder = "../../ui/"]
struct Assets;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/ui", get(serve_index))
        .route("/ui/", get(serve_index))
        .route("/ui/assets/*path", get(serve_asset))
}

async fn serve_index() -> Response {
    serve_embedded("index.html")
}

async fn serve_asset(Path(path): Path<String>) -> Response {
    serve_embedded(&format!("assets/{}", path))
}

fn serve_embedded(path: &str) -> Response {
    match Assets::get(path) {
        Some(file) => {
            let mime = mime_guess::from_path(path)
                .first_or_octet_stream()
                .to_string();
            (StatusCode::OK, [(header::CONTENT_TYPE, mime)], file.data).into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "text/plain".to_string())],
            format!("Asset not found: {}", path).into_bytes(),
        )
            .into_response(),
    }
}
