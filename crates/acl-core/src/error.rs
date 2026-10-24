use std::path::PathBuf;

use thiserror::Error;

/// Core error type for launcher operations.
#[derive(Debug, Error)]
pub enum CoreError {
    #[error("Failed to determine standard application directories")]
    ProjectDirsUnavailable,

    #[error("I/O error at path '{path}': {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Failed to parse configuration file: {0}")]
    ConfigParse(#[from] toml::de::Error),

    #[error("Failed to serialize configuration: {0}")]
    ConfigSerialize(#[from] toml::ser::Error),
}

pub type Result<T> = std::result::Result<T, CoreError>;
