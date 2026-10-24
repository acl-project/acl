use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use tracing::info;

use crate::error::{CoreError, Result};

/// Global launcher configuration stored in `config.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LauncherConfig {
    /// Minimum JVM memory allocation in megabytes (MB).
    pub min_memory_mb: u32,
    /// Maximum JVM memory allocation in megabytes (MB).
    pub max_memory_mb: u32,
    /// Optional custom path to the Java executable.
    pub java_path: Option<PathBuf>,
    /// Maximum number of concurrent file downloads.
    pub max_concurrent_downloads: usize,
    /// Whether to close the launcher window when the game starts.
    pub close_launcher_on_launch: bool,
}

impl Default for LauncherConfig {
    fn default() -> Self {
        Self {
            min_memory_mb: 1024,
            max_memory_mb: 4096,
            java_path: None,
            max_concurrent_downloads: 16,
            close_launcher_on_launch: false,
        }
    }
}

impl LauncherConfig {
    pub fn load_or_default(path: &Path) -> Result<Self> {
        if !path.exists() {
            info!("Configuration file not found, using default settings");
            let config = Self::default();
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
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| CoreError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        let content = toml::to_string_pretty(self)?;

        let tmp_path = path.with_extension("toml.tmp");

        // Write the file atomically.
        let write_result = (|| -> std::io::Result<()> {
            let mut tmp_file = File::create(&tmp_path)?;
            tmp_file.write_all(content.as_bytes())?;
            tmp_file.sync_all()?;
            Ok(())
        })();

        if let Err(source) = write_result {
            let _ = fs::remove_file(&tmp_path);
            return Err(CoreError::Io {
                path: tmp_path,
                source,
            });
        }

        fs::rename(&tmp_path, path).map_err(|source| {
            let _ = fs::remove_file(&tmp_path);
            CoreError::Io {
                path: path.to_path_buf(),
                source,
            }
        })?;

        info!("Configuration saved to {}", path.display());
        Ok(())
    }
}
