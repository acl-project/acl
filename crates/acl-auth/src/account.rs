use crate::error::{AuthError, Result};
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use tracing::debug;
use uuid::Uuid;

pub const MIN_USERNAME_LEN: usize = 3;
pub const MAX_USERNAME_LEN: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountType {
    /// Local offline-mode account.
    Offline,
    /// Microsoft OAuth 2.0 account.
    Microsoft,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub uuid: Uuid,
    pub username: String,
    pub account_type: AccountType,
}

impl Account {
    pub fn new_offline(username: impl Into<String>) -> Result<Self> {
        let username = username.into();
        Self::validate_username(&username)?;

        let uuid = Self::generate_offline_uuid(&username);
        debug!(
            "Created offline account for user '{}' with UUID {}",
            username, uuid
        );

        Ok(Self {
            uuid,
            username,
            account_type: AccountType::Offline,
        })
    }

    /// Validates that a Minecraft username is 3-16 characters and contains only ASCII
    /// alphanumeric characters or underscores (`_`).
    pub fn validate_username(username: &str) -> Result<()> {
        if username.is_empty() {
            return Err(AuthError::EmptyUsername);
        }
        let len = username.chars().count();
        if !(MIN_USERNAME_LEN..=MAX_USERNAME_LEN).contains(&len) {
            return Err(AuthError::InvalidUsernameLength { len });
        }

        if let Some(ch) = username
            .chars()
            .find(|&c| !(c.is_ascii_alphanumeric() || c == '_'))
        {
            return Err(AuthError::InvalidUsernameChar { ch });
        }

        Ok(())
    }

    pub fn generate_offline_uuid(username: &str) -> Uuid {
        let mut hasher = Md5::new();
        hasher.update(b"OfflinePlayer:");
        hasher.update(username.as_bytes());
        let digest: [u8; 16] = hasher.finalize().into();

        uuid::Builder::from_md5_bytes(digest).into_uuid()
    }
}
