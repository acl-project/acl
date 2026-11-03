// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

pub mod account;
pub mod credential;
pub mod error;
pub mod microsoft;
pub mod store;

pub use account::{Account, AccountType};
pub use credential::CredentialStore;
pub use error::{AuthError, Result};
pub use store::AccountStore;
