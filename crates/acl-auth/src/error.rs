use std::path::PathBuf;

use thiserror::Error;
use uuid::Uuid;

/// Error type for authentication and account management operations.
#[derive(Debug, Error)]
pub enum AuthError {
    #[error("Username cannot be empty")]
    EmptyUsername,

    #[error("Username length must be between 3 and 16 characters, got {len}")]
    InvalidUsernameLength { len: usize },

    #[error(
        "Username contains invalid character '{ch}'; only ASCII alphanumeric characters and underscores are allowed"
    )]
    InvalidUsernameChar { ch: char },

    #[error("The account with UUID '{uuid}' does not exist")]
    AccountNotFound { uuid: Uuid },

    #[error("I/O error at path '{path}': {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Failed to parse JSON: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, AuthError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_error_display() {
        assert_eq!(
            AuthError::EmptyUsername.to_string(),
            "Username cannot be empty"
        );
        assert_eq!(
            AuthError::InvalidUsernameLength { len: 2 }.to_string(),
            "Username length must be between 3 and 16 characters, got 2"
        );
        assert_eq!(
            AuthError::InvalidUsernameChar { ch: '@' }.to_string(),
            "Username contains invalid character '@'; only ASCII alphanumeric characters and underscores are allowed"
        );
        let test_uuid = Uuid::nil();
        assert_eq!(
            AuthError::AccountNotFound { uuid: test_uuid }.to_string(),
            format!("The account with UUID '{test_uuid}' does not exist")
        );
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        assert_eq!(
            AuthError::Io {
                path: PathBuf::from("/test/path"),
                source: io_err,
            }
            .to_string(),
            "I/O error at path '/test/path': file not found"
        );
        let json_err = serde_json::from_str::<serde_json::Value>("invalid json").unwrap_err();
        let expected_json_msg = format!("Failed to parse JSON: {json_err}");
        assert_eq!(AuthError::Json(json_err).to_string(), expected_json_msg);
    }
}
