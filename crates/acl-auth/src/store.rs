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

        if account.is_ok() {
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
            info!("Configuration file not found, using default settings");
            let config = Self::default();
            config.save(path)?;
            return Ok(config);
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
