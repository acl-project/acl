// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use acl_core::{
    CoreError, Instance, LauncherConfig, LauncherDirs, ModLoader, launch,
    manager::InstanceManager,
    task::{ProgressUnit, TaskId, TaskManager},
};
use acl_minecraft::{
    AssetFile, AssetIndex, AssetItemData, AssetVersion, DownloadSource, MetaLoader,
    MetaLoaderEntry, MinecraftError, ModpackLoader, ModpackManifest, VERSION_MANIFEST_URL,
    VersionEntry, VersionManifest, VersionMetadata,
    download::{DownloadProgress, DownloadTask, download_file_with_progress},
    forge::{self, ForgeManifest, ModernInstallProfile, NeoForgeManifest},
};

use relm4::{SharedState, tokio};
use tracing::warn;

use crate::{i18n::LocalizeError, pages::installer::asset_config::InstallTarget, t};

pub struct InstallContext {
    pub mgr: Arc<SharedState<InstanceManager>>,
    pub client: reqwest::Client,
    pub dirs: LauncherDirs,
    pub config: Arc<SharedState<LauncherConfig>>,
    pub active_install: Arc<Mutex<Option<String>>>,
}

#[derive(Debug, Clone)]
struct CoreInstallStages {
    metadata: usize,
    asset_index: usize,
    download: usize,
    runtime: usize,
}

struct CoreInstallEnv {
    client: reqwest::Client,
    dirs: LauncherDirs,
    global_config: LauncherConfig,
    task_id: TaskId,
    task_mgr: Arc<TaskManager>,
}

/// The context used during the installation phase
struct InstallerPostContext {
    installer_path: PathBuf,
    temp_dir: PathBuf,
    modern: Option<ModernInstallProfile>,
}

impl Drop for InstallerPostContext {
    fn drop(&mut self) {
        // Clean up the temporary files generated
        let _ = std::fs::remove_dir_all(&self.temp_dir);
    }
}

