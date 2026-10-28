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
            .map_or(global_config.max_memory_mb, |mb| mb);

        let min_mem = instance
            .config
            .min_memory_mb
            .map_or(global_config.min_memory_mb, |mb| mb);

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
