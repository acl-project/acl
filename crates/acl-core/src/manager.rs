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

        info!("Loading instances configuration from {}", path.display());

        let content =
            fs::read_to_string(path.clone()).map_err(|source| CoreError::Io { path, source })?;

        if let Ok(config) = toml::from_str(&content) {
            mgr.config = config;
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
