// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use keyring::Entry;
use uuid::Uuid;

use crate::microsoft::types::MicrosoftTokenBundle;
use crate::{AuthError, Result};
const SERVICE_NAME: &str = "adwaita-craft-launcher";

pub struct CredentialStore;

impl CredentialStore {
    /// Save the token. If the system keyring fails, throw an error directly and never proceed with any subsequent persistence.
    pub fn set(uuid: Uuid, tokens: &MicrosoftTokenBundle) -> Result<()> {
        let entry = Entry::new(SERVICE_NAME, &uuid.to_string())?;
        entry
            .set_password(&serde_json::to_string(tokens)?)
            .map_err(AuthError::from)
    }
    /// Get the Token.
    pub fn get(uuid: Uuid) -> Result<MicrosoftTokenBundle> {
        let entry = Entry::new(SERVICE_NAME, &uuid.to_string())?;
        match entry.get_password() {
            Ok(tokens) => Ok(serde_json::from_str(&tokens)?),
            Err(err) => Err(AuthError::Keyring(err)),
        }
    }

    pub fn delete(uuid: Uuid) -> Result<()> {
        let entry = Entry::new(SERVICE_NAME, &uuid.to_string())?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(AuthError::Keyring(err)),
        }
    }
}
