use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};
use tracing::info;
use uuid::Uuid;

use crate::{Account, AuthError, Result};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AccountStore {
    accounts: Vec<Account>,
    active_uuid: Option<Uuid>,
}

impl AccountStore {
    pub fn new() -> Self {
        Self {
            accounts: Vec::new(),
            active_uuid: None,
        }
    }

    pub fn accounts(&self) -> &[Account] {
        &self.accounts
    }

    pub fn active_account(&self) -> Option<&Account> {
        self.accounts
            .iter()
            .find(|account| Some(account.uuid) == self.active_uuid)
    }

    pub fn active_uuid(&self) -> Option<Uuid> {
        self.active_uuid
    }

    /// If an account with the same uuid already exists, update that account's information; otherwise, append it to the list.
    /// If the current active_uuid is None, automatically set this account as the currently selected account.
    pub fn add_account(&mut self, account: Account) {
        if self.active_uuid.is_none() {
            self.active_uuid = Some(account.uuid);
        }

        if let Some(i) = self
            .accounts
            .iter()
            .position(|acc| acc.uuid == account.uuid)
        {
            self.accounts[i] = account;
        } else {
            self.accounts.push(account);
        }
    }

    /// Remove the account by uuid.
    /// If the removed account is the currently selected account, update active_uuid to the first account in the remaining list.
    pub fn remove_account(&mut self, uuid: Uuid) -> Result<Account> {
        let account = self
            .accounts
            .iter()
            .position(|account| account.uuid == uuid)
            .map(|i| self.accounts.remove(i))
            .ok_or(AuthError::AccountNotFound { uuid });

        if account.is_ok() && self.active_uuid == Some(uuid) {
            if let Some(first) = self.accounts.first() {
                self.active_uuid = Some(first.uuid);
            } else {
                self.active_uuid = None;
            }
        }

        account
    }

    /// If the uuid exists in the accounts list, set it as the currently active account.
    pub fn set_active_account(&mut self, uuid: Uuid) -> Result<&Account> {
        self.accounts
            .iter()
            .position(|account| account.uuid == uuid)
            .map(|i| {
                self.active_uuid = Some(uuid);
                &self.accounts[i]
            })
            .ok_or(AuthError::AccountNotFound { uuid })
    }

