// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{path::PathBuf, sync::Arc};

use acl_core::{
    Instance, LauncherConfig, LauncherDirs, ModLoader, launch, manager::InstanceManager,
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

pub fn spawn_install(
    tasks: &Arc<TaskManager>,
    mgr: Arc<SharedState<InstanceManager>>,
    instance: Instance,
    entry: VersionEntry,
    client: reqwest::Client,
    dirs: LauncherDirs,
    config: LauncherConfig,
) {
    let max_concurrent = config.max_concurrent_downloads;
    let download_source = config.download_source;

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
        format!("Install {}", instance.id()),
        stage_labels,
        move |task_id, task_mgr| {
            let instance = instance.clone();
            let entry = entry.clone();

            let client = client.clone();
            let dirs = dirs.clone();
            let global_config = config.clone();
            let instance_mgr = Arc::clone(&mgr);

            async move {
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
                tokio::task::spawn_blocking(move || {
                    inst_c.prepare_runtime(&metadata_c, &asset_index, &dirs_c)
                })
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_i18n())?;

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
                instance_mgr.write().refresh().map_err(|e| e.to_i18n())?;
                Ok(())
            }
        },
    );
}
