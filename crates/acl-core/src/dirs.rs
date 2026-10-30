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

    pub fn ensure_dirs_exist(&self) -> Result<()> {
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

    /// Path to the accounts storage file (`accounts.toml`).
    pub fn accounts_file(&self) -> PathBuf {
        self.config_dir.join("accounts.toml")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct TempDirGuard {
        path: PathBuf,
    }

    impl TempDirGuard {
        fn new(name: &str) -> Self {
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let id = COUNTER.fetch_add(1, Ordering::SeqCst);
            let path = std::env::temp_dir().join(format!(
                "acl_test_dirs_{}_{}_{}",
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
    fn test_with_paths_and_getters() {
        let config_dir = PathBuf::from("/custom/config");
        let data_dir = PathBuf::from("/custom/data");

        let dirs = LauncherDirs::with_paths(config_dir.clone(), data_dir.clone());
        assert_eq!(dirs.config_dir(), config_dir.as_path());
        assert_eq!(dirs.data_dir(), data_dir.as_path());
        assert_eq!(dirs.config_file(), config_dir.join("config.toml"));
        assert_eq!(dirs.instances_dir(), data_dir.join("instances"));
        assert_eq!(dirs.assets_dir(), data_dir.join("assets"));
        assert_eq!(dirs.libraries_dir(), data_dir.join("libraries"));
        assert_eq!(dirs.versions_dir(), data_dir.join("versions"));
    }

    #[test]
    fn test_ensure_dirs_exist() {
        let temp = TempDirGuard::new("ensure_dirs");
        let config_dir = temp.path().join("config");
        let data_dir = temp.path().join("data");

        let dirs = LauncherDirs::with_paths(config_dir.clone(), data_dir.clone());

        assert!(!config_dir.exists());
        assert!(!data_dir.exists());
        assert!(!dirs.assets_dir().exists());
        assert!(!dirs.instances_dir().exists());
        assert!(!dirs.libraries_dir().exists());
        assert!(!dirs.versions_dir().exists());

        dirs.ensure_dirs_exist()
            .expect("failed to ensure dirs exist");

        assert!(config_dir.is_dir());
        assert!(data_dir.is_dir());
        assert!(dirs.assets_dir().is_dir());
        assert!(dirs.instances_dir().is_dir());
        assert!(dirs.libraries_dir().is_dir());
        assert!(dirs.versions_dir().is_dir());

        // Calling it again when directories already exist should succeed idempotently
        dirs.ensure_dirs_exist()
            .expect("failed to call ensure_dirs_exist idempotently");
    }

    #[test]
    fn test_new_launcher_dirs() {
        if let Ok(dirs) = LauncherDirs::new() {
            assert!(dirs.config_file().ends_with("config.toml"));
            assert!(dirs.instances_dir().ends_with("instances"));
        }
    }
}
