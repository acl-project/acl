// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    fs::{self},
    path::Path,
};

use acl_common::region::is_mainland_china;
use acl_minecraft::mirror::DownloadSource;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::{
    error::{CoreError, Result},
    java::{self, JavaInstallation},
};

/// Global launcher configuration stored in `config.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LauncherConfig {
    /// Minimum JVM memory allocation in megabytes (MB).
    pub min_memory_mb: u32,
    /// Maximum JVM memory allocation in megabytes (MB).
    pub max_memory_mb: u32,
    /// Default java
    pub default_java: Option<JavaInstallation>,
    /// Discovered or configured Java installations.
    pub java_list: Vec<JavaInstallation>,
    /// Maximum number of concurrent file downloads.
    pub max_concurrent_downloads: usize,
    /// Whether to close the launcher window when the game starts.
    pub close_launcher_on_launch: bool,
    /// The launcher's download source.
    pub download_source: DownloadSource,
    /// Offline feature toggle.
    pub enable_offline: bool,
}

impl Default for LauncherConfig {
    fn default() -> Self {
        Self {
            min_memory_mb: 512,
            max_memory_mb: 4096,
            default_java: None,
            java_list: Vec::new(),
            max_concurrent_downloads: 16,
            close_launcher_on_launch: false,
            download_source: DownloadSource::Official,
            enable_offline: false,
        }
    }
}

impl LauncherConfig {
    /// Recommended launcher configuration.
    pub fn recommended() -> Self {
        let download_source = if is_mainland_china() {
            DownloadSource::Bmclapi
        } else {
            DownloadSource::Official
        };

        let threads = match std::thread::available_parallelism() {
            Ok(count) => count.get(),
            Err(err) => {
                warn!("Failed to get thread count: {err}");
                1
            }
        };

        let max_concurrent_downloads = threads.saturating_mul(4).clamp(16, 64);

        info!("Set max concurrent downloads {max_concurrent_downloads}");

        Self {
            min_memory_mb: 512,
            max_memory_mb: 4096,
            default_java: None,
            java_list: Vec::new(),
            max_concurrent_downloads,
            close_launcher_on_launch: false,
            download_source,
            enable_offline: false, // Offline login is disabled by default.
        }
    }

    pub fn load_or_recommended(path: &Path) -> Result<Self> {
        if !path.exists() {
            info!("Configuration file not found, using recommended settings");
            let mut config: LauncherConfig = Self::recommended();

            // Search for Java once during initialization.
            config.search_java();

            config.save(path)?;
            return Ok(config);
        }

        info!("Loading configuration from {}", path.display());

        let content = fs::read_to_string(path).map_err(|source| CoreError::Io {
            path: path.to_path_buf(),
            source,
        })?;

        let config = toml::from_str(&content)?;

        Ok(config)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let content = toml::to_string_pretty(self)?;

        acl_common::fs::write_atomic(path, content.as_bytes()).map_err(|source| CoreError::Io {
            path: path.to_path_buf(),
            source,
        })?;

        info!("Luancher configuration saved to {}", path.display());
        Ok(())
    }

    /// Search for Java installations on the system.
    pub fn search_java(&mut self) {
        let java_list = java::discover_java_installations();
        info!("Discovered {} Java installation(s)", java_list.len());

        if java_list.is_empty() {
            warn!("No valid Java installation found during discovery");
        }

        self.java_list = java_list;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
    };

    struct TempDirGuard {
        path: PathBuf,
    }

