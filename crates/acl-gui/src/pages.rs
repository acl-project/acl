// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

pub mod accounts;
pub mod download;
pub mod home;
pub mod installer;
pub mod instance_detail;
pub mod instances;
pub mod settings;
pub mod tasks;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
enum LoadState {
    /// Not yet loaded (initial lazy-load state)
    #[default]
    Idle,
    /// Currently fetching / reading from cache
    Loading,
    /// Successfully loaded
    Ready,
    /// Failed to load (retains localized error message for display and retry)
    Error(String),
}
