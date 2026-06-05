//! default paths for data and model directories

use std::path::PathBuf;

/// GGUF models directory. From MARANODE_MODELS_DIR or ./models.
pub fn default_models_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("MARANODE_MODELS_DIR") {
        return PathBuf::from(dir);
    }
    if let Ok(dir) = std::env::var("MODEL_DIR") {
        return PathBuf::from(dir);
    }
    PathBuf::from("models")
}

/// admin Unix socket. on Linux: /run/maranode. else: under data dir
pub fn default_unix_socket() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        Some("/run/maranode/api.sock".into())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let path = default_data_dir().join("api.sock");
        Some(path.to_string_lossy().into_owned())
    }
}

/// data directory. from MARANODE_DATA_DIR or ~/.local/share/maranode
pub fn default_data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("MARANODE_DATA_DIR") {
        return PathBuf::from(dir);
    }

    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".local/share/maranode");
    }

    PathBuf::from(".maranode-data")
}
