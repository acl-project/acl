// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use acl_core::{
    CoreError, Instance, LauncherConfig, LauncherDirs, ModLoader, launch, manager::InstanceManager,
    task::TaskManager,
};
use acl_minecraft::{
    AssetIndex, DownloadSource, MetaLoader, MetaLoaderEntry, VersionEntry, VersionMetadata,
    download::DownloadProgress,
    forge::{self, ForgeManifest, ModernInstallProfile, NeoForgeManifest},
};

use relm4::{SharedState, tokio};
use tracing::warn;

use crate::{i18n::LocalizeError, t};

pub struct InstallContext {
    pub mgr: Arc<SharedState<InstanceManager>>,
    pub client: reqwest::Client,
    pub dirs: LauncherDirs,
    pub config: Arc<SharedState<LauncherConfig>>,
    pub active_install: Arc<Mutex<Option<String>>>,
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
    const STAGE_METADATA: usize = 0;
    const STAGE_ASSET_INDEX: usize = 1;
    const STAGE_DOWNLOAD: usize = 2;
    const STAGE_RUNTIME: usize = 3;

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

            let client = ctx.client.clone();
            let dirs = ctx.dirs.clone();
            // Each retry task uses the latest configuration file
            // If the player switches the download source, it can also ensure that the latest download source is used.
            let global_config = ctx.config.read().clone();
            let instance_mgr = Arc::clone(&ctx.mgr);
            let active_install = Arc::clone(&ctx.active_install);
            let max_concurrent = global_config.max_concurrent_downloads;
            let download_source = global_config.download_source;

            async move {
                let mut guard = InstallGuard::acquire(&active_install, &instance)?;

                // Fetch metadata
                task_mgr.start_stage(task_id, STAGE_METADATA);

                let mut metadata = instance
                    .fetch_metadata(&client, &entry, &dirs, download_source)
                    .await
                    .map_err(|e| e.to_i18n())?;

                let mut installer_context = None;

                // Handle mod loader
                match instance.config.mod_loader {
                    ModLoader::Vanilla => {}
                    ModLoader::Fabric | ModLoader::Quilt => {
                        install_fabric(
                            &client,
                            &instance,
                            download_source,
                            &mut metadata,
                            &entry,
                            instance.id(),
                        )
                        .await?
                    }
                    ModLoader::Forge | ModLoader::NeoForge => {
                        install_forge(
                            &client,
                            &instance,
                            &dirs,
                            download_source,
                            &mut metadata,
                            instance.id(),
                            &mut installer_context,
                        )
                        .await?
                    }
                }

                // Fetch asset index

                task_mgr.start_stage(task_id, STAGE_ASSET_INDEX);

                let asset_index = AssetIndex::fetch_and_verify(
                    &client,
                    &metadata.asset_index,
                    &dirs.assets_dir(),
                    download_source,
                )
                .await
                .map_err(|e| e.to_i18n())?;

                // Collect download tasks
                let tasks = instance.download_tasks(&metadata, &asset_index, &dirs);

                task_mgr.start_stage(task_id, STAGE_DOWNLOAD);

                // Create a channel to obtain and asynchronously send download progress.
                let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DownloadProgress>();

                let mgr_prog = Arc::clone(&task_mgr);

                let forward_handle = tokio::spawn(async move {
                    while let Some(prog) = rx.recv().await {
                        let done = prog.completed_files as u64;
                        let total = prog.total_files as u64;
                        mgr_prog.update_stage(task_id, STAGE_DOWNLOAD, done, total, None);
                    }
                });

                acl_minecraft::download::download_all(
                    &client,
                    tasks,
                    download_source,
                    max_concurrent,
                    Some(tx),
                )
                .await
                .map_err(|e| e.to_i18n())?;
                // Ownership of tx has already been moved; tx will be dropped once the download completes.
                let _ = forward_handle.await;

                task_mgr.start_stage(task_id, STAGE_RUNTIME);

                let inst_c = instance.clone();
                let dirs_c = dirs.clone();

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
                    let java = launch::select_java(&instance, &global_config, &metadata)
                        .map_err(|e| e.to_i18n())?;

                    modern
                        .run_processors(
                            &java.path,
                            &ctx.installer_path,
                            &instance.client_jar(),
                            &dirs.libraries_dir(),
                            &ctx.temp_dir,
                        )
                        .await
                        .map_err(|e| e.to_i18n())?;
                }

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
        },
    );
}
