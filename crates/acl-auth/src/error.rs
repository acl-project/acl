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
    }
}