    impl TempDirGuard {
        fn new(name: &str) -> Self {
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let id = COUNTER.fetch_add(1, Ordering::SeqCst);
            let path = std::env::temp_dir().join(format!(
                "acl_test_cfg_{}_{}_{}",
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
    fn test_default_config() {
        let config = LauncherConfig::default();
        assert_eq!(config.min_memory_mb, 512);
        assert_eq!(config.max_memory_mb, 4096);
        assert_eq!(config.default_java, None);
        assert!(config.java_list.is_empty());
        assert_eq!(config.max_concurrent_downloads, 16);
        assert!(!config.close_launcher_on_launch);
        assert_eq!(config.download_source, DownloadSource::Official);
    }

    #[test]
    fn test_toml_serialization_roundtrip() {
        let java = JavaInstallation {
            path: PathBuf::from("/usr/bin/java"),
            info: crate::java::JavaInfo {
                version: "21.0.2".to_string(),
                major_version: 21,
                vendor: Some("Eclipse Adoptium".to_string()),
                is_64bit: true,
            },
        };
        let config = LauncherConfig {
            min_memory_mb: 2048,
            max_memory_mb: 8192,
            default_java: Some(java.clone()),
            java_list: vec![java],
            max_concurrent_downloads: 8,
            close_launcher_on_launch: true,
            download_source: DownloadSource::Official,
            enable_offline: false,
        };

        let toml_str = toml::to_string_pretty(&config).expect("failed to serialize config");
        let deserialized: LauncherConfig =
            toml::from_str(&toml_str).expect("failed to deserialize config");
        assert_eq!(config, deserialized);
    }

    #[test]
    fn test_partial_toml_deserialization_uses_defaults() {
        let partial_toml = r#"
            max_memory_mb = 8192
            close_launcher_on_launch = true
        "#;
        let config: LauncherConfig =
            toml::from_str(partial_toml).expect("failed to parse partial toml");
        assert_eq!(config.max_memory_mb, 8192);
        assert!(config.close_launcher_on_launch);
        assert_eq!(config.min_memory_mb, 512);
        assert_eq!(config.max_concurrent_downloads, 16);
        assert_eq!(config.default_java, None);
        assert!(config.java_list.is_empty());
        assert_eq!(config.download_source, DownloadSource::Official);
    }

    #[test]
    fn test_recommended_config_bounds_and_defaults() {
        let config = LauncherConfig::recommended();
        assert_eq!(config.min_memory_mb, 512);
        assert_eq!(config.max_memory_mb, 4096);
        assert_eq!(config.default_java, None);
        assert!(config.java_list.is_empty());
        assert!(!config.close_launcher_on_launch);
        assert!((16..=64).contains(&config.max_concurrent_downloads));
        if acl_common::region::is_mainland_china() {
            assert_eq!(config.download_source, DownloadSource::Bmclapi);
        } else {
            assert_eq!(config.download_source, DownloadSource::Official);
        }
    }

    #[test]
    fn test_load_or_recommended_creates_file_when_missing() {
        let temp = TempDirGuard::new("missing");
        let config_path = temp.path().join("config.toml");
        assert!(!config_path.exists());

        let loaded = LauncherConfig::load_or_recommended(&config_path)
            .expect("failed to load or recommended");
        assert!(config_path.exists());

        let mut expected = LauncherConfig::recommended();
        expected.search_java();

        assert_eq!(loaded, expected);

        let content = fs::read_to_string(&config_path).expect("failed to read created config");
        let parsed: LauncherConfig =
            toml::from_str(&content).expect("failed to parse config from disk");
        assert_eq!(parsed, loaded);
    }

    #[test]
    fn test_load_or_recommended_loads_existing_file() {
        let temp = TempDirGuard::new("existing");
        let config_path = temp.path().join("config.toml");

        let initial = LauncherConfig {
            max_memory_mb: 6144,
            ..Default::default()
        };
        initial.save(&config_path).expect("failed to save config");

        let loaded =
            LauncherConfig::load_or_recommended(&config_path).expect("failed to load config");
        assert_eq!(loaded.max_memory_mb, 6144);
        assert_eq!(loaded, initial);
    }

    #[test]
    fn test_load_or_recommended_invalid_toml_returns_error() {
        let temp = TempDirGuard::new("invalid");
        let config_path = temp.path().join("config.toml");
        fs::write(&config_path, "invalid [ toml [[[ syntax").expect("failed to write invalid toml");

        let result = LauncherConfig::load_or_recommended(&config_path);
        assert!(matches!(result, Err(CoreError::ConfigParse(_))));
    }

    #[test]
    fn test_save_atomic_creates_parent_dirs() {
        let temp = TempDirGuard::new("nested_save");
        let config_path = temp.path().join("sub").join("nested").join("config.toml");

        let config = LauncherConfig {
            min_memory_mb: 2048,
            ..Default::default()
        };
        config
            .save(&config_path)
            .expect("failed to save in nested dir");

        assert!(config_path.exists());
        let tmp_path = config_path.with_extension("toml.tmp");
        assert!(!tmp_path.exists());

        let loaded =
            LauncherConfig::load_or_recommended(&config_path).expect("failed to load saved config");
        assert_eq!(loaded.min_memory_mb, 2048);
    }

    #[test]
    fn test_search_java_preserves_default_java_none() {
        let mut config = LauncherConfig {
            default_java: None,
            ..Default::default()
        };

        config.search_java();
        assert_eq!(config.default_java, None);
    }

    #[test]
    fn test_search_java_preserves_explicit_default_java() {
        let mock_java = JavaInstallation {
            path: PathBuf::from("/mock/custom/java"),
            info: crate::java::JavaInfo {
                version: "17.0.1".to_string(),
                major_version: 17,
                vendor: Some("CustomVendor".to_string()),
                is_64bit: true,
            },
        };
        let mut config = LauncherConfig {
            default_java: Some(mock_java.clone()),
            ..Default::default()
        };

        config.search_java();
        assert_eq!(config.default_java, Some(mock_java));
    }
}
