use std::fs;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;
use tracing::info;

use crate::CoreError;
use crate::Instance;
use crate::Result;

const CONFIG_NAME: &str = "instances.toml";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InstancesConfig {
    /// ID of the currently selected instance
    pub active_id: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct InstanceManager {
    /// Root directory path where instances are located
    pub versions_dir: PathBuf,
    /// List of currently scanned instances (sorted by last_play_at in descending order)
    pub instances: Vec<Instance>,
    /// The directory where the configuration file is located.
    pub config_dir: PathBuf,
    /// Configuration file for instance management.
    pub config: InstancesConfig,
}

impl InstanceManager {
    /// Update the instance list and save it.
    pub fn refresh(&mut self) -> Result<()> {
        self.instances = Instance::discover_all(&self.versions_dir)?;

        info!(
            "Find {} versions in {}",
            self.instances.len(),
            self.versions_dir.display()
        );

        if self.config.active_id.is_none() {
            self.config.active_id = self.instances.first().map(|ins| ins.id().to_string());
        }

        self.save()?;

        Ok(())
    }

    pub fn load(versions_dir: PathBuf, config_dir: PathBuf) -> Result<Self> {
        let mut mgr = Self {
            versions_dir,
            instances: Vec::new(),
            config_dir,
            config: Default::default(),
        };

        let path = mgr.config_file();

        if path.exists() {
            info!("Loading instances configuration from {}", path.display());

            let content = fs::read_to_string(path.clone())
                .map_err(|source| CoreError::Io { path, source })?;

            if let Ok(config) = toml::from_str(&content) {
                mgr.config = config;
            }
        }

        mgr.refresh()?;

        Ok(mgr)
    }

    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join(CONFIG_NAME)
    }

    pub fn save(&self) -> Result<()> {
        let content = toml::to_string_pretty(&self.config)?;
        let path = self.config_file();
        acl_common::fs::write_atomic(path.clone(), content.as_bytes()).map_err(|source| {
            CoreError::Io {
                path: path.clone(),
                source,
            }
        })?;

        info!("Instances configuration saved to {}", path.display());
        Ok(())
    }

    /// Set the currently selected instance.
    pub fn set_active_instance(&mut self, id: &str) -> Result<&Instance> {
        self.instances
            .iter()
            .position(|instance| instance.id() == id)
            .map(|i| {
                self.config.active_id = Some(id.to_string());
                &self.instances[i]
            })
            .ok_or(CoreError::InstanceNotFound { id: id.to_owned() })
    }

    /// Delete the instance and its local files
    /// If the instance is the currently selected one, switch to the first instance by most recent play time.
    pub fn remove_instance(&mut self, id: &str) -> Result<()> {
        let instance = self
            .instances
            .iter()
            .position(|instance| instance.id() == id)
            .map(|i| self.instances.remove(i))
            .ok_or(CoreError::InstanceNotFound { id: id.to_owned() })?;

        if self.config.active_id == Some(id.to_string()) {
            if let Some(first) = self.instances.first() {
                self.config.active_id = Some(first.id().to_owned());
            } else {
                self.config.active_id = None;
            }
        }
        instance.delete()
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
                "acl_test_mgr_{}_{}_{}",
                name,
                std::process::id(),
                id
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("failed to create temp test dir");
            Self { path }
        }

        fn path(&self) -> &PathBuf {
            &self.path
        }
    }

    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn test_manager_config_file_path() {
        let guard = TempDirGuard::new("config_path");
        let mgr = InstanceManager {
            config_dir: guard.path().clone(),
            ..Default::default()
        };
        assert_eq!(mgr.config_file(), guard.path().join("instances.toml"));
    }

    #[test]
    fn test_manager_load_creates_default_when_missing() {
        let temp = TempDirGuard::new("load_missing");
        let versions_dir = temp.path().join("versions");
        let config_dir = temp.path().join("config");
        fs::create_dir_all(&versions_dir).unwrap();
        fs::create_dir_all(&config_dir).unwrap();

        let mgr = InstanceManager::load(versions_dir.clone(), config_dir.clone()).unwrap();
        assert_eq!(mgr.versions_dir, versions_dir);
        assert_eq!(mgr.config_dir, config_dir);
        assert!(mgr.instances.is_empty());
        assert_eq!(mgr.config.active_id, None);
        assert!(mgr.config_file().exists());
    }

    #[test]
    fn test_manager_load_existing_config() {
        let temp = TempDirGuard::new("load_existing");
        let versions_dir = temp.path().join("versions");
        let config_dir = temp.path().join("config");
        fs::create_dir_all(&versions_dir).unwrap();
        fs::create_dir_all(&config_dir).unwrap();

        let config_content = "active_id = \"preset-id\"\n";
        fs::write(config_dir.join("instances.toml"), config_content).unwrap();

        let mgr = InstanceManager::load(versions_dir, config_dir).unwrap();
        assert_eq!(mgr.config.active_id, Some("preset-id".to_string()));
    }

    #[test]
    fn test_manager_refresh_auto_selects_first_instance() {
        let temp = TempDirGuard::new("refresh_select");
        let versions_dir = temp.path().join("versions");
        let config_dir = temp.path().join("config");
        fs::create_dir_all(&versions_dir).unwrap();
        fs::create_dir_all(&config_dir).unwrap();

        let _ = Instance::create("inst1", "Instance One", "1.20.4", &versions_dir).unwrap();
        let _ = Instance::create("inst2", "Instance Two", "1.21", &versions_dir).unwrap();

        let mgr = InstanceManager::load(versions_dir, config_dir).unwrap();
        assert_eq!(mgr.instances.len(), 2);
        assert!(mgr.config.active_id.is_some());
        let active = mgr.config.active_id.as_deref().unwrap();
        assert!(active == "inst1" || active == "inst2");
    }

    #[test]
    fn test_manager_set_active_instance_success_and_not_found() {
        let temp = TempDirGuard::new("set_active");
        let versions_dir = temp.path().join("versions");
        let config_dir = temp.path().join("config");
        fs::create_dir_all(&versions_dir).unwrap();
        fs::create_dir_all(&config_dir).unwrap();

        let _ = Instance::create("inst1", "Instance One", "1.20.4", &versions_dir).unwrap();
        let _ = Instance::create("inst2", "Instance Two", "1.21", &versions_dir).unwrap();

        let mut mgr = InstanceManager::load(versions_dir, config_dir).unwrap();
        let selected = mgr.set_active_instance("inst2").unwrap();
        assert_eq!(selected.id(), "inst2");
        assert_eq!(mgr.config.active_id, Some("inst2".to_string()));

        let err = mgr.set_active_instance("nonexistent").unwrap_err();
        match err {
            CoreError::InstanceNotFound { id } => assert_eq!(id, "nonexistent"),
            other => panic!("Unexpected error: {other:?}"),
        }
    }

    #[test]
    fn test_manager_remove_active_instance_fallback() {
        let temp = TempDirGuard::new("remove_fallback");
        let versions_dir = temp.path().join("versions");
        let config_dir = temp.path().join("config");
        fs::create_dir_all(&versions_dir).unwrap();
        fs::create_dir_all(&config_dir).unwrap();

        let _ = Instance::create("inst1", "Instance One", "1.20.4", &versions_dir).unwrap();
        let _ = Instance::create("inst2", "Instance Two", "1.21", &versions_dir).unwrap();

        let mut mgr = InstanceManager::load(versions_dir.clone(), config_dir).unwrap();
        mgr.set_active_instance("inst1").unwrap();
        assert_eq!(mgr.config.active_id, Some("inst1".to_string()));

        mgr.remove_instance("inst1").unwrap();
        assert_eq!(mgr.instances.len(), 1);
        assert_eq!(mgr.instances[0].id(), "inst2");
        assert_eq!(mgr.config.active_id, Some("inst2".to_string()));
        assert!(!versions_dir.join("inst1").exists());
        assert!(versions_dir.join("inst2").exists());
    }

    #[test]
    fn test_manager_remove_last_instance_clears_active() {
        let temp = TempDirGuard::new("remove_last");
        let versions_dir = temp.path().join("versions");
        let config_dir = temp.path().join("config");
        fs::create_dir_all(&versions_dir).unwrap();
        fs::create_dir_all(&config_dir).unwrap();

        let _ = Instance::create("inst1", "Instance One", "1.20.4", &versions_dir).unwrap();

        let mut mgr = InstanceManager::load(versions_dir.clone(), config_dir).unwrap();
        assert_eq!(mgr.config.active_id, Some("inst1".to_string()));

        mgr.remove_instance("inst1").unwrap();
        assert!(mgr.instances.is_empty());
        assert_eq!(mgr.config.active_id, None);
        assert!(!versions_dir.join("inst1").exists());
    }

    #[test]
    fn test_manager_remove_non_active_instance_preserves_active() {
        let temp = TempDirGuard::new("remove_non_active");
        let versions_dir = temp.path().join("versions");
        let config_dir = temp.path().join("config");
        fs::create_dir_all(&versions_dir).unwrap();
        fs::create_dir_all(&config_dir).unwrap();

        let _ = Instance::create("inst1", "Instance One", "1.20.4", &versions_dir).unwrap();
        let _ = Instance::create("inst2", "Instance Two", "1.21", &versions_dir).unwrap();

        let mut mgr = InstanceManager::load(versions_dir.clone(), config_dir).unwrap();
        mgr.set_active_instance("inst1").unwrap();

        mgr.remove_instance("inst2").unwrap();
        assert_eq!(mgr.instances.len(), 1);
        assert_eq!(mgr.config.active_id, Some("inst1".to_string()));
        assert!(!versions_dir.join("inst2").exists());
    }

    #[test]
    fn test_manager_remove_nonexistent_instance_fails() {
        let temp = TempDirGuard::new("remove_nonexistent");
        let versions_dir = temp.path().join("versions");
        let config_dir = temp.path().join("config");
        fs::create_dir_all(&versions_dir).unwrap();
        fs::create_dir_all(&config_dir).unwrap();

        let mut mgr = InstanceManager::load(versions_dir, config_dir).unwrap();
        let err = mgr.remove_instance("missing").unwrap_err();
        match err {
            CoreError::InstanceNotFound { id } => assert_eq!(id, "missing"),
            other => panic!("Unexpected error: {other:?}"),
        }
    }

    #[test]
    fn test_manager_save_atomic_persists_config() {
        let temp = TempDirGuard::new("save_atomic");
        let versions_dir = temp.path().join("versions");
        let config_dir = temp.path().join("config");
        fs::create_dir_all(&versions_dir).unwrap();
        fs::create_dir_all(&config_dir).unwrap();

        let mut mgr = InstanceManager::load(versions_dir, config_dir.clone()).unwrap();
        mgr.config.active_id = Some("saved-instance".to_string());
        mgr.save().unwrap();

        let loaded_content = fs::read_to_string(config_dir.join("instances.toml")).unwrap();
        let parsed: InstancesConfig = toml::from_str(&loaded_content).unwrap();
        assert_eq!(parsed.active_id, Some("saved-instance".to_string()));
    }
}
