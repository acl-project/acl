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

    #[error("SHA-1 mismatch for '{target}': expected {expected}, actual {actual}")]
    HashMismatch {
        target: String,
        expected: String,
        actual: String,
    },

    #[error("Zip archive error at path '{path}': {source}")]
    Zip {
        path: PathBuf,
        #[source]
        source: zip::result::ZipError,
    },
}

pub type Result<T> = std::result::Result<T, MinecraftError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_minecraft_error_display() {
        let err = MinecraftError::VersionNotFound {
            version_id: "1.21".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "Minecraft version '1.21' not found in manifest"
        );

        let io_err = MinecraftError::Io {
            path: PathBuf::from("/test/path"),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "file missing"),
        };
        assert_eq!(
            io_err.to_string(),
            "I/O error at path '/test/path': file missing"
        );

        let hash_err = MinecraftError::HashMismatch {
            target: "1.21.json".to_string(),
            expected: "abcdef".to_string(),
            actual: "123456".to_string(),
        };
        assert_eq!(
            hash_err.to_string(),
            "SHA-1 mismatch for '1.21.json': expected abcdef, actual 123456"
        );

        let zip_err = MinecraftError::Zip {
            path: PathBuf::from("/test/archive.jar"),
            source: zip::result::ZipError::FileNotFound,
        };
        assert_eq!(
            zip_err.to_string(),
            "Zip archive error at path '/test/archive.jar': specified file not found in archive"
        );
    }
}
