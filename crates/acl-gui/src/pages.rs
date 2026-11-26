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

impl LoadState {
    fn is_loading(&self) -> bool {
        matches!(self, LoadState::Loading)
    }

    fn content_page(&self, is_empty: bool) -> &'static str {
        match &self {
            LoadState::Idle | LoadState::Loading => "loading",
            LoadState::Error(_) => "error",
            LoadState::Ready => {
                if is_empty {
                    "empty"
                } else {
                    "list"
                }
            }
        }
    }

    fn error_message(&self) -> &str {
        match &self {
            LoadState::Error(e) => e,
            _ => "",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_state_default_and_is_loading() {
        assert_eq!(LoadState::default(), LoadState::Idle);
        assert!(!LoadState::Idle.is_loading());
        assert!(LoadState::Loading.is_loading());
        assert!(!LoadState::Ready.is_loading());
        assert!(!LoadState::Error("network error".to_string()).is_loading());
    }

    #[test]
    fn test_load_state_content_page() {
        for is_empty in [true, false] {
            assert_eq!(LoadState::Idle.content_page(is_empty), "loading");
            assert_eq!(LoadState::Loading.content_page(is_empty), "loading");
            assert_eq!(
                LoadState::Error("failed".to_string()).content_page(is_empty),
                "error"
            );
            assert_eq!(
                LoadState::Error(String::new()).content_page(is_empty),
                "error"
            );
        }

        assert_eq!(LoadState::Ready.content_page(true), "empty");
        assert_eq!(LoadState::Ready.content_page(false), "list");
    }

    #[test]
    fn test_load_state_error_message() {
        assert_eq!(LoadState::Idle.error_message(), "");
        assert_eq!(LoadState::Loading.error_message(), "");
        assert_eq!(LoadState::Ready.error_message(), "");
        assert_eq!(
            LoadState::Error("Network timeout".to_string()).error_message(),
            "Network timeout"
        );
        assert_eq!(LoadState::Error(String::new()).error_message(), "");
    }
}