struct TempDirGuard {
    path: PathBuf,
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[derive(Debug, Clone)]
pub enum ModpackInstallSource {
    Online(AssetFile),
    Local(PathBuf),
}

/// Install Fabric or Quilt
async fn install_fabric(
    client: &reqwest::Client,
    instance: &Instance,
    download_source: DownloadSource,
    metadata: &mut VersionMetadata,
    entry: &VersionEntry,
    id: &str,
) -> Result<(), String> {
    let mod_loader = instance.config.mod_loader;
    let meta_loader = match mod_loader {
        ModLoader::Quilt => MetaLoader::Quilt,
        _ => MetaLoader::Fabric,
    };

    if let Some(loader_ver) = &instance.config.mod_loader_version {
        let patch = MetaLoaderEntry::fetch_profile(
            meta_loader,
            client,
            &entry.id,
            loader_ver,
            &instance.loader_json(),
            download_source,
        )
        .await
        .map_err(|e| e.to_i18n())?;

        metadata.apply_patch(patch);
    } else {
        warn!(
            "Mod loader {} is configured without loader version for instance {}",
            mod_loader.label(),
            id,
        );
    }

    Ok(())
}

/// Install Forge or NeoForge
async fn install_forge(
    client: &reqwest::Client,
    instance: &Instance,
    dirs: &LauncherDirs,
    download_source: DownloadSource,
    metadata: &mut VersionMetadata,
    id: &str,
    installer_context: &mut Option<InstallerPostContext>,
) -> Result<(), String> {
    let loader = instance.config.mod_loader;
    if let Some(loader_ver) = &instance.config.mod_loader_version {
        let raw_url = match loader {
            ModLoader::NeoForge => NeoForgeManifest::download_url(loader_ver),
            _ => ForgeManifest::download_url(loader_ver),
        };

        let url = download_source.rewrite_url(&raw_url);

        let temp_dir = std::env::temp_dir().join(format!(
            "acl-{}-{}",
            loader.label().to_ascii_lowercase(),
            id
        ));

        tokio::fs::create_dir_all(&temp_dir)
            .await
            .map_err(|e| format!("Failed to create temp dir: {e}"))?;

        let installer_path = temp_dir.join("installer.jar");

        forge::download_installer(client, &url, &installer_path)
            .await
            .map_err(|e| e.to_i18n())?;

        let installer = forge::extract_installer(&installer_path, &dirs.libraries_dir(), &temp_dir)
            .map_err(|e| e.to_i18n())?;

        // Save as loader.json
        let patch_json =
            serde_json::to_string_pretty(&installer.patch).map_err(|e| e.to_string())?;
        tokio::fs::write(instance.loader_json(), patch_json)
            .await
            .map_err(|e| e.to_string())?;

        metadata.apply_patch(installer.patch);

        // Add processor dependency library
        if let Some(modern) = &installer.modern {
            metadata.libraries.extend(modern.libraries.clone());
        }

        *installer_context = Some(InstallerPostContext {
            installer_path,
            temp_dir,
            modern: installer.modern,
        });
    } else {
        warn!(
            "Mod loader {} is configured without loader version for instance {}",
            loader.label(),
            id
        );
    }

    Ok(())
}

struct InstallGuard {
    active_install: Arc<Mutex<Option<String>>>,
    instance: Instance,
    completed: bool,
}

impl InstallGuard {
    /// Attempt to perform instance installation tasks
    /// Ensures that there will only be one instance installation task at the same time
    fn acquire(
        active_install: &Arc<Mutex<Option<String>>>,
        instance: &Instance,
    ) -> Result<Self, String> {
        let mut active = active_install.lock().unwrap_or_else(|e| e.into_inner());
        if active.is_some() {
            return Err(t!(
                "install-error-already-in-progress",
                "Another installation is already in progress"
            ));
        }

        if instance.dir.exists() {
            return Err(CoreError::InstanceAlreadyExists {
                id: instance.id().to_owned(),
            }
            .to_i18n());
        }
        std::fs::create_dir_all(&instance.dir).map_err(|e| e.to_string())?;
        if let Err(err) = instance.mark_installing() {
            let _ = instance.delete();
            return Err(err.to_i18n());
        }
        *active = Some(instance.id().to_owned());
        Ok(Self {
            active_install: Arc::clone(active_install),
            instance: instance.clone(),
            completed: false,
        })
    }
}

/// Automatically release active and clean the file if it fails.
impl Drop for InstallGuard {
    fn drop(&mut self) {
        let mut active = self
            .active_install
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        // Only release the slot if it still belongs to this install, preventing a
        // rejected or duplicate task from clearing another running installation's lock.
        if active.as_deref() == Some(self.instance.id()) {
            *active = None;
        }

        drop(active);

        if !self.completed && self.instance.has_install_mark() {
            let _ = self.instance.delete();
        }
    }
}

/// Download a batch of tasks concurrently and forward file-count progress to a task stage.
async fn download_tasks_with_progress(
    env: &CoreInstallEnv,
    tasks: Vec<DownloadTask>,
    stage_idx: usize,
) -> Result<(), String> {
    env.task_mgr.start_stage(env.task_id, stage_idx);
    // Create a channel to obtain and asynchronously send download progress.
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DownloadProgress>();

    let mgr_prog = Arc::clone(&env.task_mgr);
    let task_id = env.task_id;

    let forward_handle = tokio::spawn(async move {
        while let Some(prog) = rx.recv().await {
            let done = prog.completed_files as u64;
            let total = prog.total_files as u64;
            mgr_prog.update_stage(task_id, stage_idx, done, total, None);
        }
    });

    let result = acl_minecraft::download::download_all(
        &env.client,
        tasks,
        env.global_config.download_source,
        env.global_config.max_concurrent_downloads,
        Some(tx),
    )
    .await
    .map_err(|e| e.to_i18n());
    // Ownership of tx has already been moved; tx will be dropped once the download completes.
    let _ = forward_handle.await;

    result
}

/// Run the shared core installation pipeline:
/// metadata + mod loader -> asset index -> game file downloads -> runtime & processors.
async fn install_game_core(
    env: &CoreInstallEnv,
    instance: &Instance,
    entry: &VersionEntry,
    stages: CoreInstallStages,
) -> Result<(), String> {
    let download_source = env.global_config.download_source;

    // Fetch metadata
    env.task_mgr.start_stage(env.task_id, stages.metadata);

    let mut metadata = instance
        .fetch_metadata(&env.client, entry, &env.dirs, download_source)
        .await
        .map_err(|e| e.to_i18n())?;

    let mut installer_context = None;

    // Handle mod loader
    match instance.config.mod_loader {
        ModLoader::Vanilla => {}
        ModLoader::Fabric | ModLoader::Quilt => {
            install_fabric(
                &env.client,
                instance,
                download_source,
                &mut metadata,
                entry,
                instance.id(),
            )
            .await?
        }
        ModLoader::Forge | ModLoader::NeoForge => {
            install_forge(
                &env.client,
                instance,
                &env.dirs,
                download_source,
                &mut metadata,
                instance.id(),
                &mut installer_context,
            )
            .await?
        }
    }

    // Fetch asset index
    env.task_mgr.start_stage(env.task_id, stages.asset_index);

    let asset_index = AssetIndex::fetch_and_verify(
        &env.client,
        &metadata.asset_index,
        &env.dirs.assets_dir(),
        download_source,
    )
    .await
    .map_err(|e| e.to_i18n())?;

    // Collect download tasks
    let tasks = instance.download_tasks(&metadata, &asset_index, &env.dirs);
    download_tasks_with_progress(env, tasks, stages.download).await?;

    env.task_mgr.start_stage(env.task_id, stages.runtime);

    let inst_c = instance.clone();
    let dirs_c = env.dirs.clone();

    let metadata_c = metadata.clone();

    // Prepare runtime

    // Don't put it in spawn_blocking, otherwise InstallGuard::drop and prepare_runtime will race.
    inst_c
        .prepare_runtime(&metadata_c, &asset_index, &dirs_c)
        .map_err(|e| e.to_i18n())?;

    // handle.abort() will only take effect in the next .await
    // If click Cancel in Prepare runtime, without this line,
    // the configuration file will be written directly.
    tokio::task::yield_now().await;

    // Run the forge processors (1.13+)
    if let Some(ctx) = installer_context
        && let Some(modern) = &ctx.modern
    {
        let java = launch::select_java(instance, &env.global_config, &metadata)
            .map_err(|e| e.to_i18n())?;

        modern
            .run_processors(
                &java.path,
                &ctx.installer_path,
                &instance.client_jar(),
                &env.dirs.libraries_dir(),
                &ctx.temp_dir,
            )
            .await
            .map_err(|e| e.to_i18n())?;
    }

    Ok(())
}

/// Persist config, clear the install mark, release the guard, and refresh the instance list.
fn finalize_install(
    mut guard: InstallGuard,
    instance: &Instance,
    instance_mgr: &Arc<SharedState<InstanceManager>>,
) -> Result<(), String> {
    // Successfully installed, write aclcfg.toml and refresh the list
    instance.save_config().map_err(|e| e.to_i18n())?;
    instance.clear_installing().map_err(|e| e.to_i18n())?;
    guard.completed = true;
    // Release immediately installing = false (completed, directory will not be deleted)
    drop(guard);
    instance_mgr
        .write()
        .refresh(false)
        .map_err(|e| e.to_i18n())?;
    Ok(())
}

pub fn spawn_install(
    tasks: &Arc<TaskManager>,
    instance: Instance,
    entry: VersionEntry,
    ctx: InstallContext,
) {
    let stage_labels = vec![
        t!("install-stage-fetching-metadata", "Fetching metadata"),
        t!("install-stage-fetching-asset-index", "Fetching asset index"),
        t!("install-stage-downloading-files", "Downloading files"),
        t!("install-stage-preparing-runtime", "Preparing runtime"),
    ];
    const STAGES: CoreInstallStages = CoreInstallStages {
        metadata: 0,
        asset_index: 1,
        download: 2,
        runtime: 3,
    };

    tasks.submit(
        t!(
            "tasks-install-instance",
            "Install {version}",
            version = instance.id()
        ),
        stage_labels,
        move |task_id, task_mgr| {
            let instance = instance.clone();
            let entry = entry.clone();
            let instance_mgr = Arc::clone(&ctx.mgr);
            let active_install = Arc::clone(&ctx.active_install);
            // Each retry task uses the latest configuration file
            // If the player switches the download source, it can also ensure that the latest download source is used.
            let env = CoreInstallEnv {
                client: ctx.client.clone(),
                dirs: ctx.dirs.clone(),
                global_config: ctx.config.read().clone(),
                task_id,
                task_mgr,
            };

            async move {
                let guard = InstallGuard::acquire(&active_install, &instance)?;
                install_game_core(&env, &instance, &entry, STAGES).await?;
                finalize_install(guard, &instance, &instance_mgr)
            }
        },
    );
}

async fn prepare_modpack_source(
    env: &CoreInstallEnv,
    source: &ModpackInstallSource,
    instance_id: &str,
) -> Result<(PathBuf, ModpackManifest, Option<TempDirGuard>), String> {
    match source {
        ModpackInstallSource::Local(path) => {
            let manifest = ModpackManifest::parse_from_zip(path).map_err(|e| e.to_i18n())?;
            Ok((path.clone(), manifest, None))
        }
        ModpackInstallSource::Online(file) => {
            const STAGE_DOWNLOAD_MODPACK: usize = 0;

            env.task_mgr
                .set_stage_unit(env.task_id, STAGE_DOWNLOAD_MODPACK, ProgressUnit::Bytes);
            env.task_mgr
                .start_stage(env.task_id, STAGE_DOWNLOAD_MODPACK);

            let temp_dir = std::env::temp_dir().join(format!("acl-modpack-{instance_id}"));
            tokio::fs::create_dir_all(&temp_dir)
                .await
                .map_err(|source| {
                    acl_minecraft::MinecraftError::Io {
                        path: temp_dir.clone(),
                        source,
                    }
                    .to_i18n()
                })?;

            // Wrap immediately so any subsequent download/parse failure or cancellation cleans up temp_dir.
            let guard = TempDirGuard {
                path: temp_dir.clone(),
            };

            let archive_path = temp_dir.join("pack.mrpack");
            let task = DownloadTask {
                name: file.filename.clone(),
                url: file.url.clone(),
                destination: archive_path.clone(),
                sha1: file.sha1.clone().unwrap_or_default(),
                size: file.size,
            };

            download_file_with_progress(
                &env.client,
                &task,
                env.global_config.download_source,
                |done, total| {
                    env.task_mgr.update_stage(
                        env.task_id,
                        STAGE_DOWNLOAD_MODPACK,
                        done,
                        total,
                        None,
                    );
                },
            )
            .await
            .map_err(|e| e.to_i18n())?;

            let manifest =
                ModpackManifest::parse_from_zip(&archive_path).map_err(|e| e.to_i18n())?;

            Ok((archive_path, manifest, Some(guard)))
        }
    }
}

async fn resolve_modpack_metadata(
    env: &CoreInstallEnv,
    manifest: &ModpackManifest,
    instance: &mut Instance,
    metadata_stage: usize,
) -> Result<VersionEntry, String> {
    env.task_mgr.start_stage(env.task_id, metadata_stage);
    let download_source = env.global_config.download_source;
    let version_manifest = VersionManifest::fetch_with_cache(
        &env.client,
        VERSION_MANIFEST_URL,
        env.dirs.cache_dir(),
        download_source,
        false,
    )
    .await
    .map_err(|e| e.to_i18n())?;

    let entry = version_manifest
        .find_version(&manifest.game_version)
        .ok_or(
            MinecraftError::VersionNotFound {
                version_id: manifest.game_version.clone(),
            }
            .to_i18n(),
        )?;

    instance.config.game_version = manifest.game_version.clone();

    let (loader, loader_ver) = match &manifest.loader {
        None => (ModLoader::Vanilla, None),
        Some(ModpackLoader::Fabric(v)) => (ModLoader::Fabric, Some(v.clone())),
        Some(ModpackLoader::Quilt(v)) => (ModLoader::Quilt, Some(v.clone())),
        Some(ModpackLoader::Forge(raw)) => {
            let forge_manifest = ForgeManifest::fetch_with_cache(
                &env.client,
                env.dirs.cache_dir(),
                download_source,
                false,
            )
            .await
            .map_err(|e| e.to_i18n())?;

            let v = forge_manifest
                .resolve_version(&manifest.game_version, raw)
                .ok_or_else(|| {
                    t!(
                        "install-error-resolve-forge-version",
                        "Failed to resolve forge version {raw}",
                        raw = raw
                    )
                })?;

            (ModLoader::Forge, Some(v))
        }
        Some(ModpackLoader::NeoForge(raw)) => {
            let neoforge_manifest = NeoForgeManifest::fetch_with_cache(
                &env.client,
                &manifest.game_version,
                env.dirs.cache_dir(),
                download_source,
                false,
            )
            .await
            .map_err(|e| e.to_i18n())?;

            let v = neoforge_manifest
                .resolve_version(&manifest.game_version, raw)
                .ok_or_else(|| {
                    t!(
                        "install-error-resolve-neoforge-version",
                        "Failed to resolve neoforge version {raw}",
                        raw = raw,
                    )
                })?;

            (ModLoader::NeoForge, Some(v))
        }
    };

    instance.config.mod_loader = loader;
    instance.config.mod_loader_version = loader_ver;

    Ok(entry.clone())
}

pub fn spawn_modpack_install(
    tasks: &Arc<TaskManager>,
    instance_name: String,
    source: ModpackInstallSource,
    ctx: InstallContext,
) {
    let mut stage_labels = Vec::with_capacity(7);
    let (core_stages, stage_modpack_files, stage_overrides) = {
        let mut add_stage = |label: String| -> usize {
            let idx = stage_labels.len();
            stage_labels.push(label);
            idx
        };

        if matches!(source, ModpackInstallSource::Online(_)) {
            add_stage(t!(
                "install-stage-downloading-modpack",
                "Downloading modpack"
            ));
        }

        let metadata = add_stage(t!("install-stage-fetching-metadata", "Fetching metadata"));
        let asset_index = add_stage(t!(
            "install-stage-fetching-asset-index",
            "Fetching asset index"
        ));
        let download = add_stage(t!("install-stage-downloading-files", "Downloading files"));
        let runtime = add_stage(t!("install-stage-preparing-runtime", "Preparing runtime"));
        let core_stages = CoreInstallStages {
            metadata,
            asset_index,
            download,
            runtime,
        };
        let stage_modpack_files = add_stage(t!(
            "install-stage-downloading-modpack-files",
            "Downloading modpack files"
        ));
        let stage_overrides =
            add_stage(t!("install-stage-applying-overrides", "Applying overrides"));

        (core_stages, stage_modpack_files, stage_overrides)
    };

    tasks.submit(
        t!(
            "tasks-install-instance",
            "Install {version}",
            version = instance_name
        ),
        stage_labels,
        move |task_id, task_mgr| {
            let core_stages = core_stages.clone();
            let instance_name = instance_name.clone();
            let source = source.clone();
            let instance_mgr = Arc::clone(&ctx.mgr);
            let active_install = Arc::clone(&ctx.active_install);
            let env = CoreInstallEnv {
                client: ctx.client.clone(),
                dirs: ctx.dirs.clone(),
                global_config: ctx.config.read().clone(),
                task_id,
                task_mgr,
            };

            async move {
                let mut instance =
                    Instance::new(&instance_name, &instance_name, "", &env.dirs.versions_dir())
                        .map_err(|e| e.to_i18n())?;

                let guard = InstallGuard::acquire(&active_install, &instance)?;

                let (mrpack_path, manifest, _temp_guard) =
                    prepare_modpack_source(&env, &source, instance.id()).await?;

                let entry =
                    resolve_modpack_metadata(&env, &manifest, &mut instance, core_stages.metadata)
                        .await?;

                install_game_core(&env, &instance, &entry, core_stages).await?;

                let modpack_tasks = manifest.download_tasks(instance.game_dir(), true);
                download_tasks_with_progress(&env, modpack_tasks, stage_modpack_files).await?;

                // Apply overrides last so pack customizations take precedence over downloaded files.
                env.task_mgr.start_stage(env.task_id, stage_overrides);
                manifest
                    .extract_overrides(&mrpack_path, instance.game_dir())
                    .map_err(|e| e.to_i18n())?;

                // Ensure cancellation takes effect before persisting config and marking complete.
                tokio::task::yield_now().await;

                finalize_install(guard, &instance, &instance_mgr)
            }
        },
    );
}

pub fn spawn_asset_download(
    tasks: &Arc<TaskManager>,
    item: AssetItemData,
    version: AssetVersion,
    target: InstallTarget,
    client: reqwest::Client,
    config: Arc<SharedState<LauncherConfig>>,
) {
    let Some(file) = version.primary_file.clone() else {
        return;
    };

    let dest_dir = match target {
        InstallTarget::CustomDir(dir) => dir,
        InstallTarget::Instance(inst) => match inst.asset_dir(item.project_type) {
            Some(dir) => dir,
            None => {
                return;
            }
        },
    };

    let download_task = DownloadTask {
        name: file.filename.clone(),
        url: file.url,
        destination: dest_dir.join(&file.filename),
        sha1: file.sha1.unwrap_or_default(),
        size: file.size,
    };

    let stage_labels = vec![t!("install-stage-downloading-files", "Downloading files")];
    const STAGE_DOWNLOAD: usize = 0;

    tasks.submit(
        t!("tasks-download-asset", "Download {name}", name = item.title),
        stage_labels,
        move |task_id, task_mgr| {
            let client = client.clone();
            let download_task = download_task.clone();
            let download_source = config.read().download_source;

            async move {
                task_mgr.set_stage_unit(task_id, STAGE_DOWNLOAD, ProgressUnit::Bytes);

                task_mgr.start_stage(task_id, STAGE_DOWNLOAD);

                download_file_with_progress(
                    &client,
                    &download_task,
                    download_source,
                    |done, total: u64| {
                        task_mgr.update_stage(task_id, STAGE_DOWNLOAD, done, total, None);
                    },
                )
                .await
                .map_err(|e| e.to_i18n())?;
                Ok(())
            }
        },
    );
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
                "acl_test_install_{}_{}_{}",
                name,
                std::process::id(),
                id
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("failed to create temp test dir");
            Self { path }
        }

