//! shared types, paths, and audit event definitions

pub mod baseline;
pub mod error;
pub mod events;
pub mod gguf;
pub mod models;
pub mod paths;
pub mod receipt;
pub mod secure;
pub mod types;
pub mod user;
pub mod workspace;

pub use error::{MaranodeError, Result};
