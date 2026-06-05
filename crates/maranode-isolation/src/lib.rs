//! block outbound traffic with iptables (default DROP; allow API port and optional whitelist)
//! also manage per-workspace Linux network namespaces

pub mod iptables;
pub mod netns;
pub mod probe;
pub mod state;

pub use state::{IsolationConfig, Isolator, VerifyOutcome, WhitelistEntry};
