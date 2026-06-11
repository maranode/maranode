//! GGUF blob storage, model manifests, and SQLite metadata

pub mod blob;
pub mod bootstrap;
pub mod db;
pub mod defaults;
pub mod download;
pub mod store;
pub mod user_db;
pub mod workspace_db;

pub use bootstrap::{maybe_bootstrap, BootstrapOptions, ModelCoverage};
pub use store::ModelStore;
pub use user_db::{SessionRecord, UserDb};
pub use workspace_db::{generate_dek, WorkspaceDb};