    pub fn load_or_default(path: &Path) -> Result<Self> {
        if !path.exists() {
            info!("Accounts file not found, using default settings");
            let store = Self::default();
            store.save(path)?;
            return Ok(store);
        }

        info!("Loading accounts from {}", path.display());

        let content = fs::read_to_string(path).map_err(|source| AuthError::Io {
            path: path.to_path_buf(),
            source,
        })?;

        Ok(serde_json::from_str(&content)?)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| AuthError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        let content = serde_json::to_string_pretty(self)?;

        let tmp_path = path.with_extension("json.tmp");

        // Write the file atomically.
        let write_result = (|| -> std::io::Result<()> {
            let mut tmp_file = File::create(&tmp_path)?;
            tmp_file.write_all(content.as_bytes())?;
            tmp_file.sync_all()?;
            Ok(())
        })();

        if let Err(source) = write_result {
            let _ = fs::remove_file(&tmp_path);
            return Err(AuthError::Io {
                path: tmp_path,
                source,
            });
        }

        fs::rename(&tmp_path, path).map_err(|source| {
            let _ = fs::remove_file(&tmp_path);
            AuthError::Io {
                path: path.to_path_buf(),
                source,
            }
        })?;

        info!("Accounts saved to {}", path.display());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct TempDirGuard {
        path: PathBuf,
    }

    impl TempDirGuard {
        fn new(name: &str) -> Self {
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let id = COUNTER.fetch_add(1, Ordering::SeqCst);
            let path = std::env::temp_dir().join(format!(
                "acl_test_auth_store_{}_{}_{}",
                name,
                std::process::id(),
                id
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("failed to create temp test dir");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn test_store_new_and_default() {
        let store = AccountStore::new();
        assert!(store.accounts().is_empty());
        assert_eq!(store.active_uuid(), None);
        assert_eq!(store.active_account(), None);
        assert_eq!(store, AccountStore::default());
    }

    #[test]
    fn test_store_add_account_first_and_subsequent() {
        let mut store = AccountStore::new();
        let acc1 = Account::new_offline("Player1").unwrap();
        let acc2 = Account::new_offline("Player2").unwrap();

        store.add_account(acc1.clone());
        assert_eq!(store.accounts().len(), 1);
        assert_eq!(store.active_uuid(), Some(acc1.uuid));
        assert_eq!(store.active_account(), Some(&acc1));

        store.add_account(acc2.clone());
        assert_eq!(store.accounts().len(), 2);
        assert_eq!(store.active_uuid(), Some(acc1.uuid));
        assert_eq!(store.active_account(), Some(&acc1));
    }

    #[test]
    fn test_store_add_account_update_existing() {
        let mut store = AccountStore::new();
        let acc1 = Account::new_offline("Player1").unwrap();
        store.add_account(acc1.clone());

        let mut acc1_updated = acc1.clone();
        acc1_updated.username = "Player1Updated".to_string();
        store.add_account(acc1_updated.clone());

        assert_eq!(store.accounts().len(), 1);
        assert_eq!(store.accounts()[0].username, "Player1Updated");
        assert_eq!(store.active_uuid(), Some(acc1.uuid));
    }

    #[test]
    fn test_store_set_active_account() {
        let mut store = AccountStore::new();
        let acc1 = Account::new_offline("Player1").unwrap();
        let acc2 = Account::new_offline("Player2").unwrap();
        store.add_account(acc1.clone());
        store.add_account(acc2.clone());

        let active = store.set_active_account(acc2.uuid).unwrap();
        assert_eq!(active, &acc2);
        assert_eq!(store.active_uuid(), Some(acc2.uuid));
        assert_eq!(store.active_account(), Some(&acc2));

        let missing_uuid = Uuid::nil();
        let err = store.set_active_account(missing_uuid).unwrap_err();
        assert!(matches!(err, AuthError::AccountNotFound { uuid } if uuid == missing_uuid));
    }

    #[test]
    fn test_store_remove_account_not_found() {
        let mut store = AccountStore::new();
        let missing_uuid = Uuid::nil();
        let err = store.remove_account(missing_uuid).unwrap_err();
        assert!(matches!(err, AuthError::AccountNotFound { uuid } if uuid == missing_uuid));
    }

    #[test]
    fn test_store_remove_account_active_and_fallback() {
        let mut store = AccountStore::new();
        let acc1 = Account::new_offline("Player1").unwrap();
        let acc2 = Account::new_offline("Player2").unwrap();
        let acc3 = Account::new_offline("Player3").unwrap();
        store.add_account(acc1.clone());
        store.add_account(acc2.clone());
        store.add_account(acc3.clone());

        // Active account is acc1. Remove acc1 -> fallback to acc2 (first in remaining).
        let removed = store.remove_account(acc1.uuid).unwrap();
        assert_eq!(removed, acc1);
        assert_eq!(store.accounts().len(), 2);
        assert_eq!(store.active_uuid(), Some(acc2.uuid));

        // Remove acc2 -> fallback to acc3.
        store.remove_account(acc2.uuid).unwrap();
        assert_eq!(store.active_uuid(), Some(acc3.uuid));

        // Remove acc3 -> list is empty, active_uuid becomes None.
        store.remove_account(acc3.uuid).unwrap();
        assert!(store.accounts().is_empty());
        assert_eq!(store.active_uuid(), None);
    }

    #[test]
    fn test_store_remove_account_non_active() {
        let mut store = AccountStore::new();
        let acc1 = Account::new_offline("Player1").unwrap();
        let acc2 = Account::new_offline("Player2").unwrap();
        store.add_account(acc1.clone());
        store.add_account(acc2.clone());

        // Active is acc1. Remove acc2.
        let removed = store.remove_account(acc2.uuid).unwrap();
        assert_eq!(removed, acc2);
        assert_eq!(store.accounts().len(), 1);
        // active_uuid must remain acc1
        assert_eq!(store.active_uuid(), Some(acc1.uuid));
    }

    #[test]
    fn test_store_load_or_default_creates_missing_file() {
        let temp = TempDirGuard::new("missing_file");
        let store_path = temp.path().join("accounts.json");
        assert!(!store_path.exists());

        let loaded =
            AccountStore::load_or_default(&store_path).expect("failed to load default store");
        assert_eq!(loaded, AccountStore::default());
        assert!(store_path.exists());

        let content = fs::read_to_string(&store_path).expect("failed to read created store file");
        let parsed: AccountStore =
            serde_json::from_str(&content).expect("failed to deserialize store");
        assert_eq!(parsed, AccountStore::default());
    }

    #[test]
    fn test_store_save_and_load_roundtrip() {
        let temp = TempDirGuard::new("roundtrip");
        let store_path = temp.path().join("accounts.json");

        let mut store = AccountStore::new();
        let acc1 = Account::new_offline("Player1").unwrap();
        let acc2 = Account::new_offline("Player2").unwrap();
        store.add_account(acc1.clone());
        store.add_account(acc2.clone());
        store.set_active_account(acc2.uuid).unwrap();

        store.save(&store_path).expect("failed to save store");
        assert!(store_path.exists());

        let loaded = AccountStore::load_or_default(&store_path).expect("failed to load store");
        assert_eq!(loaded, store);
        assert_eq!(loaded.active_uuid(), Some(acc2.uuid));
    }

    #[test]
    fn test_store_load_invalid_json() {
        let temp = TempDirGuard::new("invalid_json");
        let store_path = temp.path().join("accounts.json");
        fs::write(&store_path, "not a valid json content").expect("failed to write invalid json");

        let result = AccountStore::load_or_default(&store_path);
        assert!(matches!(result, Err(AuthError::Json(_))));
    }

    #[test]
    fn test_store_save_creates_parent_dirs() {
        let temp = TempDirGuard::new("nested_save");
        let store_path = temp.path().join("sub").join("nested").join("accounts.json");

        let mut store = AccountStore::new();
        let acc = Account::new_offline("Player1").unwrap();
        store.add_account(acc);

        store
            .save(&store_path)
            .expect("failed to save in nested dir");
        assert!(store_path.exists());
        let tmp_path = store_path.with_extension("json.tmp");
        assert!(!tmp_path.exists());

        let loaded = AccountStore::load_or_default(&store_path).expect("failed to load store");
        assert_eq!(loaded, store);
    }
}