        fn path(&self) -> &std::path::Path {
            &self.path
        }
    }

    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn test_install_guard_acquire_concurrency_and_cleanup() {
        let temp = TempDirGuard::new("guard_lifecycle");
        let active_install: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

        let inst_a = Instance::new("inst_a", "Inst A", "1.21.1", temp.path())
            .expect("should construct inst_a");
        let inst_b = Instance::new("inst_b", "Inst B", "1.21.1", temp.path())
            .expect("should construct inst_b");

        let guard = InstallGuard::acquire(&active_install, &inst_a)
            .expect("first install guard should succeed");
        assert!(inst_a.dir.exists());
        assert!(inst_a.has_install_mark());
        assert_eq!(active_install.lock().unwrap().as_deref(), Some("inst_a"));

        // Concurrent install while inst_a is active should fail without clearing inst_a's lock.
        let concurrent = InstallGuard::acquire(&active_install, &inst_b);
        assert!(concurrent.is_err());
        assert!(!inst_b.dir.exists());
        assert_eq!(active_install.lock().unwrap().as_deref(), Some("inst_a"));

        // Dropping an uncompleted guard removes the marked directory and clears active_install.
        drop(guard);
        assert!(!inst_a.dir.exists());
        assert_eq!(*active_install.lock().unwrap(), None);
    }

    #[test]
    fn test_install_guard_preserves_existing_dir_and_completed_install() {
        let temp = TempDirGuard::new("guard_existing_and_completed");
        let active_install: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

        let existing = Instance::new("existing", "Existing", "1.21.1", temp.path())
            .expect("should construct existing");
        existing
            .save_config()
            .expect("should save existing instance config");

        // Acquiring over an existing directory fails and leaves the existing directory untouched.
        let err = InstallGuard::acquire(&active_install, &existing);
        assert!(err.is_err());
        assert!(existing.dir.exists());
        assert!(existing.config_file().exists());
        assert_eq!(*active_install.lock().unwrap(), None);

        // Completed install clears the mark and preserves the instance directory on drop.
        let completed = Instance::new("completed", "Completed", "1.21.1", temp.path())
            .expect("should construct completed");
        let mut guard = InstallGuard::acquire(&active_install, &completed)
            .expect("guard should acquire for new instance");
        completed.save_config().expect("should save config");
        completed
            .clear_installing()
            .expect("should clear install mark");
        guard.completed = true;
        drop(guard);

        assert!(completed.dir.exists());
        assert!(completed.config_file().exists());
        assert!(!completed.has_install_mark());
        assert_eq!(*active_install.lock().unwrap(), None);
    }
}
