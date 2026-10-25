use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum MinecraftError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Failed to parse JSON: {0}")]
    Json(#[from] serde_json::Error),

    #[error("I/O error at path '{path}': {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Minecraft version '{version_id}' not found in manifest")]
    VersionNotFound { version_id: String },

    #[error("System clock error: {0}")]
    SystemTime(#[from] std::time::SystemTimeError),
}

pub type Result<T> = std::result::Result<T, MinecraftError>;
