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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_username_empty() {
        let err = Account::validate_username("").unwrap_err();
        assert_eq!(err, AuthError::EmptyUsername);
    }

    #[test]
    fn test_validate_username_valid() {
        assert!(Account::validate_username("abc").is_ok());
        assert!(Account::validate_username("Player_1").is_ok());
        assert!(Account::validate_username("Steve").is_ok());
        assert!(Account::validate_username("1234567890123456").is_ok());
    }

    #[test]
    fn test_validate_username_length_boundaries() {
        assert_eq!(
            Account::validate_username("ab").unwrap_err(),
            AuthError::InvalidUsernameLength { len: 2 }
        );
        assert_eq!(
            Account::validate_username("a".repeat(17).as_str()).unwrap_err(),
            AuthError::InvalidUsernameLength { len: 17 }
        );
    }

    #[test]
    fn test_validate_username_invalid_chars() {
        assert_eq!(
            Account::validate_username("Player-1").unwrap_err(),
            AuthError::InvalidUsernameChar { ch: '-' }
        );
        assert_eq!(
            Account::validate_username("Player 1").unwrap_err(),
            AuthError::InvalidUsernameChar { ch: ' ' }
        );
        assert_eq!(
            Account::validate_username("Player!").unwrap_err(),
            AuthError::InvalidUsernameChar { ch: '!' }
        );
        assert_eq!(
            Account::validate_username("测试玩家").unwrap_err(),
            AuthError::InvalidUsernameChar { ch: '测' }
        );
    }

    #[test]
    fn test_generate_offline_uuid_known_vectors() {
        let steve_uuid = Account::generate_offline_uuid("Steve");
        assert_eq!(
            steve_uuid.to_string(),
            "5627dd98-e6be-3c21-b8a8-e92344183641"
        );
        assert_eq!(steve_uuid.get_version_num(), 3);

        let alex_uuid = Account::generate_offline_uuid("Alex");
        assert_eq!(
            alex_uuid.to_string(),
            "36532b5e-c442-3dbb-a24c-c7e55d0f979a"
        );
        assert_eq!(alex_uuid.get_version_num(), 3);
    }

    #[test]
    fn test_new_offline_success() {
        let account = Account::new_offline("Steve").expect("Account creation should succeed");
        assert_eq!(account.username, "Steve");
        assert_eq!(account.account_type, AccountType::Offline);
        assert_eq!(account.uuid, Account::generate_offline_uuid("Steve"));
    }

    #[test]
    fn test_new_offline_invalid() {
        let err = Account::new_offline("ab").unwrap_err();
        assert_eq!(err, AuthError::InvalidUsernameLength { len: 2 });
    }

    #[test]
    fn test_account_type_and_clone() {
        let t1 = AccountType::Offline;
        let t2 = AccountType::Microsoft;
        assert_ne!(t1, t2);
        assert_eq!(t1, t1.clone());

        let account = Account::new_offline("Steve").unwrap();
        let cloned = account.clone();
        assert_eq!(account, cloned);
    }
}
