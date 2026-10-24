use std::fs;
use std::path::Path;
use std::path::PathBuf;

use directories::ProjectDirs;
use tracing::debug;

use crate::error::CoreError;
use crate::error::Result;

#[derive(Debug, Clone)]
pub struct LauncherDirs {
    config_dir: PathBuf,
    data_dir: PathBuf,
}

impl LauncherDirs {
    /// Creates a new `LauncherDirs` using system standard directories.
    pub fn new() -> Result<Self> {
        let project_dirs = ProjectDirs::from("com", "acl-project", "adwaita-craft-launcher")
            .ok_or(CoreError::ProjectDirsUnavailable)?;

        Ok(Self {
            config_dir: project_dirs.config_dir().to_path_buf(),
            data_dir: project_dirs.data_dir().to_path_buf(),
        })
    }

    /// Creates a `LauncherDirs` with custom paths (useful for testing).
    pub fn with_paths(config_dir: PathBuf, data_dir: PathBuf) -> Self {
        Self {
            config_dir,
            data_dir,
        }
    }

    pub fn ensure_dir_exist(&self) -> Result<()> {
        let dirs = [
            self.config_dir(),
            self.data_dir(),
            &self.assets_dir(),
            &self.instances_dir(),
            &self.libraries_dir(),
            &self.versions_dir(),
        ];

        for dir in dirs {
            if !dir.exists() {
                debug!("Creating launcher directory: {}", dir.display());
                fs::create_dir_all(dir).map_err(|source| CoreError::Io {
                    path: dir.to_path_buf(),
                    source,
                })?;
            }
        }

        Ok(())
    }

    /// Path to the configuration directory.
    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    /// Path to the root data directory.
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// Path to the main configuration file (`config.toml`).
    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    /// Path to the game instances directory.
    pub fn instances_dir(&self) -> PathBuf {
        self.data_dir.join("instances")
    }

    /// Path to the shared Minecraft assets directory.
    pub fn assets_dir(&self) -> PathBuf {
        self.data_dir.join("assets")
    }

    /// Path to the shared Minecraft libraries directory.
    pub fn libraries_dir(&self) -> PathBuf {
        self.data_dir.join("libraries")
    }

    /// Path to the shared Minecraft versions directory.
    pub fn versions_dir(&self) -> PathBuf {
        self.data_dir.join("versions")
    }
}
