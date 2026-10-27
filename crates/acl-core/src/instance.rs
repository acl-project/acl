use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{CoreError, Result};
use serde::{Deserialize, Serialize};
use tracing::info;

const CONFIG_NAME: &str = "aclcfg.toml";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModLoader {
    #[default]
    Vanilla,
    Fabric,
    Quilt,
    Forge,
    NeoForge,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InstanceConfig {
    /// Unique directory identifier for the instance (prevent path traversal)
    pub id: String,
    /// Display name of the instance
    pub name: String,
    /// Minecraft version name (e.g., 1.21)
    pub game_version: String,
    /// Type of mod loader
    #[serde(default)]
    pub mod_loader: ModLoader,
    /// Version of the mod loader; None if vanilla
    #[serde(default)]
    pub mod_loader_version: Option<String>,
    /// Instance-level Java path; falls back to the global configuration if absent
    #[serde(default)]
    pub java_path: Option<PathBuf>,
    /// Instance-level minimum memory; falls back to the global configuration if absent
    #[serde(default)]
    pub min_memory_mb: Option<u32>,
    /// Instance-level maximum memory; falls back to the global configuration if absent
    #[serde(default)]
    pub max_memory_mb: Option<u32>,
    /// Additional JVM arguments
    #[serde(default)]
    pub extra_jvm_args: Vec<String>,
    /// Timestamp when the instance was created
    #[serde(default)]
    pub created_at: u64,
    /// Timestamp of the last launch
    #[serde(default)]
    pub last_play_at: u64,
}

impl InstanceConfig {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        game_version: impl Into<String>,
    ) -> Result<Self> {
        let id = id.into();

        Self::validate_instance_id(&id)?;

        Ok(Self {
            id,
            name: name.into(),
            game_version: game_version.into(),
            created_at: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
            ..Default::default()
        })
    }

    pub fn load(path: &Path) -> Result<Self> {
        info!("Loading instance config from {}", path.display());

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

        info!("Instance config saved to {}", path.display());
        Ok(())
    }

    pub fn validate_instance_id(id: &str) -> Result<()> {
        if id.trim().is_empty() {
            return Err(CoreError::InvalidInstanceId {
                id: id.to_string(),
                reason: "Instance ID cannot be empty".to_string(),
            });
        }

        if id != id.trim() {
            return Err(CoreError::InvalidInstanceId {
                id: id.to_string(),
                reason: "Instance ID cannot have leading or trailing whitespace".to_string(),
            });
        }

        if id == "." || id == ".." {
            return Err(CoreError::InvalidInstanceId {
                id: id.to_string(),
                reason: "Instance ID cannot be '.' or '..'".to_string(),
            });
        }

        const FORBIDDEN_CHARS: &[char] = &['/', '\\', '<', '>', ':', '"', '|', '?', '*', '\0'];
        if id
            .chars()
            .any(|c| FORBIDDEN_CHARS.contains(&c) || c.is_control())
        {
            return Err(CoreError::InvalidInstanceId {
                id: id.to_string(),
                reason: "Instance ID contains invalid path characters".to_string(),
            });
        }

        let mut components = Path::new(id).components();
        match (components.next(), components.next()) {
            (Some(std::path::Component::Normal(_)), None) => Ok(()),
            _ => Err(CoreError::InvalidInstanceId {
                id: id.to_string(),
                reason: "Instance ID must be a single normal directory name".to_string(),
            }),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Instance {
    /// The root directory of this instance (e.g., <versions_dir>/<id>).
    pub dir: PathBuf,
    pub config: InstanceConfig,
}

impl Instance {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        game_version: impl Into<String>,
        version_dir: &Path,
    ) -> Result<Self> {
        let config = InstanceConfig::new(id, name, game_version)?;

        Ok(Self {
            dir: version_dir.join(&config.id),
            config,
        })
    }

    pub fn from_config(config: InstanceConfig, version_dir: &Path) -> Self {
        Self {
            dir: version_dir.join(&config.id),
            config,
        }
    }

    pub fn load(id: &str, version_dir: &Path) -> Result<Self> {
        InstanceConfig::validate_instance_id(id)?;

        let config_path = version_dir.join(id).join(CONFIG_NAME);
        let config = InstanceConfig::load(&config_path)?;

        if config.id != id {
            return Err(CoreError::InvalidInstanceId {
                id: id.to_owned(),
                reason: format!(
                    "Instance config id {} is not equal with id {}",
                    config.id, id
                ),
            });
        }

        Ok(Instance::from_config(config, version_dir))
    }

    pub fn save(&self) -> Result<()> {
        self.config.save(&self.config_file())
    }

    pub fn game_dir(&self) -> &Path {
        &self.dir
    }

    pub fn config_file(&self) -> PathBuf {
        self.dir.join(CONFIG_NAME)
    }

    pub fn client_jar(&self) -> PathBuf {
        self.dir.join(format!("{}.jar", self.config.id))
    }

    pub fn version_json(&self) -> PathBuf {
        self.dir.join(format!("{}.json", self.config.id))
    }

    pub fn natives_dir(&self) -> PathBuf {
        self.dir.join(format!(
            "natives-{}-{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        ))
    }

    pub fn saves_dir(&self) -> PathBuf {
        self.dir.join("saves")
    }

    pub fn mods_dir(&self) -> PathBuf {
        self.dir.join("mods")
    }

    pub fn resourcepacks_dir(&self) -> PathBuf {
        self.dir.join("resourcepacks")
    }
}
