// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    collections::BTreeMap,
    fs, io,
    path::PathBuf,
    process::{self, Stdio},
};

use acl_auth::{Account, AccountType};
use acl_minecraft::{VersionMetadata, VersionType};
use tracing::{debug, info, warn};

use crate::{
    CoreError, Instance, LauncherConfig, LauncherDirs, Result, gpu::DiscreteGpuEnv,
    java::JavaInstallation, system::memory_info,
};

const LAUNCHER_NAME: &str = "ACL";
const LAUNCHER_VERSION: &str = env!("CARGO_PKG_VERSION");

fn is_wayland_session() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var("XDG_SESSION_TYPE").is_ok_and(|s| s.eq_ignore_ascii_case("wayland"))
}

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

#[derive(Clone, PartialEq, Eq)]
pub struct LaunchCommand {
    pub java_path: PathBuf,
    /// Working dir
    pub game_dir: PathBuf,
    pub jvm_args: Vec<String>,
    pub main_class: String,
    pub game_args: Vec<String>,
    pub env_vars: BTreeMap<String, String>,
}

impl std::fmt::Debug for LaunchCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let sanitized_game_args: Vec<String> = self
            .game_args
            .iter()
            .enumerate()
            .map(|(i, arg)| {
                if i > 0 && self.game_args.get(i - 1).map(|s| s.as_str()) == Some("--accessToken") {
                    "[REDACTED]".to_string()
                } else if arg.starts_with("--accessToken=") {
                    "--accessToken=[REDACTED]".to_string()
                } else {
                    arg.clone()
                }
            })
            .collect();

        f.debug_struct("LaunchCommand")
            .field("java_path", &self.java_path)
            .field("game_dir", &self.game_dir)
            .field("jvm_args", &self.jvm_args)
            .field("main_class", &self.main_class)
            .field("game_args", &sanitized_game_args)
            .field("env_vars", &self.env_vars.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl LaunchCommand {
    fn resolve_env_vars(
        instance: &Instance,
        global_config: &LauncherConfig,
        is_wayland_session: bool,
        gpu_env: Option<&DiscreteGpuEnv>,
    ) -> BTreeMap<String, String> {
        let mut env = BTreeMap::new();
        let custom = instance.config.custom_settings;

        let force_wayland = if custom {
            instance
                .config
                .force_wayland
                .unwrap_or(global_config.force_wayland)
        } else {
            global_config.force_wayland
        };

        let use_discrete_gpu = if custom {
            instance
                .config
                .use_discrete_gpu
                .unwrap_or(global_config.use_discrete_gpu)
        } else {
            global_config.use_discrete_gpu
        };

        // Preset switches (lowest priority, can be overridden by user env vars)

        // Only inject environment variables when "force_wayland" is enabled and running
        // under an actual Wayland session, preventing crashes on X11.
        if force_wayland && is_wayland_session {
            env.insert("SDL_VIDEO_DRIVER".to_string(), "wayland".to_string());
        }
        if use_discrete_gpu && let Some(gpu) = gpu_env {
            gpu.apply_vars(&mut env);
        }

        // Global custom env vars
        env.extend(
            global_config
                .env_vars
                .iter()
                .map(|(k, v)| (k.clone(), v.clone())),
        );

        // Instance custom env vars (highest priority)
        if custom {
            env.extend(
                instance
                    .config
                    .env_vars
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone())),
            );
        }

        env
    }

    fn build_param_vars(
        instance: &Instance,
        dirs: &LauncherDirs,
        metadata: &VersionMetadata,
        account: &Account,
        access_token: &str,
        classpath: String,
    ) -> Vec<(&'static str, String)> {
        let natives_dir = instance.natives_dir().to_string_lossy().into_owned();
        let game_dir = instance.game_dir().to_string_lossy().into_owned();
        let assets_dir = dirs.assets_dir().to_string_lossy().into_owned();

        let virtual_dir = dirs
            .assets_dir()
            .join("virtual")
            .join(&metadata.asset_index.id);

        // For legacy asset versions (e.g. 1.6.4), reconstructed assets reside under "assets/virtual/<index_id>"
        // (such as "assets/virtual/legacy"). Point "${game_assets}" there so sounds and music load properly.
        let game_assets = if virtual_dir.exists() || metadata.assets.as_deref() == Some("legacy") {
            virtual_dir.to_string_lossy().into_owned()
        } else {
            assets_dir.clone()
        };

        let version_type = match metadata.version_type {
            VersionType::Release => "release",
            VersionType::Snapshot => "snapshot",
            VersionType::OldBeta => "old_beta",
            VersionType::OldAlpha => "old_alpha",
        };

        let user_type = match account.account_type {
            AccountType::Offline => "legacy".to_string(),
            AccountType::Microsoft => "msa".to_string(),
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
            ("${assets_root}", assets_dir),
            ("${game_assets}", game_assets),
            ("${assets_index_name}", metadata.asset_index.id.clone()),
            ("${auth_uuid}", account.uuid.simple().to_string()),
            ("${auth_access_token}", access_token.to_string()),
            ("${user_type}", user_type),
            ("${version_type}", version_type.to_string()),
            ("${user_properties}", "{}".to_string()),
            (
                "${library_directory}",
                dirs.libraries_dir().to_string_lossy().into_owned(),
            ),
            ("${classpath_separator}", ":".to_owned()),
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

    fn resolve_memory_limits(
        instance: &Instance,
        global_config: &LauncherConfig,
        available_bytes: u64,
        is_64bit: bool,
    ) -> (u32, u32) {
        let custom = instance.config.custom_settings;

        let auto_memory = if custom {
            instance
                .config
                .auto_memory
                .unwrap_or(global_config.auto_memory)
        } else {
            global_config.auto_memory
        };

        let mut max_mem = if custom {
            instance
                .config
                .max_memory_mb
                .unwrap_or(global_config.max_memory_mb)
        } else {
            global_config.max_memory_mb
        };

        if auto_memory && let Some(suggested) = suggested_max_memory_mb(available_bytes, is_64bit) {
            max_mem = suggested;
        }

        let min_mem = if auto_memory {
            global_config.min_memory_mb.min(max_mem)
        } else {
            global_config.min_memory_mb
        };
        (min_mem, max_mem)
    }

    pub fn build(
        instance: &Instance,
        global_config: &LauncherConfig,
        dirs: &LauncherDirs,
        metadata: &VersionMetadata,
        account: &Account,
        access_token: &str,
        java: &JavaInstallation,
    ) -> Result<Self> {
        let mut jvm_args = Vec::new();

        let custom = instance.config.custom_settings;

        let (min_mem, max_mem) = Self::resolve_memory_limits(
            instance,
            global_config,
            memory_info().available,
            java.info.is_64bit,
        );

        if min_mem > max_mem {
            return Err(CoreError::InvalidMemoryConfig {
                min_mb: min_mem,
                max_mb: max_mem,
            });
        }

        info!("Launching game with max memory: {max_mem} MiB");

        // Memory arguments.
        jvm_args.push(format!("-Xms{min_mem}M"));
        jvm_args.push(format!("-Xmx{max_mem}M"));

        let entries =
            metadata.classpath_entries(&dirs.libraries_dir(), instance.id(), &dirs.versions_dir());

        let classpath = std::env::join_paths(entries)?
            .to_string_lossy()
            .into_owned();

        let vars =
            Self::build_param_vars(instance, dirs, metadata, account, access_token, classpath);

        // JVM arguments in the metadata.
        let raw_jvm = metadata.resolve_jvm_arguments();
        jvm_args.extend(Self::apply_placeholders(raw_jvm, &vars));

        // User-defined JVM arguments.
        jvm_args.extend(instance.config.extra_jvm_args.iter().cloned());

        // Command-line arguments for the game.
        let mut game_args = Self::apply_placeholders(metadata.resolve_game_arguments(), &vars);

        let fullscreen = if custom {
            instance
                .config
                .fullscreen
                .unwrap_or(global_config.fullscreen)
        } else {
            global_config.fullscreen
        };

        let fullscreen_param = "--fullscreen".to_string();
        if fullscreen && !game_args.contains(&fullscreen_param) {
            game_args.push(fullscreen_param);
        }

        let use_discrete_gpu = if custom {
            instance
                .config
                .use_discrete_gpu
                .unwrap_or(global_config.use_discrete_gpu)
        } else {
            global_config.use_discrete_gpu
        };

        let env_vars = if use_discrete_gpu {
            Self::resolve_env_vars(
                instance,
                global_config,
                is_wayland_session(),
                Some(&DiscreteGpuEnv::detect()),
            )
        } else {
            Self::resolve_env_vars(instance, global_config, is_wayland_session(), None)
        };

        Ok(Self {
            java_path: java.path.to_path_buf(),
            game_dir: instance.game_dir().to_path_buf(),
            jvm_args,
            main_class: metadata.main_class.clone(),
            game_args,
            env_vars,
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
            .envs(&self.env_vars)
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
    let preferred_java = if instance.config.custom_settings {
        instance.config.java.as_ref()
    } else {
        global_config.default_java.as_ref()
    };

    if let Some(java) = preferred_java
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

const MIB: u64 = 1024 * 1024;
const RESERVE_MB: u64 = 512;
const EIGHT_GIB_MB: u64 = 8 * 1024;
const BASE_TIER_MB: u64 = 6553; // EIGHT_GIB_MB * 4 / 5
const SIXTEEN_GIB_MB: u64 = 16 * 1024;
const BIT32_CAP_MB: u32 = 768;

pub fn suggested_max_memory_mb(available_bytes: u64, is_64bit: bool) -> Option<u32> {
    let available_mb = available_bytes.checked_div(MIB)?;
    if available_mb == 0 {
        return None;
    }

    let usable_mb = available_mb.saturating_sub(RESERVE_MB);

    let tiered_mb = if usable_mb <= EIGHT_GIB_MB {
        usable_mb.saturating_mul(4).checked_div(5)?
    } else {
        let extra = usable_mb.saturating_sub(EIGHT_GIB_MB).checked_div(5)?;
        BASE_TIER_MB.saturating_add(extra).min(SIXTEEN_GIB_MB)
    };

    // Fallback memory
    let floor_mb = available_mb.checked_div(2)?.clamp(1, RESERVE_MB);

    let suggested_mb = tiered_mb.max(floor_mb).min(SIXTEEN_GIB_MB);

    let mut mb = u32::try_from(suggested_mb).unwrap_or(u32::MAX);
    if !is_64bit {
        mb = mb.min(BIT32_CAP_MB);
    }
    Some(mb)
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
        let global_config = LauncherConfig {
            auto_memory: false,
            ..Default::default()
        };
        let dirs = LauncherDirs::with_paths(
            PathBuf::from("/mock/config"),
            PathBuf::from("/mock/data"),
            PathBuf::from("/mock/cache"),
        );
        let metadata = mock_version_metadata("1.21.1", false, Some(21));
        let account = Account::new_offline("Steve").expect("failed to create account");

        let java_path = PathBuf::from("/mock/java/bin/java");
        let java = JavaInstallation {
            path: java_path.clone(),
            info: JavaInfo {
                version: "17".to_string(),
                major_version: 17,
                vendor: None,
                is_64bit: true,
            },
        };
        let access_token = "0".to_string();

        let cmd = LaunchCommand::build(
            &instance,
            &global_config,
            &dirs,
            &metadata,
            &account,
            &access_token,
            &java,
        )
        .expect("build launch command should succeed");

        assert_eq!(cmd.java_path, java_path);
        assert_eq!(cmd.game_dir, instance.game_dir());
        assert_eq!(cmd.main_class, "net.minecraft.client.main.Main");

        // Memory arguments
        assert_eq!(cmd.jvm_args[0], "-Xms512M");
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
    fn test_launch_command_build_microsoft_account() {
        let instance = mock_instance("ms_inst");
        let global_config = LauncherConfig::default();
        let dirs = LauncherDirs::with_paths(
            PathBuf::from("/mock/config"),
            PathBuf::from("/mock/data"),
            PathBuf::from("/mock/cache"),
        );
        let mut metadata = mock_version_metadata("1.21.1", false, Some(21));
        if let Some(args) = &mut metadata.arguments {
            args.game
                .push(ArgumentEntry::Simple("--accessToken".to_string()));
            args.game
                .push(ArgumentEntry::Simple("${auth_access_token}".to_string()));
            args.game
                .push(ArgumentEntry::Simple("--userType".to_string()));
            args.game
                .push(ArgumentEntry::Simple("${user_type}".to_string()));
        }

        let hex_uuid = "4566e69fc90748ee8d71d7ba5aaafced";
        let account = Account::new_microsoft("Steve", hex_uuid).expect("failed to create account");
        let java_path = PathBuf::from("/mock/java/bin/java");
        let java = JavaInstallation {
            path: java_path.clone(),
            info: JavaInfo {
                version: "17".to_string(),
                major_version: 17,
                vendor: None,
                is_64bit: true,
            },
        };
        let access_token = "mock_msa_access_token_xyz".to_string();

        let cmd = LaunchCommand::build(
            &instance,
            &global_config,
            &dirs,
            &metadata,
            &account,
            &access_token,
            &java,
        )
        .expect("build launch command should succeed");

        assert_eq!(account.account_type, AccountType::Microsoft);

        let token_pos = cmd
            .game_args
            .iter()
            .position(|arg| arg == "--accessToken")
            .expect("--accessToken should exist");
        assert_eq!(cmd.game_args[token_pos + 1], "mock_msa_access_token_xyz");

        let user_type_pos = cmd
            .game_args
            .iter()
            .position(|arg| arg == "--userType")
            .expect("--userType should exist");
        assert_eq!(cmd.game_args[user_type_pos + 1], "msa");

        let uuid_pos = cmd
            .game_args
            .iter()
            .position(|arg| arg == "--uuid")
            .expect("--uuid should exist");
        assert_eq!(cmd.game_args[uuid_pos + 1], hex_uuid);
    }

    #[test]
    fn test_launch_command_build_legacy() {
        let instance = mock_instance("legacy_inst");
        let global_config = LauncherConfig {
            auto_memory: false,
            ..Default::default()
        };
        let dirs = LauncherDirs::with_paths(
            PathBuf::from("/mock/config"),
            PathBuf::from("/mock/data"),
            PathBuf::from("/mock/cache"),
        );
        let metadata = mock_version_metadata("1.12.2", true, Some(8));
        let account = Account::new_offline("Alex").expect("failed to create account");
        let java_path = PathBuf::from("/mock/java8/bin/java");
        let java = JavaInstallation {
            path: java_path.clone(),
            info: JavaInfo {
                version: "1.8".to_string(),
                major_version: 8,
                vendor: None,
                is_64bit: true,
            },
        };
        let access_token = "0".to_string();

        let cmd = LaunchCommand::build(
            &instance,
            &global_config,
            &dirs,
            &metadata,
            &account,
            &access_token,
            &java,
        )
        .expect("build legacy launch command should succeed");

        assert_eq!(cmd.java_path, java_path);
        assert_eq!(cmd.main_class, "net.minecraft.client.main.Main");

        // Legacy JVM arguments automatically provided
        assert_eq!(cmd.jvm_args[0], "-Xms512M");
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
        instance.config.custom_settings = true;
        instance.config.max_memory_mb = Some(8192);

        let global_config = LauncherConfig {
            min_memory_mb: 2048,
            max_memory_mb: 4096,
            auto_memory: false,
            ..Default::default()
        };
        let dirs = LauncherDirs::with_paths(
            PathBuf::from("/mock/config"),
            PathBuf::from("/mock/data"),
            PathBuf::from("/mock/cache"),
        );
        let metadata = mock_version_metadata("1.21.1", false, Some(21));
        let account = Account::new_offline("Steve").unwrap();
        let java_path = PathBuf::from("/mock/bin/java");
        let java = JavaInstallation {
            path: java_path.clone(),
            info: JavaInfo {
                version: "17".to_string(),
                major_version: 17,
                vendor: None,
                is_64bit: true,
            },
        };
        let access_token = "0".to_string();

        let cmd = LaunchCommand::build(
            &instance,
            &global_config,
            &dirs,
            &metadata,
            &account,
            &access_token,
            &java,
        )
        .expect("memory config should be valid");

        assert_eq!(cmd.jvm_args[0], "-Xms2048M");
        assert_eq!(cmd.jvm_args[1], "-Xmx8192M");

        // Invalid memory configuration (min > max)
        instance.config.max_memory_mb = Some(1024);

        let err = LaunchCommand::build(
            &instance,
            &global_config,
            &dirs,
            &metadata,
            &account,
            "0",
            &java,
        )
        .expect_err("min_mem > max_mem should fail");

        match err {
            CoreError::InvalidMemoryConfig { min_mb, max_mb } => {
                assert_eq!(min_mb, 2048);
                assert_eq!(max_mb, 1024);
            }
            other => panic!("expected InvalidMemoryConfig, got: {other:?}"),
        }
    }

    #[test]
    fn test_launch_command_memory_limits_without_custom_settings() {
        let mut instance = mock_instance("mem_inst_default");
        instance.config.custom_settings = false;
        instance.config.max_memory_mb = Some(8192);

        let global_config = LauncherConfig {
            min_memory_mb: 1024,
            max_memory_mb: 4096,
            auto_memory: false,
            ..Default::default()
        };
        let dirs = LauncherDirs::with_paths(
            PathBuf::from("/mock/config"),
            PathBuf::from("/mock/data"),
            PathBuf::from("/mock/cache"),
        );
        let metadata = mock_version_metadata("1.21.1", false, Some(21));
        let account = Account::new_offline("Steve").unwrap();
        let java_path = PathBuf::from("/mock/bin/java");
        let java = JavaInstallation {
            path: java_path.clone(),
            info: JavaInfo {
                version: "17".to_string(),
                major_version: 17,
                vendor: None,
                is_64bit: true,
            },
        };
        let access_token = "0".to_string();

        let cmd = LaunchCommand::build(
            &instance,
            &global_config,
            &dirs,
            &metadata,
            &account,
            &access_token,
            &java,
        )
        .expect("memory config should be valid");

        assert_eq!(cmd.jvm_args[0], "-Xms1024M");
        assert_eq!(cmd.jvm_args[1], "-Xmx4096M");
    }

    #[test]
    fn test_launch_command_extra_jvm_args() {
        let mut instance = mock_instance("extra_jvm_inst");
        instance.config.extra_jvm_args =
            vec!["-XX:+UseG1GC".to_string(), "-Dcustom.prop=true".to_string()];

        let global_config = LauncherConfig::default();
        let dirs = LauncherDirs::with_paths(
            PathBuf::from("/mock/config"),
            PathBuf::from("/mock/data"),
            PathBuf::from("/mock/cache"),
        );
        let metadata = mock_version_metadata("1.21.1", false, Some(21));
        let account = Account::new_offline("Steve").unwrap();
        let java_path = PathBuf::from("/mock/bin/java");
        let java = JavaInstallation {
            path: java_path.clone(),
            info: JavaInfo {
                version: "17".to_string(),
                major_version: 17,
                vendor: None,
                is_64bit: true,
            },
        };
        let access_token = "0".to_string();

        let cmd = LaunchCommand::build(
            &instance,
            &global_config,
            &dirs,
            &metadata,
            &account,
            &access_token,
            &java,
        )
        .expect("build should succeed");

        let len = cmd.jvm_args.len();
        assert_eq!(cmd.jvm_args[len - 2], "-XX:+UseG1GC");
        assert_eq!(cmd.jvm_args[len - 1], "-Dcustom.prop=true");
    }

    #[test]
    fn test_select_java_instance_override() {
        let mut instance = mock_instance("inst");
        instance.config.custom_settings = true;
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
    fn test_select_java_instance_override_ignored_when_custom_settings_disabled() {
        let mut instance = mock_instance("inst");
        instance.config.custom_settings = false;
        let inst_java = mock_java("/mock/instance/java", 17);
        instance.config.java = Some(inst_java);

        let default_java = mock_java("/mock/default/java", 21);
        let global_config = LauncherConfig {
            default_java: Some(default_java.clone()),
            ..Default::default()
        };

        let metadata = mock_version_metadata("1.20", false, Some(17));

        let selected = select_java(&instance, &global_config, &metadata)
            .expect("should fallback to default Java");
        assert_eq!(selected, default_java);
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

    #[test]
    fn test_launch_command_debug_redacts_access_token() {
        let cmd = LaunchCommand {
            java_path: PathBuf::from("/usr/bin/java"),
            game_dir: PathBuf::from("/path/to/game"),
            jvm_args: vec!["-Xmx2G".to_string()],
            main_class: "net.minecraft.client.main.Main".to_string(),
            game_args: vec![
                "--username".to_string(),
                "Player".to_string(),
                "--accessToken".to_string(),
                "secret_token_separate".to_string(),
                "--accessToken=secret_token_equals".to_string(),
                "--version".to_string(),
                "1.21.1".to_string(),
            ],
            env_vars: BTreeMap::from([(
                "SECRET_ENV_KEY".to_string(),
                "super_secret_env_value".to_string(),
            )]),
        };

        let debug_str = format!("{cmd:?}");
        assert!(!debug_str.contains("secret_token_separate"));
        assert!(!debug_str.contains("secret_token_equals"));
        assert!(debug_str.contains("[REDACTED]"));
        assert!(debug_str.contains("--accessToken=[REDACTED]"));
        assert!(debug_str.contains("Player"));
        assert!(debug_str.contains("SECRET_ENV_KEY"));
        assert!(!debug_str.contains("super_secret_env_value"));
    }

    #[test]
    fn test_resolve_env_vars_precedence_and_custom_settings() {
        let mesa_gpu = DiscreteGpuEnv {
            has_nvidia_driver: false,
            nvidia_egl_vendor_json: None,
        };
        let nvidia_gpu = DiscreteGpuEnv {
            has_nvidia_driver: true,
            nvidia_egl_vendor_json: Some(
                "/usr/share/glvnd/egl_vendor.d/10_nvidia.json".to_string(),
            ),
        };

        // 1. Both switches disabled -> empty even under a Wayland session
        let mut global = LauncherConfig {
            force_wayland: false,
            use_discrete_gpu: false,
            ..Default::default()
        };
        let mut instance = mock_instance("env_inst");
        assert!(LaunchCommand::resolve_env_vars(&instance, &global, true, None).is_empty());

        // 2. Global switches enabled: X11 session skips SDL_VIDEO_DRIVER=wayland, Wayland session injects it
        global.force_wayland = true;
        global.use_discrete_gpu = true;
        assert_eq!(
            LaunchCommand::resolve_env_vars(&instance, &global, false, Some(&mesa_gpu)),
            BTreeMap::from([("DRI_PRIME".to_string(), "1".to_string())])
        );
        assert_eq!(
            LaunchCommand::resolve_env_vars(&instance, &global, true, Some(&mesa_gpu)),
            BTreeMap::from([
                ("DRI_PRIME".to_string(), "1".to_string()),
                ("SDL_VIDEO_DRIVER".to_string(), "wayland".to_string()),
            ])
        );
        assert_eq!(
            LaunchCommand::resolve_env_vars(&instance, &global, true, Some(&nvidia_gpu)),
            BTreeMap::from([
                ("SDL_VIDEO_DRIVER".to_string(), "wayland".to_string()),
                (
                    "__EGL_VENDOR_LIBRARY_FILENAMES".to_string(),
                    "/usr/share/glvnd/egl_vendor.d/10_nvidia.json".to_string()
                ),
                (
                    "__GLX_VENDOR_LIBRARY_NAME".to_string(),
                    "nvidia".to_string()
                ),
                ("__NV_PRIME_RENDER_OFFLOAD".to_string(), "1".to_string()),
                (
                    "__VK_LAYER_NV_optimus".to_string(),
                    "NVIDIA_only".to_string()
                ),
            ])
        );

        // 3. custom_settings = false ignores instance toggles and instance env_vars, while global env_vars override presets
        global.env_vars = BTreeMap::from([
            ("SDL_VIDEO_DRIVER".to_string(), "x11".to_string()),
            ("SHARED".to_string(), "from_global".to_string()),
        ]);
        instance.config.custom_settings = false;
        instance.config.force_wayland = Some(false);
        instance.config.use_discrete_gpu = Some(false);
        instance.config.env_vars = BTreeMap::from([
            ("INSTANCE_ONLY".to_string(), "ignored".to_string()),
            ("SHARED".to_string(), "from_instance".to_string()),
        ]);

        assert_eq!(
            LaunchCommand::resolve_env_vars(&instance, &global, true, Some(&mesa_gpu)),
            BTreeMap::from([
                ("DRI_PRIME".to_string(), "1".to_string()),
                ("SDL_VIDEO_DRIVER".to_string(), "x11".to_string()),
                ("SHARED".to_string(), "from_global".to_string()),
            ])
        );

        // 4. custom_settings = true with None inherits global toggles and applies preset < global < instance precedence
        instance.config.custom_settings = true;
        instance.config.force_wayland = None;
        instance.config.use_discrete_gpu = None;
        assert_eq!(
            LaunchCommand::resolve_env_vars(&instance, &global, true, Some(&mesa_gpu)),
            BTreeMap::from([
                ("DRI_PRIME".to_string(), "1".to_string()),
                ("INSTANCE_ONLY".to_string(), "ignored".to_string()),
                ("SDL_VIDEO_DRIVER".to_string(), "x11".to_string()),
                ("SHARED".to_string(), "from_instance".to_string()),
            ])
        );

        // 5. custom_settings = true with Some(false) disables preset switches even when global is true
        instance.config.force_wayland = Some(false);
        instance.config.use_discrete_gpu = Some(false);
        global.env_vars.remove("SDL_VIDEO_DRIVER");
        assert_eq!(
            LaunchCommand::resolve_env_vars(&instance, &global, true, Some(&mesa_gpu)),
            BTreeMap::from([
                ("INSTANCE_ONLY".to_string(), "ignored".to_string()),
                ("SHARED".to_string(), "from_instance".to_string()),
            ])
        );
    }

    #[test]
    fn test_suggested_max_memory_mb_tiers_caps_and_monotonicity() {
        // Zero and sub-MiB values cannot be allocated
        assert_eq!(suggested_max_memory_mb(0, true), None);
        assert_eq!(suggested_max_memory_mb(MIB - 1, true), None);
        assert_eq!(suggested_max_memory_mb(0, false), None);

        // Low-memory floor: 50% clamped to [1, 512] MiB
        assert_eq!(suggested_max_memory_mb(MIB, true), Some(1));
        assert_eq!(suggested_max_memory_mb(2 * MIB, true), Some(1));
        assert_eq!(suggested_max_memory_mb(256 * MIB, true), Some(128));
        assert_eq!(suggested_max_memory_mb(512 * MIB, true), Some(256));
        assert_eq!(suggested_max_memory_mb(513 * MIB, true), Some(256));
        assert_eq!(suggested_max_memory_mb(1024 * MIB, true), Some(512));
        assert_eq!(suggested_max_memory_mb(1152 * MIB, true), Some(512));

        // First tier: usable <= 8 GiB -> usable * 4 / 5
        assert_eq!(suggested_max_memory_mb(2048 * MIB, true), Some(1228));
        assert_eq!(suggested_max_memory_mb(4096 * MIB, true), Some(2867));
        assert_eq!(suggested_max_memory_mb(8704 * MIB, true), Some(6553));

        // Second tier: usable > 8 GiB -> 6553 + (usable - 8192) / 5, capped at 16 GiB
        assert_eq!(suggested_max_memory_mb(16384 * MIB, true), Some(8089));
        assert_eq!(suggested_max_memory_mb(32768 * MIB, true), Some(11365));
        assert_eq!(suggested_max_memory_mb(65536 * MIB, true), Some(16384));
        assert_eq!(suggested_max_memory_mb(u64::MAX, true), Some(16384));

        // 32-bit JVM cap at 768 MiB
        assert_eq!(suggested_max_memory_mb(512 * MIB, false), Some(256));
        assert_eq!(suggested_max_memory_mb(1024 * MIB, false), Some(512));
        assert_eq!(suggested_max_memory_mb(2048 * MIB, false), Some(768));
        assert_eq!(suggested_max_memory_mb(32768 * MIB, false), Some(768));

        // Monotonicity across transition boundaries
        let sample_mbs = [
            1, 2, 100, 256, 511, 512, 513, 514, 768, 1024, 1152, 1365, 2048, 4096, 8703, 8704,
            8705, 16384, 32768, 65536,
        ];
        let mut prev = 0;
        for mb in sample_mbs {
            let suggested = suggested_max_memory_mb(mb * MIB, true)
                .expect("non-zero MiB should produce a suggestion");
            assert!(
                suggested >= prev,
                "expected monotonic suggestion at {mb} MiB: {suggested} < {prev}"
            );
            prev = suggested;
        }
    }

    #[test]
    fn test_resolve_memory_limits_precedence_and_clamping() {
        let mut instance = mock_instance("auto_mem_inst");
        let global = LauncherConfig {
            min_memory_mb: 1024,
            max_memory_mb: 4096,
            auto_memory: true,
            ..Default::default()
        };

        // 1. Global auto_memory enabled with 4 GiB available -> suggested 2867 MiB
        assert_eq!(
            LaunchCommand::resolve_memory_limits(&instance, &global, 4096 * MIB, true),
            (1024, 2867)
        );

        // 2. Auto memory clamps min_memory_mb down to max_mem when available RAM is low or 32-bit
        assert_eq!(
            LaunchCommand::resolve_memory_limits(&instance, &global, 1024 * MIB, true),
            (512, 512)
        );
        assert_eq!(
            LaunchCommand::resolve_memory_limits(&instance, &global, 4096 * MIB, false),
            (768, 768)
        );

        // 3. Unavailable memory info (0 bytes) falls back to configured max_memory_mb
        assert_eq!(
            LaunchCommand::resolve_memory_limits(&instance, &global, 0, true),
            (1024, 4096)
        );

        // 4. custom_settings = false ignores instance auto_memory and max_memory_mb overrides
        instance.config.custom_settings = false;
        instance.config.auto_memory = Some(false);
        instance.config.max_memory_mb = Some(6144);
        assert_eq!(
            LaunchCommand::resolve_memory_limits(&instance, &global, 4096 * MIB, true),
            (1024, 2867)
        );

        // 5. custom_settings = true with auto_memory = Some(false) uses instance max_memory_mb without clamping min
        instance.config.custom_settings = true;
        assert_eq!(
            LaunchCommand::resolve_memory_limits(&instance, &global, 4096 * MIB, true),
            (1024, 6144)
        );
        instance.config.max_memory_mb = Some(512);
        assert_eq!(
            LaunchCommand::resolve_memory_limits(&instance, &global, 4096 * MIB, true),
            (1024, 512)
        );

        // 6. custom_settings = true with auto_memory = None inherits global auto_memory
        instance.config.auto_memory = None;
        assert_eq!(
            LaunchCommand::resolve_memory_limits(&instance, &global, 16384 * MIB, true),
            (1024, 8089)
        );

        // 7. Global auto_memory = false, instance override auto_memory = Some(true)
        let global_manual = LauncherConfig {
            min_memory_mb: 1024,
            max_memory_mb: 4096,
            auto_memory: false,
            ..Default::default()
        };
        instance.config.auto_memory = Some(true);
        assert_eq!(
            LaunchCommand::resolve_memory_limits(&instance, &global_manual, 16384 * MIB, true),
            (1024, 8089)
        );
    }

    #[test]
    fn test_launch_command_fullscreen_flag_precedence_and_dedup() {
        let mut instance = mock_instance("fullscreen_inst");
        let dirs = LauncherDirs::with_paths(
            PathBuf::from("/mock/config"),
            PathBuf::from("/mock/data"),
            PathBuf::from("/mock/cache"),
        );
        let account = Account::new_offline("Steve").expect("failed to create account");
        let java = mock_java("/mock/java/bin/java", 21);

        // 1. Disabled by default
        let mut global = LauncherConfig {
            auto_memory: false,
            fullscreen: false,
            ..Default::default()
        };
        let metadata = mock_version_metadata("1.21.1", false, Some(21));
        let cmd = LaunchCommand::build(&instance, &global, &dirs, &metadata, &account, "0", &java)
            .expect("build should succeed");
        assert!(!cmd.game_args.iter().any(|arg| arg == "--fullscreen"));

        // 2. Global fullscreen enabled injects --fullscreen
        global.fullscreen = true;
        let cmd = LaunchCommand::build(&instance, &global, &dirs, &metadata, &account, "0", &java)
            .expect("build should succeed");
        assert_eq!(
            cmd.game_args
                .iter()
                .filter(|arg| arg.as_str() == "--fullscreen")
                .count(),
            1
        );

        // 3. custom_settings = false ignores instance fullscreen override
        instance.config.custom_settings = false;
        instance.config.fullscreen = Some(false);
        let cmd = LaunchCommand::build(&instance, &global, &dirs, &metadata, &account, "0", &java)
            .expect("build should succeed");
        assert!(cmd.game_args.iter().any(|arg| arg == "--fullscreen"));

        // 4. custom_settings = true respects instance fullscreen = Some(false)
        instance.config.custom_settings = true;
        let cmd = LaunchCommand::build(&instance, &global, &dirs, &metadata, &account, "0", &java)
            .expect("build should succeed");
        assert!(!cmd.game_args.iter().any(|arg| arg == "--fullscreen"));

        // 5. custom_settings = true with Some(true) overrides global false
        global.fullscreen = false;
        instance.config.fullscreen = Some(true);
        let cmd = LaunchCommand::build(&instance, &global, &dirs, &metadata, &account, "0", &java)
            .expect("build should succeed");
        assert_eq!(
            cmd.game_args
                .iter()
                .filter(|arg| arg.as_str() == "--fullscreen")
                .count(),
            1
        );

        // 6. Does not duplicate --fullscreen if already present in metadata game arguments
        let mut metadata_with_fs = mock_version_metadata("1.21.1", false, Some(21));
        if let Some(args) = &mut metadata_with_fs.arguments {
            args.game
                .push(ArgumentEntry::Simple("--fullscreen".to_string()));
        }
        let cmd = LaunchCommand::build(
            &instance,
            &global,
            &dirs,
            &metadata_with_fs,
            &account,
            "0",
            &java,
        )
        .expect("build should succeed");
        assert_eq!(
            cmd.game_args
                .iter()
                .filter(|arg| arg.as_str() == "--fullscreen")
                .count(),
            1
        );
    }
}
