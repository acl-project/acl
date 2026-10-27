use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{CoreError, Result, java::JavaInstallation};
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
    pub java: Option<JavaInstallation>,
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

    pub fn load_from_dir(instance_dir: &Path) -> Result<Self> {
        let config_path = instance_dir.join(CONFIG_NAME);
        let config = InstanceConfig::load(&config_path)?;

        Ok(Self {
            dir: instance_dir.to_path_buf(),
            config,
        })
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

    pub fn id(&self) -> &str {
        &self.config.id
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
                "acl_test_inst_{}_{}_{}",
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
    fn test_validate_instance_id_valid() {
        assert!(InstanceConfig::validate_instance_id("vanilla-1.21").is_ok());
        assert!(InstanceConfig::validate_instance_id("my_instance").is_ok());
        assert!(InstanceConfig::validate_instance_id("1.20.4-fabric").is_ok());
        assert!(InstanceConfig::validate_instance_id("modpack.v1").is_ok());
        assert!(InstanceConfig::validate_instance_id("NormalName123").is_ok());
    }

    #[test]
    fn test_validate_instance_id_invalid() {
        assert!(InstanceConfig::validate_instance_id("").is_err());
        assert!(InstanceConfig::validate_instance_id("   ").is_err());
        assert!(InstanceConfig::validate_instance_id(" leading").is_err());
        assert!(InstanceConfig::validate_instance_id("trailing ").is_err());
        assert!(InstanceConfig::validate_instance_id(".").is_err());
        assert!(InstanceConfig::validate_instance_id("..").is_err());
        assert!(InstanceConfig::validate_instance_id("foo/bar").is_err());
        assert!(InstanceConfig::validate_instance_id("foo\\bar").is_err());
        assert!(InstanceConfig::validate_instance_id("foo:bar").is_err());
        assert!(InstanceConfig::validate_instance_id("foo*bar").is_err());
        assert!(InstanceConfig::validate_instance_id("foo?bar").is_err());
        assert!(InstanceConfig::validate_instance_id("foo\"bar").is_err());
        assert!(InstanceConfig::validate_instance_id("foo<bar").is_err());
        assert!(InstanceConfig::validate_instance_id("foo>bar").is_err());
        assert!(InstanceConfig::validate_instance_id("foo|bar").is_err());
        assert!(InstanceConfig::validate_instance_id("foo\0bar").is_err());
        assert!(InstanceConfig::validate_instance_id("foo\nbar").is_err());
    }

    #[test]
    fn test_mod_loader_serde_and_default() {
        assert_eq!(ModLoader::default(), ModLoader::Vanilla);

        #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
        struct Wrapper {
            loader: ModLoader,
        }

        let loaders = [
            (ModLoader::Vanilla, "loader = \"vanilla\"\n"),
            (ModLoader::Fabric, "loader = \"fabric\"\n"),
            (ModLoader::Quilt, "loader = \"quilt\"\n"),
            (ModLoader::Forge, "loader = \"forge\"\n"),
            (ModLoader::NeoForge, "loader = \"neoforge\"\n"),
        ];

        for (loader, expected_toml) in loaders {
            let wrapped = Wrapper { loader };
            let serialized = toml::to_string(&wrapped).expect("failed to serialize ModLoader");
            assert_eq!(serialized, expected_toml);

            let deserialized: Wrapper =
                toml::from_str(&serialized).expect("failed to deserialize ModLoader");
            assert_eq!(deserialized, wrapped);
        }

        let invalid_toml = "loader = \"unknown_loader\"\n";
        assert!(toml::from_str::<Wrapper>(invalid_toml).is_err());
    }

    #[test]
    fn test_instance_config_new_and_defaults() {
        let config = InstanceConfig::new("test-inst", "Test Instance", "1.21")
            .expect("should create instance config");
        assert_eq!(config.id, "test-inst");
        assert_eq!(config.name, "Test Instance");
        assert_eq!(config.game_version, "1.21");
        assert_eq!(config.mod_loader, ModLoader::Vanilla);
        assert_eq!(config.mod_loader_version, None);
        assert_eq!(config.java, None);
        assert_eq!(config.min_memory_mb, None);
        assert_eq!(config.max_memory_mb, None);
        assert!(config.extra_jvm_args.is_empty());
        assert!(config.created_at > 0);
        assert_eq!(config.last_play_at, 0);

        let err = InstanceConfig::new("../invalid", "Invalid", "1.21");
        assert!(matches!(err, Err(CoreError::InvalidInstanceId { .. })));
    }

    #[test]
    fn test_instance_config_save_and_load() {
        let temp = TempDirGuard::new("save_load_config");
        let cfg_path = temp.path().join(CONFIG_NAME);

        let mut config = InstanceConfig::new("save-load-id", "Save Load Instance", "1.21.1")
            .expect("should create config");
        config.mod_loader = ModLoader::Fabric;
        config.mod_loader_version = Some("0.16.0".to_string());
        config.min_memory_mb = Some(2048);
        config.max_memory_mb = Some(8192);
        config.extra_jvm_args = vec!["-Xmx8G".to_string(), "-Dfoo=bar".to_string()];

        config.save(&cfg_path).expect("failed to save config");
        assert!(cfg_path.exists());

        let loaded = InstanceConfig::load(&cfg_path).expect("failed to load config");
        assert_eq!(loaded.id, "save-load-id");
        assert_eq!(loaded.name, "Save Load Instance");
        assert_eq!(loaded.game_version, "1.21.1");
        assert_eq!(loaded.mod_loader, ModLoader::Fabric);
        assert_eq!(loaded.mod_loader_version.as_deref(), Some("0.16.0"));
        assert_eq!(loaded.min_memory_mb, Some(2048));
        assert_eq!(loaded.max_memory_mb, Some(8192));
        assert_eq!(loaded.extra_jvm_args, vec!["-Xmx8G", "-Dfoo=bar"]);

        let non_existent = temp.path().join("missing.toml");
        assert!(matches!(
            InstanceConfig::load(&non_existent),
            Err(CoreError::Io { .. })
        ));
    }

    #[test]
    fn test_instance_new_and_from_config() {
        let base_dir = Path::new("/custom/versions");
        let instance = Instance::new("inst-1", "Instance One", "1.21", base_dir)
            .expect("should create instance");

        assert_eq!(instance.dir, base_dir.join("inst-1"));
        assert_eq!(instance.game_dir(), base_dir.join("inst-1"));
        assert_eq!(
            instance.config_file(),
            base_dir.join("inst-1").join(CONFIG_NAME)
        );
        assert_eq!(
            instance.client_jar(),
            base_dir.join("inst-1").join("inst-1.jar")
        );
        assert_eq!(
            instance.version_json(),
            base_dir.join("inst-1").join("inst-1.json")
        );
        assert_eq!(instance.saves_dir(), base_dir.join("inst-1").join("saves"));
        assert_eq!(instance.mods_dir(), base_dir.join("inst-1").join("mods"));
        assert_eq!(
            instance.resourcepacks_dir(),
            base_dir.join("inst-1").join("resourcepacks")
        );
        let expected_natives = format!(
            "natives-{}-{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        );
        assert_eq!(
            instance.natives_dir(),
            base_dir.join("inst-1").join(expected_natives)
        );

        let from_cfg = Instance::from_config(instance.config.clone(), base_dir);
        assert_eq!(from_cfg.dir, instance.dir);
        assert_eq!(from_cfg.config.id, instance.config.id);
    }

    #[test]
    fn test_instance_load_from_dir_and_save() {
        let temp = TempDirGuard::new("instance_load_save");
        let version_dir = temp.path();

        let instance = Instance::new("my_inst", "My Instance", "1.21", version_dir)
            .expect("should create instance");
        instance.save().expect("should save instance config");

        let loaded = Instance::load_from_dir(&instance.dir).expect("should load from dir");
        assert_eq!(loaded.dir, instance.dir);
        assert_eq!(loaded.config.id, "my_inst");
        assert_eq!(loaded.config.name, "My Instance");
        assert_eq!(loaded.client_jar(), instance.dir.join("my_inst.jar"));
        assert_eq!(loaded.version_json(), instance.dir.join("my_inst.json"));
    }

    #[test]
    fn test_instance_renamed_folder_compatibility() {
        let temp = TempDirGuard::new("renamed_folder");
        let original_dir = temp.path().join("original_folder_name");

        let config = InstanceConfig::new("fabric-1.21", "Renamed Test Instance", "1.21")
            .expect("should create config");
        let instance = Instance {
            dir: original_dir.clone(),
            config,
        };
        instance.save().expect("should save config in original dir");

        let renamed_dir = temp.path().join("player_renamed_pack");
        fs::rename(&original_dir, &renamed_dir).expect("should rename directory");

        let loaded = Instance::load_from_dir(&renamed_dir)
            .expect("should load successfully from renamed folder");
        assert_eq!(loaded.dir, renamed_dir);
        assert_eq!(loaded.config.id, "fabric-1.21");
        assert_eq!(loaded.client_jar(), renamed_dir.join("fabric-1.21.jar"));
        assert_eq!(loaded.version_json(), renamed_dir.join("fabric-1.21.json"));
        assert_eq!(loaded.saves_dir(), renamed_dir.join("saves"));
    }
}
