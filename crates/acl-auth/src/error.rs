use thiserror::Error;

/// Error type for authentication and account management operations.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AuthError {
    #[error("Username cannot be empty")]
    EmptyUsername,

    #[error("Username length must be between 3 and 16 characters, got {len}")]
    InvalidUsernameLength { len: usize },

    #[error(
        "Username contains invalid character '{ch}'; only ASCII alphanumeric characters and underscores are allowed"
    )]
    InvalidUsernameChar { ch: char },
}

pub type Result<T> = std::result::Result<T, AuthError>;
