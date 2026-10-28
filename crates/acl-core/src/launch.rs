use std::{
    fs, io,
    path::PathBuf,
    process::{self, Stdio},
};

use acl_auth::Account;
use acl_minecraft::{VersionMetadata, VersionType};
use tracing::{debug, info, warn};

use crate::{CoreError, Instance, LauncherConfig, LauncherDirs, Result, java::JavaInstallation};

const LAUNCHER_NAME: &str = "ACL";
const LAUNCHER_VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct GameProcess {
    pub id: Option<u32>,
    pub child: tokio::process::Child,
}

impl GameProcess {
    /// Asynchronously wait for the game to exit, returning the exit status (`status.success()` being true indicates a normal exit; false indicates a crash).
    pub async fn wait(mut self) -> io::Result<process::ExitStatus> {
        let status = self.child.wait().await?;
        if status.success() {
            info!("Game process exited successfully");
        } else {
            warn!(
                "Game process exited with non-zero status: {:?}",
                status.code()
            );
        }
        Ok(status)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchCommand {
    pub java_path: PathBuf,
    /// Working dir
    pub game_dir: PathBuf,
    pub jvm_args: Vec<String>,
    pub main_class: String,
    pub game_args: Vec<String>,
}

impl LaunchCommand {
    fn build_param_vars(
        instance: &Instance,
        dirs: &LauncherDirs,
        metadata: &VersionMetadata,
        account: &Account,
        classpath: String,
    ) -> Vec<(&'static str, String)> {
        let natives_dir = instance.natives_dir().to_string_lossy().into_owned();
        let game_dir = instance.game_dir().to_string_lossy().into_owned();
        let assets_dir = dirs.assets_dir().to_string_lossy().into_owned();

        let version_type = match metadata.version_type {
            VersionType::Release => "release",
            VersionType::Snapshot => "snapshot",
            VersionType::OldBeta => "old_beta",
            VersionType::OldAlpha => "old_alpha",
        };

        vec![
            // JVM argument placeholder
            ("${natives_directory}", natives_dir),
            ("${launcher_name}", LAUNCHER_NAME.to_owned()),
            ("${launcher_version}", LAUNCHER_VERSION.to_owned()),
            ("${classpath}", classpath),
            // Game argument placeholder
            ("${auth_player_name}", account.username.clone()),
            ("${version_name}", instance.config.id.clone()),
            ("${game_directory}", game_dir),
            ("${assets_root}", assets_dir.clone()),
            ("${game_assets}", assets_dir),
            ("${assets_index_name}", metadata.asset_index.id.clone()),
            ("${auth_uuid}", account.uuid.simple().to_string()),
            ("${auth_access_token}", "0".to_string()),
            ("${user_type}", "legacy".to_string()),
            ("${version_type}", version_type.to_string()),
            ("${user_properties}", "{}".to_string()),
        ]
    }

    fn apply_placeholders(raw_args: Vec<String>, vars: &[(&str, String)]) -> Vec<String> {
        raw_args
            .into_iter()
            .map(|mut arg| {
                for (placeholder, value) in vars {
                    arg = arg.replace(placeholder, value);
                }
                arg
            })
            .collect()
    }

    pub fn build(
        instance: &Instance,
        global_config: &LauncherConfig,
        dirs: &LauncherDirs,
        metadata: &VersionMetadata,
        account: &Account,
        java_path: PathBuf,
    ) -> Result<Self> {
        let mut jvm_args = Vec::new();

        let max_mem = instance
            .config
            .max_memory_mb
            .unwrap_or(global_config.max_memory_mb);

        let min_mem = instance
            .config
            .min_memory_mb
            .unwrap_or(global_config.min_memory_mb);

        if min_mem > max_mem {
            return Err(CoreError::InvalidMemoryConfig {
                min_mb: min_mem,
                max_mb: max_mem,
            });
        }

        // Memory arguments.
        jvm_args.push(format!("-Xms{min_mem}M"));
        jvm_args.push(format!("-Xmx{max_mem}M"));

        let entries =
            metadata.classpath_entries(&dirs.libraries_dir(), instance.id(), &dirs.versions_dir());

        let classpath = std::env::join_paths(entries)?
            .to_string_lossy()
            .into_owned();

        let vars = Self::build_param_vars(instance, dirs, metadata, account, classpath);

        // JVM arguments in the metadata.
        let raw_jvm = metadata.resolve_jvm_arguments();
        jvm_args.extend(Self::apply_placeholders(raw_jvm, &vars));

        // User-defined JVM arguments.
        jvm_args.extend(instance.config.extra_jvm_args.iter().cloned());

        // Command-line arguments for the game.
        let game_args = Self::apply_placeholders(metadata.resolve_game_arguments(), &vars);

        Ok(Self {
            java_path,
            game_dir: instance.game_dir().to_path_buf(),
            jvm_args,
            main_class: metadata.main_class.clone(),
            game_args,
        })
    }

    pub async fn spawn(&self) -> Result<GameProcess> {
        fs::create_dir_all(&self.game_dir).map_err(|source| CoreError::Io {
            path: self.game_dir.clone(),
            source,
        })?;

        info!(
            "Spawning Minecraft process using Java: {}",
            self.java_path.display()
        );
        debug!("Working directory: {}", self.game_dir.display());
        debug!("JVM arguments: {:?}", self.jvm_args);
        debug!("Main class: {}", self.main_class);
        // don't print sensitive game arguments
        let child = tokio::process::Command::new(&self.java_path)
            .current_dir(&self.game_dir)
            .args(&self.jvm_args)
            .arg(&self.main_class)
            .args(&self.game_args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|source| CoreError::ProcessSpawn {
                java_path: self.java_path.clone(),
                source,
            })?;

        let pid = child.id();
        info!("Minecraft process spawned successfully with PID: {:?}", pid);

        Ok(GameProcess {
            id: child.id(),
            child,
        })
    }
}

pub fn select_java(
    instance: &Instance,
    global_config: &LauncherConfig,
    metadata: &VersionMetadata,
) -> Result<JavaInstallation> {
    let required_major = metadata
        .java_version
        .as_ref()
        .map_or(8, |j| j.major_version);

    debug!(
        "Resolving Java runtime for required major version: {}",
        required_major
    );

    // Try to obtain it from the game-configured Java or the global Java.
    if let Some(java) = instance
        .config
        .java
        .as_ref()
        .or(global_config.default_java.as_ref())
        && java.info.major_version >= required_major
    {
        info!(
            "Selected preferred Java runtime {} at {}",
            java.info.major_version,
            java.path.display()
        );
        return Ok(java.clone());
    }

    debug!(
        "Preferred Java runtime is unavailable or incompatible, searching discovered installations"
    );

    if global_config.java_list.is_empty() {
        return Err(CoreError::JavaNotFound);
    }

    // Try to obtain the exact version from the global Java list.
    if let Some(exact) = global_config
        .java_list
        .iter()
        .find(|j| j.info.major_version == required_major)
    {
        info!(
            "Selected exact matching Java runtime {} at {}",
            exact.info.major_version,
            exact.path.display()
        );

        return Ok(exact.clone());
    }

    // Try to find a Java version greater than the target version from the global Java list.
    if let Some(compatible) = global_config
        .java_list
        .iter()
        .filter(|j| j.info.major_version > required_major)
        .min_by_key(|j| j.info.major_version)
    {
        info!(
            "Selected compatible higher Java runtime {} at {}",
            compatible.info.major_version,
            compatible.path.display()
        );

        return Ok(compatible.clone());
    }

    warn!(
        "No compatible Java installation found for major version {}",
        required_major
    );

    // No suitable Java found; find the highest version to return an error.
    let best_incompatible = global_config
        .java_list
        .iter()
        .max_by_key(|j| j.info.major_version)
        .ok_or(CoreError::JavaNotFound)?;

    Err(CoreError::JavaVersionMismatch {
        required_major,
        found_major: best_incompatible.info.major_version,
        path: best_incompatible.path.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::java::JavaInfo;
    use acl_minecraft::{
        ArgumentEntry, Arguments, AssetIndexInfo, DownloadFile, JavaVersionInfo, VersionDownloads,
        VersionMetadata, VersionType,
    };
    use std::path::{Path, PathBuf};

    fn mock_java(path: &str, major: u32) -> JavaInstallation {
        JavaInstallation {
            path: PathBuf::from(path),
            info: JavaInfo {
                version: format!("{major}.0.1"),
                major_version: major,
                vendor: Some("Adoptium".to_string()),
                is_64bit: true,
            },
        }
    }

    fn mock_version_metadata(id: &str, legacy: bool, java_major: Option<u32>) -> VersionMetadata {
        let arguments = if legacy {
            None
        } else {
            Some(Arguments {
                game: vec![
                    ArgumentEntry::Simple("--username".to_string()),
                    ArgumentEntry::Simple("${auth_player_name}".to_string()),
                    ArgumentEntry::Simple("--version".to_string()),
                    ArgumentEntry::Simple("${version_name}".to_string()),
                    ArgumentEntry::Simple("--gameDir".to_string()),
                    ArgumentEntry::Simple("${game_directory}".to_string()),
                    ArgumentEntry::Simple("--assetsDir".to_string()),
                    ArgumentEntry::Simple("${assets_root}".to_string()),
                    ArgumentEntry::Simple("--uuid".to_string()),
                    ArgumentEntry::Simple("${auth_uuid}".to_string()),
                ],
                jvm: vec![
                    ArgumentEntry::Simple("-Djava.library.path=${natives_directory}".to_string()),
                    ArgumentEntry::Simple(
                        "-Dminecraft.launcher.brand=${launcher_name}".to_string(),
                    ),
                    ArgumentEntry::Simple(
                        "-Dminecraft.launcher.version=${launcher_version}".to_string(),
                    ),
                    ArgumentEntry::Simple("-cp".to_string()),
                    ArgumentEntry::Simple("${classpath}".to_string()),
                ],
            })
        };

        let minecraft_arguments = if legacy {
            Some(
                "--username ${auth_player_name} --version ${version_name} --gameDir ${game_directory}"
                    .to_string(),
            )
        } else {
            None
        };

        VersionMetadata {
            id: id.to_string(),
            version_type: VersionType::Release,
            main_class: "net.minecraft.client.main.Main".to_string(),
            minecraft_arguments,
            arguments,
            assets: Some("17".to_string()),
            asset_index: AssetIndexInfo {
                id: "17".to_string(),
                sha1: "abc123sha1".to_string(),
                size: 100,
                total_size: 200,
                url: "https://example.com/asset.json".to_string(),
            },
            downloads: VersionDownloads {
                client: DownloadFile {
                    sha1: "client_sha1".to_string(),
                    size: 1000,
                    url: "https://example.com/client.jar".to_string(),
                },
                server: None,
            },
            java_version: java_major.map(|major_version| JavaVersionInfo {
                component: "java-runtime".to_string(),
                major_version,
            }),
            libraries: Vec::new(),
        }
    }

    fn mock_instance(id: &str) -> Instance {
        let base_dir = Path::new("/mock/data/instances");
        Instance::new(id, id, "1.21", base_dir).expect("failed to create mock instance")
    }

    #[test]
    fn test_launch_command_build_modern() {
        let instance = mock_instance("modern_inst");
        let global_config = LauncherConfig::default();
        let dirs =
            LauncherDirs::with_paths(PathBuf::from("/mock/config"), PathBuf::from("/mock/data"));
        let metadata = mock_version_metadata("1.21.1", false, Some(21));
        let account = Account::new_offline("Steve").expect("failed to create account");
        let java_path = PathBuf::from("/mock/java/bin/java");

        let cmd = LaunchCommand::build(
            &instance,
            &global_config,
            &dirs,
            &metadata,
            &account,
            java_path.clone(),
        )
        .expect("build launch command should succeed");

        assert_eq!(cmd.java_path, java_path);
        assert_eq!(cmd.game_dir, instance.game_dir());
        assert_eq!(cmd.main_class, "net.minecraft.client.main.Main");

        // Memory arguments
        assert_eq!(cmd.jvm_args[0], "-Xms1024M");
        assert_eq!(cmd.jvm_args[1], "-Xmx4096M");

        // JVM arguments placeholder replacement
        assert!(
            cmd.jvm_args
                .iter()
                .any(|arg| arg.starts_with("-Djava.library.path="))
        );
        assert!(
            cmd.jvm_args
                .iter()
                .any(|arg| arg == "-Dminecraft.launcher.brand=ACL")
        );
        assert!(cmd.jvm_args.iter().any(
            |arg| arg == &format!("-Dminecraft.launcher.version={}", env!("CARGO_PKG_VERSION"))
        ));
        assert!(cmd.jvm_args.iter().any(|arg| arg == "-cp"));
        let cp_arg = cmd.jvm_args.iter().position(|arg| arg == "-cp").unwrap() + 1;
        assert!(cmd.jvm_args[cp_arg].contains("modern_inst.jar"));

        // Game arguments placeholder replacement
        assert_eq!(
            cmd.game_args,
            vec![
                "--username",
                "Steve",
                "--version",
                "modern_inst",
                "--gameDir",
                instance.game_dir().to_str().unwrap(),
                "--assetsDir",
                "/mock/data/assets",
                "--uuid",
                &account.uuid.simple().to_string(),
            ]
        );
    }

    #[test]
    fn test_launch_command_build_legacy() {
        let instance = mock_instance("legacy_inst");
        let global_config = LauncherConfig::default();
        let dirs =
            LauncherDirs::with_paths(PathBuf::from("/mock/config"), PathBuf::from("/mock/data"));
        let metadata = mock_version_metadata("1.12.2", true, Some(8));
        let account = Account::new_offline("Alex").expect("failed to create account");
        let java_path = PathBuf::from("/mock/java8/bin/java");

        let cmd = LaunchCommand::build(
            &instance,
            &global_config,
            &dirs,
            &metadata,
            &account,
            java_path.clone(),
        )
        .expect("build legacy launch command should succeed");

        assert_eq!(cmd.java_path, java_path);
        assert_eq!(cmd.main_class, "net.minecraft.client.main.Main");

        // Legacy JVM arguments automatically provided
        assert_eq!(cmd.jvm_args[0], "-Xms1024M");
        assert_eq!(cmd.jvm_args[1], "-Xmx4096M");
        assert!(
            cmd.jvm_args
                .iter()
                .any(|arg| arg.starts_with("-Djava.library.path="))
        );
        assert!(cmd.jvm_args.iter().any(|arg| arg == "-cp"));

        // Game arguments from split minecraft_arguments
        assert_eq!(
            cmd.game_args,
            vec![
                "--username",
                "Alex",
                "--version",
                "legacy_inst",
                "--gameDir",
                instance.game_dir().to_str().unwrap(),
            ]
        );
    }

    #[test]
    fn test_launch_command_memory_limits_and_validation() {
        let mut instance = mock_instance("mem_inst");
        instance.config.min_memory_mb = Some(2048);
        instance.config.max_memory_mb = Some(8192);

        let global_config = LauncherConfig::default();
        let dirs =
            LauncherDirs::with_paths(PathBuf::from("/mock/config"), PathBuf::from("/mock/data"));
        let metadata = mock_version_metadata("1.21.1", false, Some(21));
        let account = Account::new_offline("Steve").unwrap();
        let java_path = PathBuf::from("/mock/bin/java");

        let cmd = LaunchCommand::build(
            &instance,
            &global_config,
            &dirs,
            &metadata,
            &account,
            java_path.clone(),
        )
        .expect("memory config should be valid");

        assert_eq!(cmd.jvm_args[0], "-Xms2048M");
        assert_eq!(cmd.jvm_args[1], "-Xmx8192M");

        // Invalid memory configuration (min > max)
        instance.config.min_memory_mb = Some(8192);
        instance.config.max_memory_mb = Some(4096);

        let err = LaunchCommand::build(
            &instance,
            &global_config,
            &dirs,
            &metadata,
            &account,
            java_path,
        )
        .expect_err("min_mem > max_mem should fail");

        match err {
            CoreError::InvalidMemoryConfig { min_mb, max_mb } => {
                assert_eq!(min_mb, 8192);
                assert_eq!(max_mb, 4096);
            }
            other => panic!("expected InvalidMemoryConfig, got: {other:?}"),
        }
    }

    #[test]
    fn test_launch_command_extra_jvm_args() {
        let mut instance = mock_instance("extra_jvm_inst");
        instance.config.extra_jvm_args =
            vec!["-XX:+UseG1GC".to_string(), "-Dcustom.prop=true".to_string()];

        let global_config = LauncherConfig::default();
        let dirs =
            LauncherDirs::with_paths(PathBuf::from("/mock/config"), PathBuf::from("/mock/data"));
        let metadata = mock_version_metadata("1.21.1", false, Some(21));
        let account = Account::new_offline("Steve").unwrap();
        let java_path = PathBuf::from("/mock/bin/java");

        let cmd = LaunchCommand::build(
            &instance,
            &global_config,
            &dirs,
            &metadata,
            &account,
            java_path,
        )
        .expect("build should succeed");

        let len = cmd.jvm_args.len();
        assert_eq!(cmd.jvm_args[len - 2], "-XX:+UseG1GC");
        assert_eq!(cmd.jvm_args[len - 1], "-Dcustom.prop=true");
    }

    #[test]
    fn test_select_java_instance_override() {
        let mut instance = mock_instance("inst");
        let inst_java = mock_java("/mock/instance/java", 17);
        instance.config.java = Some(inst_java.clone());

        let global_config = LauncherConfig {
            default_java: Some(mock_java("/mock/default/java", 21)),
            ..Default::default()
        };

        let metadata = mock_version_metadata("1.20", false, Some(17));

        let selected =
            select_java(&instance, &global_config, &metadata).expect("should select instance Java");
        assert_eq!(selected, inst_java);
    }

    #[test]
    fn test_select_java_default_match() {
        let instance = mock_instance("inst");
        let default_java = mock_java("/mock/default/java", 21);
        let global_config = LauncherConfig {
            default_java: Some(default_java.clone()),
            ..Default::default()
        };

        let metadata = mock_version_metadata("1.21", false, Some(21));

        let selected =
            select_java(&instance, &global_config, &metadata).expect("should select default Java");
        assert_eq!(selected, default_java);
    }

    #[test]
    fn test_select_java_exact_match_fallback() {
        let instance = mock_instance("inst");
        let java17 = mock_java("/mock/discovered/java17", 17);
        let java21 = mock_java("/mock/discovered/java21", 21);
        let global_config = LauncherConfig {
            // default_java is incompatible (Java 8 < 17)
            default_java: Some(mock_java("/mock/default/java8", 8)),
            java_list: vec![
                mock_java("/mock/discovered/java8", 8),
                java17.clone(),
                java21,
            ],
            ..Default::default()
        };

        let metadata = mock_version_metadata("1.20", false, Some(17));

        let selected = select_java(&instance, &global_config, &metadata)
            .expect("should find exact matching Java");
        assert_eq!(selected, java17);
    }

    #[test]
    fn test_select_java_higher_compatible_fallback() {
        let instance = mock_instance("inst");
        let java21 = mock_java("/mock/discovered/java21", 21);
        let java22 = mock_java("/mock/discovered/java22", 22);
        let global_config = LauncherConfig {
            java_list: vec![
                mock_java("/mock/discovered/java8", 8),
                java22,
                java21.clone(),
            ],
            ..Default::default()
        };

        // Requires 17, but only 21 and 22 are available -> picks 21
        let metadata = mock_version_metadata("1.20", false, Some(17));

        let selected = select_java(&instance, &global_config, &metadata)
            .expect("should find lowest compatible higher Java");
        assert_eq!(selected, java21);
    }

    #[test]
    fn test_select_java_mismatch_error() {
        let instance = mock_instance("inst");
        let global_config = LauncherConfig {
            java_list: vec![
                mock_java("/mock/discovered/java8", 8),
                mock_java("/mock/discovered/java11", 11),
            ],
            ..Default::default()
        };

        let metadata = mock_version_metadata("1.21", false, Some(21));

        let err = select_java(&instance, &global_config, &metadata)
            .expect_err("should fail when all versions are incompatible");

        match err {
            CoreError::JavaVersionMismatch {
                required_major,
                found_major,
                path,
            } => {
                assert_eq!(required_major, 21);
                assert_eq!(found_major, 11);
                assert_eq!(path, PathBuf::from("/mock/discovered/java11"));
            }
            other => panic!("expected JavaVersionMismatch, got: {other:?}"),
        }
    }

    #[test]
    fn test_select_java_empty_list_error() {
        let instance = mock_instance("inst");
        let global_config = LauncherConfig::default();
        let metadata = mock_version_metadata("1.21", false, Some(21));

        let err = select_java(&instance, &global_config, &metadata)
            .expect_err("should fail when java_list is empty");

        match err {
            CoreError::JavaNotFound => {}
            other => panic!("expected JavaNotFound, got: {other:?}"),
        }
    }

    #[test]
    fn test_select_java_default_legacy_version() {
        let instance = mock_instance("inst");
        let java8 = mock_java("/mock/discovered/java8", 8);
        let global_config = LauncherConfig {
            java_list: vec![java8.clone()],
            ..Default::default()
        };

        // java_version is None -> defaults to 8
        let metadata = mock_version_metadata("1.12.2", true, None);

        let selected = select_java(&instance, &global_config, &metadata)
            .expect("should default to Java 8 requirement");
        assert_eq!(selected, java8);
    }
}
