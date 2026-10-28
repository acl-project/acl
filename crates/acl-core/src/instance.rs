use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{CoreError, LauncherDirs, Result, java::JavaInstallation};
use acl_minecraft::{AssetIndex, VersionEntry, VersionMetadata, download::DownloadTask};
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
    /// Instance-level Java; falls back to the global configuration if absent
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

    pub fn save_config(&self) -> Result<()> {
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

    pub fn create(
        id: impl Into<String>,
        name: impl Into<String>,
        game_version: impl Into<String>,
        version_dir: &Path,
    ) -> Result<Self> {
        let id = id.into();
        let instance = Instance::new(id.clone(), name, game_version, version_dir)?;
        if instance.dir.exists() {
            return Err(CoreError::InstanceAlreadyExists { id });
        }

        instance.save_config()?;

        Ok(instance)
    }

    pub fn discover_all(version_dir: &Path) -> Result<Vec<Self>> {
        let mut instances = Vec::new();

        if !version_dir.exists() {
            return Ok(instances);
        }

        let read_dir = fs::read_dir(version_dir).map_err(|source| CoreError::Io {
            path: version_dir.to_path_buf(),
            source,
        })?;

        for entry in read_dir {
            let entry = entry.map_err(|source| CoreError::Io {
                path: version_dir.to_path_buf(),
                source,
            })?;

            let path = entry.path();

            if !path.is_dir() {
                continue;
            }

            let config = path.join(CONFIG_NAME);
            if !config.exists() {
                continue;
            }

            if let Ok(instance) = Instance::load_from_dir(&path) {
                instances.push(instance);
            }
        }

        instances.sort_by_key(|a| std::cmp::Reverse(a.config.last_play_at));

        Ok(instances)
    }

    pub fn delete(&self) -> Result<()> {
        // If the directory does not exist, return directly.
        if !self.dir.exists() {
            return Ok(());
        }

        fs::remove_dir_all(&self.dir).map_err(|source| CoreError::Io {
            path: self.dir.clone(),
            source,
        })
    }

    pub fn update_last_play(&mut self) -> Result<()> {
        self.config.last_play_at = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        self.save_config()?;
        Ok(())
    }

    /// Get the metadata for the current instance version.
    pub async fn fetch_metadata(
        &self,
        client: &reqwest::Client,
        entry: &VersionEntry,
        dirs: &LauncherDirs,
    ) -> Result<VersionMetadata> {
        Ok(
            VersionMetadata::fetch_and_verify(client, entry, self.id(), &dirs.versions_dir())
                .await?,
        )
    }

    /// Get the download tasks required for the current instance version.
    pub fn download_tasks(
        &self,
        metadata: &VersionMetadata,
        asset_index: &AssetIndex,
        dirs: &LauncherDirs,
    ) -> Vec<DownloadTask> {
        let mut tasks = Vec::new();
        tasks.push(metadata.client_download_task(self.id(), &dirs.versions_dir()));
        tasks.extend(metadata.library_download_tasks(&dirs.libraries_dir()));
        tasks.extend(asset_index.asset_download_tasks(&dirs.assets_dir()));
        tasks
    }

    /// Map resource files and extract native libraries.
    pub fn prepare_runtime(
        &self,
        metadata: &VersionMetadata,
        asset_index: &AssetIndex,
        dirs: &LauncherDirs,
    ) -> Result<()> {
        asset_index.reconstruct_legacy_assets(
            &metadata.asset_index.id,
            &dirs.assets_dir(),
            self.game_dir(),
        )?;

        metadata.extract_natives(&dirs.libraries_dir(), &self.natives_dir())?;

        Ok(())
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
        instance.save_config().expect("should save instance config");

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
        instance
            .save_config()
            .expect("should save config in original dir");

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

    #[test]
    fn test_instance_create_and_already_exists() {
        let temp = TempDirGuard::new("instance_create");
        let version_dir = temp.path();

        let instance = Instance::create("inst_create", "Created Instance", "1.21.1", version_dir)
            .expect("should create new instance");
        assert_eq!(instance.id(), "inst_create");
        assert!(instance.dir.exists());
        assert!(instance.config_file().exists());

        let loaded =
            Instance::load_from_dir(&instance.dir).expect("should load created instance from disk");
        assert_eq!(loaded.id(), "inst_create");
        assert_eq!(loaded.config.name, "Created Instance");
        assert_eq!(loaded.config.game_version, "1.21.1");

        let duplicate = Instance::create("inst_create", "Duplicate", "1.21.1", version_dir);
        assert!(matches!(
            duplicate,
            Err(CoreError::InstanceAlreadyExists { id }) if id == "inst_create"
        ));
    }

    #[test]
    fn test_instance_discover_all() {
        let temp = TempDirGuard::new("instance_discover");
        let non_existent = temp.path().join("non_existent_versions");

        let empty =
            Instance::discover_all(&non_existent).expect("non-existent dir should return empty");
        assert!(empty.is_empty());

        let version_dir = temp.path().join("versions");
        fs::create_dir_all(&version_dir).expect("failed to create versions dir");

        // Regular file in versions_dir should be ignored
        fs::write(version_dir.join("README.txt"), b"ignore me").expect("failed to write file");

        // Directory without aclcfg.toml should be ignored
        fs::create_dir_all(version_dir.join("unconfigured_dir"))
            .expect("failed to create unconfigured dir");

        // Directory with corrupted aclcfg.toml should be skipped gracefully
        let broken_dir = version_dir.join("broken_inst");
        fs::create_dir_all(&broken_dir).expect("failed to create broken dir");
        fs::write(broken_dir.join(CONFIG_NAME), b"invalid = [toml")
            .expect("failed to write broken config");

        // Valid instances with different last_play_at timestamps
        let mut inst_older = Instance::create("older_inst", "Older", "1.20.1", &version_dir)
            .expect("failed to create older_inst");
        inst_older.config.last_play_at = 100;
        inst_older
            .save_config()
            .expect("failed to save older_inst config");

        let mut inst_newer = Instance::create("newer_inst", "Newer", "1.21.1", &version_dir)
            .expect("failed to create newer_inst");
        inst_newer.config.last_play_at = 500;
        inst_newer
            .save_config()
            .expect("failed to save newer_inst config");

        let discovered = Instance::discover_all(&version_dir).expect("discover_all should succeed");
        assert_eq!(discovered.len(), 2);
        assert_eq!(discovered[0].id(), "newer_inst");
        assert_eq!(discovered[1].id(), "older_inst");
    }

    #[test]
    fn test_instance_delete() {
        let temp = TempDirGuard::new("instance_delete");
        let version_dir = temp.path();

        let instance = Instance::create("to_delete", "Delete Me", "1.21", version_dir)
            .expect("should create instance");
        assert!(instance.dir.exists());

        instance.delete().expect("should delete existing instance");
        assert!(!instance.dir.exists());

        // Deleting again when directory does not exist should be a no-op
        instance
            .delete()
            .expect("deleting non-existent instance should succeed");
    }

    #[test]
    fn test_instance_update_last_play() {
        let temp = TempDirGuard::new("instance_update_last_play");
        let version_dir = temp.path();

        let mut instance = Instance::create("play_inst", "Play Instance", "1.21", version_dir)
            .expect("should create instance");
        assert_eq!(instance.config.last_play_at, 0);

        instance
            .update_last_play()
            .expect("should update last_play_at");
        assert!(instance.config.last_play_at > 0);

        let reloaded =
            Instance::load_from_dir(&instance.dir).expect("should reload instance config");
        assert_eq!(reloaded.config.last_play_at, instance.config.last_play_at);
    }

    fn sample_version_metadata() -> VersionMetadata {
        use acl_minecraft::{
            AssetIndexInfo, DownloadFile, Library, LibraryArtifact, LibraryDownloads,
            VersionDownloads, VersionType,
        };

        let lib = Library {
            name: "com.mojang:authlib:6.0.55".to_string(),
            downloads: Some(LibraryDownloads {
                artifact: Some(LibraryArtifact {
                    path: Some("com/mojang/authlib/6.0.55/authlib-6.0.55.jar".to_string()),
                    sha1: "1111222233334444555566667777888899990000".to_string(),
                    size: 1024,
                    url: "https://libraries.minecraft.net/authlib.jar".to_string(),
                }),
                classifiers: None,
            }),
            natives: None,
            rules: None,
            extract: None,
        };

        VersionMetadata {
            id: "1.21.1".to_string(),
            version_type: VersionType::Release,
            main_class: "net.minecraft.client.main.Main".to_string(),
            minecraft_arguments: None,
            arguments: None,
            assets: Some("17".to_string()),
            asset_index: AssetIndexInfo {
                id: "17".to_string(),
                sha1: "0123456789abcdef0123456789abcdef01234567".to_string(),
                size: 100,
                total_size: 200,
                url: "https://example.com/assets/17.json".to_string(),
            },
            downloads: VersionDownloads {
                client: DownloadFile {
                    sha1: "abcdef0123456789abcdef0123456789abcdef01".to_string(),
                    size: 4096,
                    url: "https://example.com/client.jar".to_string(),
                },
                server: None,
            },
            java_version: None,
            libraries: vec![lib],
        }
    }

    #[tokio::test]
    async fn test_instance_fetch_metadata_from_cache() {
        use acl_minecraft::{VersionType, cache_key_for_url};

        let temp = TempDirGuard::new("instance_fetch_metadata");
        let dirs = LauncherDirs::with_paths(temp.path().join("config"), temp.path().join("data"));

        let instance = Instance::create("cached_inst", "Cached", "1.21.1", &dirs.versions_dir())
            .expect("should create instance");

        let json_payload = r#"{
            "id": "1.21.1",
            "type": "release",
            "mainClass": "net.minecraft.client.main.Main",
            "assetIndex": {
                "id": "17",
                "sha1": "0123456789abcdef0123456789abcdef01234567",
                "size": 100,
                "totalSize": 200,
                "url": "https://example.com/assets/17.json"
            },
            "downloads": {
                "client": {
                    "sha1": "abcdef0123456789abcdef0123456789abcdef01",
                    "size": 4096,
                    "url": "https://example.com/client.jar"
                }
            },
            "libraries": []
        }"#;

        fs::write(instance.version_json(), json_payload).expect("failed to write cached metadata");
        let sha1 = cache_key_for_url(json_payload)
            .trim_end_matches(".json")
            .to_string();

        let entry = VersionEntry {
            id: "1.21.1".to_string(),
            version_type: VersionType::Release,
            url: "http://127.0.0.1:9/unused".to_string(),
            time: "2024-08-08T00:00:00+00:00".to_string(),
            release_time: "2024-08-08T00:00:00+00:00".to_string(),
            sha1,
            compliance_level: 1,
        };

        let client = reqwest::Client::new();
        let metadata = instance
            .fetch_metadata(&client, &entry, &dirs)
            .await
            .expect("should load metadata from local cache");

        assert_eq!(metadata.id, "1.21.1");
        assert_eq!(metadata.main_class, "net.minecraft.client.main.Main");
        assert_eq!(metadata.downloads.client.size, 4096);
    }

    #[test]
    fn test_instance_download_tasks() {
        use acl_minecraft::AssetObject;
        use std::collections::HashMap;

        let dirs =
            LauncherDirs::with_paths(PathBuf::from("/mock/config"), PathBuf::from("/mock/data"));
        let instance = Instance::new(
            "dl_inst",
            "Download Instance",
            "1.21.1",
            &dirs.versions_dir(),
        )
        .expect("should create instance");

        let metadata = sample_version_metadata();
        let mut objects = HashMap::new();
        objects.insert(
            "icons/icon_16x16.png".to_string(),
            AssetObject {
                hash: "bdf48ef6b5d0d23ebb4f8a6179d4a1bfab1a9598".to_string(),
                size: 512,
            },
        );
        let asset_index = AssetIndex {
            objects,
            map_to_resources: false,
            is_virtual: false,
        };

        let tasks = instance.download_tasks(&metadata, &asset_index, &dirs);
        // 1 client jar + 1 library + 1 asset object
        assert_eq!(tasks.len(), 3);

        assert_eq!(tasks[0].name, "dl_inst.jar");
        assert_eq!(tasks[0].destination, instance.client_jar());
        assert_eq!(tasks[0].size, 4096);

        assert_eq!(tasks[1].name, "com.mojang:authlib:6.0.55");
        assert_eq!(
            tasks[1].destination,
            dirs.libraries_dir()
                .join("com/mojang/authlib/6.0.55/authlib-6.0.55.jar")
        );

        assert_eq!(tasks[2].name, "icons/icon_16x16.png");
        assert_eq!(
            tasks[2].destination,
            dirs.assets_dir()
                .join("objects/bd/bdf48ef6b5d0d23ebb4f8a6179d4a1bfab1a9598")
        );
    }

    fn build_stored_zip(entry_name: &str, content: &[u8], crc32: u32) -> Vec<u8> {
        let name_bytes = entry_name.as_bytes();
        let name_len = name_bytes.len() as u16;
        let data_len = content.len() as u32;

        let mut buf = Vec::new();
        // Local file header
        buf.extend_from_slice(b"PK\x03\x04");
        buf.extend_from_slice(&20u16.to_le_bytes()); // version needed
        buf.extend_from_slice(&0u16.to_le_bytes()); // flags
        buf.extend_from_slice(&0u16.to_le_bytes()); // compression method: stored
        buf.extend_from_slice(&0u16.to_le_bytes()); // mod time
        buf.extend_from_slice(&0u16.to_le_bytes()); // mod date
        buf.extend_from_slice(&crc32.to_le_bytes());
        buf.extend_from_slice(&data_len.to_le_bytes());
        buf.extend_from_slice(&data_len.to_le_bytes());
        buf.extend_from_slice(&name_len.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes()); // extra len
        buf.extend_from_slice(name_bytes);
        buf.extend_from_slice(content);

        let cd_offset = buf.len() as u32;
        // Central directory header
        buf.extend_from_slice(b"PK\x01\x02");
        buf.extend_from_slice(&20u16.to_le_bytes()); // version made by
        buf.extend_from_slice(&20u16.to_le_bytes()); // version needed
        buf.extend_from_slice(&0u16.to_le_bytes()); // flags
        buf.extend_from_slice(&0u16.to_le_bytes()); // method: stored
        buf.extend_from_slice(&0u16.to_le_bytes()); // time
        buf.extend_from_slice(&0u16.to_le_bytes()); // date
        buf.extend_from_slice(&crc32.to_le_bytes());
        buf.extend_from_slice(&data_len.to_le_bytes());
        buf.extend_from_slice(&data_len.to_le_bytes());
        buf.extend_from_slice(&name_len.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes()); // extra len
        buf.extend_from_slice(&0u16.to_le_bytes()); // comment len
        buf.extend_from_slice(&0u16.to_le_bytes()); // disk number
        buf.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
        buf.extend_from_slice(&0u32.to_le_bytes()); // external attrs
        buf.extend_from_slice(&0u32.to_le_bytes()); // local header offset
        buf.extend_from_slice(name_bytes);

        let cd_size = (buf.len() as u32) - cd_offset;
        // End of central directory
        buf.extend_from_slice(b"PK\x05\x06");
        buf.extend_from_slice(&0u16.to_le_bytes()); // disk num
        buf.extend_from_slice(&0u16.to_le_bytes()); // start disk
        buf.extend_from_slice(&1u16.to_le_bytes()); // entries on disk
        buf.extend_from_slice(&1u16.to_le_bytes()); // total entries
        buf.extend_from_slice(&cd_size.to_le_bytes());
        buf.extend_from_slice(&cd_offset.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes()); // comment len
        buf
    }

    #[test]
    fn test_instance_prepare_runtime() {
        use acl_minecraft::{AssetObject, Library, LibraryArtifact, LibraryDownloads};
        use std::collections::HashMap;

        let temp = TempDirGuard::new("instance_prepare_runtime");
        let dirs = LauncherDirs::with_paths(temp.path().join("config"), temp.path().join("data"));

        let instance = Instance::create("runtime_inst", "Runtime", "1.21.1", &dirs.versions_dir())
            .expect("should create instance");

        // Prepare a mock asset object for legacy resource mapping
        let object_hash = "bdf48ef6b5d0d23ebb4f8a6179d4a1bfab1a9598";
        let object_path = dirs
            .assets_dir()
            .join("objects")
            .join(&object_hash[..2])
            .join(object_hash);
        fs::create_dir_all(object_path.parent().expect("parent should exist"))
            .expect("failed to create asset object dir");
        fs::write(&object_path, b"legacy-asset-bytes").expect("failed to write asset object");

        let mut objects = HashMap::new();
        objects.insert(
            "sounds/click.ogg".to_string(),
            AssetObject {
                hash: object_hash.to_string(),
                size: 18,
            },
        );
        let asset_index = AssetIndex {
            objects,
            map_to_resources: true,
            is_virtual: false,
        };

        // Prepare a mock native library jar containing an empty .so file (CRC32 of b"" is 0)
        let rel_native_jar = "org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3-natives-linux.jar";
        let native_jar_path = dirs.libraries_dir().join(rel_native_jar);
        fs::create_dir_all(native_jar_path.parent().expect("parent should exist"))
            .expect("failed to create native jar parent dir");
        let zip_bytes = build_stored_zip("liblwjgl.so", b"", 0);
        fs::write(&native_jar_path, zip_bytes).expect("failed to write native jar");

        let mut metadata = sample_version_metadata();
        metadata.libraries = vec![Library {
            name: "org.lwjgl:lwjgl:3.3.3:natives-linux".to_string(),
            downloads: Some(LibraryDownloads {
                artifact: Some(LibraryArtifact {
                    path: Some(rel_native_jar.to_string()),
                    sha1: "0000000000000000000000000000000000000000".to_string(),
                    size: 100,
                    url: "https://example.com/native.jar".to_string(),
                }),
                classifiers: None,
            }),
            natives: None,
            rules: None,
            extract: None,
        }];

        instance
            .prepare_runtime(&metadata, &asset_index, &dirs)
            .expect("prepare_runtime should succeed");

        let mapped_resource = instance.game_dir().join("resources/sounds/click.ogg");
        assert!(mapped_resource.exists());
        assert_eq!(
            fs::read(&mapped_resource).expect("should read mapped resource"),
            b"legacy-asset-bytes"
        );

        let extracted_so = instance.natives_dir().join("liblwjgl.so");
        assert!(extracted_so.exists());
    }
}
