//! http api compatible with openai format (axum)

pub mod error;
pub mod openai;
pub mod rag_embedder;
pub mod routes;
pub mod runtime;
pub mod state;
pub mod user_ctx;
pub mod workspace_ctx;

pub use rag_embedder::EngineEmbedder;
pub use runtime::{RuntimeSettings, SharedRuntime, SmtpCfg};
pub use state::{
    new_oidc_pending, AppState, IdentityConfig, LdapCfg, OidcCfg, RagIngestPolicy, SamlCfg,
};
pub use user_ctx::UserCtx;
pub use workspace_ctx::WorkspaceCtx;

use axum::extract::DefaultBodyLimit;
use axum::Router;
use tower_http::trace::TraceLayer;

/// maximum size for request body (default 32 MB)
pub const MAX_BODY_BYTES: usize = 32 * 1024 * 1024;

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .merge(routes::ui::router())
        .merge(routes::health::router())
        .merge(routes::models::router())
        .merge(routes::chat::router())
        .merge(routes::embeddings::router())
        .merge(routes::attestation::router())
        .merge(routes::audit::router())
        .merge(routes::rag::router())
        .merge(routes::stats::router())
        .merge(routes::workspaces::router())
        .merge(routes::users::router())
        .merge(routes::reset::router())
        .merge(routes::identity::router())
        .merge(routes::baseline::router())
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
