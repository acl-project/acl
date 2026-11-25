// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

pub mod asset_config;
pub mod configure;
pub mod loader_select;
pub mod manual;
pub mod modpack_config;
pub mod version_select;

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use acl_core::{
    CoreError, Instance, InstanceConfig, LauncherConfig, ModLoader, manager::InstanceManager,
};
use acl_minecraft::{
    AssetItemData, AssetVersion, DownloadSource, ModpackManifest, ProjectHost, ProjectType,
    VersionEntry, curseforge, modrinth,
};

use relm4::{
    Component, ComponentController, ComponentParts, ComponentSender, Controller, SharedState,
};
use tracing::{error, info};

use crate::{
    i18n::LocalizeError,
    install::ModpackInstallSource,
    pages::installer::{
        asset_config::{
            AssetConfigInit, AssetConfigInput, AssetConfigOutput, AssetConfigPage, InstallTarget,
            ResolvedDep,
        },
        configure::{ConfigureInit, ConfigureInput, ConfigureOutput, ConfigurePage},
        loader_select::{
            LoaderSelectInit, LoaderSelectInput, LoaderSelectOutput, LoaderSelectPage,
        },
        manual::{ManualInit, ManualInput, ManualOutput, ManualPage},
        modpack_config::{ModpackInit, ModpackInput, ModpackOutput, ModpackPage, ModpackSource},
        version_select::{
            VersionSelectInit, VersionSelectInput, VersionSelectOutput, VersionSelectPage,
        },
    },
    t,
};

async fn fetch_asset_versions(
    client: &reqwest::Client,
    host: ProjectHost,
    project_id: &str,
    cache_dir: &Path,
    download_source: DownloadSource,
    force_refresh: bool,
) -> Result<Vec<AssetVersion>, String> {
    match host {
        ProjectHost::Modrinth => modrinth::ProjectVersion::fetch_list_with_cache(
            client,
            project_id,
            &[],
            &[],
            cache_dir,
            download_source,
            force_refresh,
        )
        .await
        .map(|list| list.into_iter().map(AssetVersion::from).collect())
        .map_err(|e| e.to_i18n()),
        ProjectHost::CurseForge => {
            let mod_id = curseforge::parse_project_id(project_id).map_err(|e| e.to_i18n())?;

            curseforge::ModFile::fetch_list_with_cache(
                client,
                mod_id,
                None,
                None,
                cache_dir,
                force_refresh,
            )
            .await
            .map(|list| list.into_iter().map(AssetVersion::from).collect())
            .map_err(|e| e.to_i18n())
        }
    }
}

async fn fetch_project_details(
    client: &reqwest::Client,
    host: ProjectHost,
    ids: &[String],
    cache_dir: &Path,
    download_source: DownloadSource,
    force_refresh: bool,
) -> Result<HashMap<String, AssetItemData>, String> {
    match host {
        ProjectHost::Modrinth => modrinth::ProjectDetail::fetch_batch_with_cache(
            client,
            ids,
            cache_dir,
            download_source,
            force_refresh,
        )
        .await
        .map(|list| {
            list.into_iter()
                .map(|detail| {
                    let id = detail.id.clone();
                    (id, AssetItemData::from(detail))
                })
                .collect()
        })
        .map_err(|e| e.to_i18n()),
        ProjectHost::CurseForge => {
            let mod_ids: Vec<u64> = ids
                .iter()
                .map(|id| curseforge::parse_project_id(id))
                .collect::<acl_minecraft::Result<Vec<_>>>()
                .map_err(|e| e.to_i18n())?;

            curseforge::fetch_projects_batch_with_cache(client, &mod_ids, cache_dir, force_refresh)
                .await
                .map(|list| {
                    list.into_iter()
                        .map(|info| (info.id.to_string(), AssetItemData::from(info)))
                        .collect()
                })
                .map_err(|e| e.to_i18n())
        }
    }
}

fn manual_download_url(item: &AssetItemData, version: &AssetVersion) -> Option<String> {
    if version.primary_file.is_some() {
        return None;
    }

    match version.host {
        ProjectHost::CurseForge => {
            if let Some(base) = item
                .website_url
                .as_deref()
                .map(str::trim)
                .filter(|u| !u.is_empty())
            {
                Some(format!(
                    "{}/files/{}",
                    base.trim_end_matches('/'),
                    version.id
                ))
            } else {
                Some(format!(
                    "https://www.curseforge.com/projects/{}",
                    version.project_id
                ))
            }
        }
        ProjectHost::Modrinth => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InstallerStep {
    #[default]
    Configure,
    SelectLoader,
    AssetConfig,
    SelectVersion,
    ModpackConfig,
    ManualDownloads,
}

impl InstallerStep {
    fn as_str(self) -> &'static str {
        match self {
            Self::Configure => "configure",
            Self::SelectLoader => "select_loader",
            Self::AssetConfig => "asset_config",
            Self::SelectVersion => "select_version",
            Self::ModpackConfig => "modpack_config",
            Self::ManualDownloads => "manual_downloads",
        }
    }
}

pub struct InstallerInit {
    pub mgr: Arc<SharedState<InstanceManager>>,
    pub config: Arc<SharedState<LauncherConfig>>,
    pub client: reqwest::Client,
    pub cache_dir: PathBuf,
    pub active_install: Arc<Mutex<Option<String>>>,
}

#[derive(Debug)]
pub enum InstallerInput {
    ShowToast(String),
    CancelClicked,
    BackTo(InstallerStep),
    OpenLoader(ModLoader),
    Create {
        name: String,
        loader: ModLoader,
        loader_version: Option<String>,
    },
    Refresh,
    SetLoader(ModLoader, Option<String>),
    OpenVanilla(VersionEntry),

    OpenAsset(Box<AssetItemData>),
    OpenVersionSelect {
        title: String,
        target_mc_version: Option<String>,
        versions: Vec<AssetVersion>,
        return_to: InstallerStep,
    },
    OpenLocalModpack {
        path: PathBuf,
        manifest: Box<ModpackManifest>,
    },
    VersionSelectBack,
    VersionSelectFinish(Box<AssetVersion>),

    OpenDependency(Box<ResolvedDep>),
    InstallAsset {
        item: Box<AssetItemData>,
        version: Box<AssetVersion>,
        target: InstallTarget,
    },

    InstallModpack {
        name: String,
        source: ModpackInstallSource,
    },

    OpenManualDownloads {
        instance_id: String,
        launch_after: bool,
    },
    LaunchBypass(String),
}

#[derive(Debug, Clone)]
pub enum InstallerOutput {
    Cancel,
    Created(Box<Instance>, VersionEntry),
    ShowToast(String),
    InstallAsset {
        item: Box<AssetItemData>,
        version: Box<AssetVersion>,
        target: InstallTarget,
    },

    InstallModpack {
        name: String,
        source: ModpackInstallSource,
    },
    LaunchBypass(String),
}

pub struct InstallerPage {
    mgr: Arc<SharedState<InstanceManager>>,
    step: InstallerStep,
    selected_version: Option<VersionEntry>,

    configure: Controller<ConfigurePage>,
    loader_select: Controller<LoaderSelectPage>,
    asset_config: Controller<AssetConfigPage>,
    modpack_config: Controller<ModpackPage>,
    version_select: Controller<VersionSelectPage>,
    version_select_return_to: InstallerStep,
    manual: Controller<ManualPage>,

    active_install: Arc<Mutex<Option<String>>>,
}

impl InstallerPage {
    fn validate_new_instance(&self, name: &str) -> Result<(), String> {
        // You have poisoning and I am poisoned
        if self
            .active_install
            .lock()
            .map_or(true, |active| active.is_some())
        {
            return Err(t!(
                "instances-create-error-active-install",
                "Another installation is already in progress"
            ));
        }

        if let Err(err) = InstanceConfig::validate_instance_id(name) {
            error!("Failed to create instance: {err}");
            return Err(t!(
                "instances-create-error-create-instance",
                "Failed to create instance: {err}",
                err = err
            ));
        }

        let mgr = self.mgr.read();
        let already_exists =
            mgr.versions_dir.join(name).exists() || mgr.instances.iter().any(|i| i.id() == name);
        if already_exists {
            return Err(CoreError::InstanceAlreadyExists {
                id: name.to_string(),
            }
            .to_i18n());
        }

        Ok(())
    }
}

#[relm4::component(pub)]
impl Component for InstallerPage {
    type Init = InstallerInit;
    type Input = InstallerInput;
    type Output = InstallerOutput;
    type CommandOutput = ();

    view! {
        #[root]
        gtk::Stack {
            set_transition_type: gtk::StackTransitionType::Crossfade,
            set_transition_duration: 150,

            add_named[Some(InstallerStep::Configure.as_str())] = model.configure.widget(),
            add_named[Some(InstallerStep::SelectLoader.as_str())] = model.loader_select.widget(),
            add_named[Some(InstallerStep::AssetConfig.as_str())] = model.asset_config.widget(),
            add_named[Some(InstallerStep::SelectVersion.as_str())] = model.version_select.widget(),
            add_named[Some(InstallerStep::ModpackConfig.as_str())] = model.modpack_config.widget(),
            add_named[Some(InstallerStep::ManualDownloads.as_str())] = model.manual.widget(),

            #[watch]
            set_visible_child_name: model.step.as_str(),
        }

    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let configure = ConfigurePage::builder()
            .launch(ConfigureInit {
                config: Arc::clone(&init.config),
            })
            .forward(sender.input_sender(), |msg| match msg {
                ConfigureOutput::Back => InstallerInput::CancelClicked,
                ConfigureOutput::Create {
                    name,
                    loader,
                    loader_version,
                } => InstallerInput::Create {
                    name,
                    loader,
                    loader_version,
                },
                ConfigureOutput::OpenLoader(loader) => InstallerInput::OpenLoader(loader),
            });

        let loader_select = LoaderSelectPage::builder()
            .launch(LoaderSelectInit {
                config: Arc::clone(&init.config),
                client: init.client.clone(),
                cache_dir: init.cache_dir.clone(),
            })
            .forward(sender.input_sender(), |msg| match msg {
                LoaderSelectOutput::Back => InstallerInput::BackTo(InstallerStep::Configure),
                LoaderSelectOutput::Selected { loader, version } => {
                    InstallerInput::SetLoader(loader, version)
                }
                LoaderSelectOutput::ShowToast(message) => InstallerInput::ShowToast(message),
            });

        let asset_config = AssetConfigPage::builder()
            .launch(AssetConfigInit {
                mgr: Arc::clone(&init.mgr),
                config: Arc::clone(&init.config),
                client: init.client.clone(),
                cache_dir: init.cache_dir.clone(),
            })
            .forward(sender.input_sender(), |msg| match msg {
                AssetConfigOutput::Back => InstallerInput::CancelClicked,
                AssetConfigOutput::Install {
                    item,
                    version,
                    target,
                } => InstallerInput::InstallAsset {
                    item,
                    version,
                    target,
                },
                AssetConfigOutput::OpenDependency(dep) => InstallerInput::OpenDependency(dep),
                AssetConfigOutput::OpenVersionSelect {
                    title,
                    target_mc_version,
                    versions,
                } => InstallerInput::OpenVersionSelect {
                    title,
                    target_mc_version,
                    versions,
                    return_to: InstallerStep::AssetConfig,
                },
                AssetConfigOutput::ShowToast(m) => InstallerInput::ShowToast(m),
            });

        let modpack_config = ModpackPage::builder()
            .launch(ModpackInit {
                config: Arc::clone(&init.config),
                client: init.client.clone(),
                cache_dir: init.cache_dir.clone(),
            })
            .forward(sender.input_sender(), |msg| match msg {
                ModpackOutput::Back => InstallerInput::CancelClicked,
                ModpackOutput::Install { name, source } => {
                    InstallerInput::InstallModpack { name, source }
                }
                ModpackOutput::OpenVersionSelect { title, versions } => {
                    InstallerInput::OpenVersionSelect {
                        title,
                        target_mc_version: None,
                        versions,
                        return_to: InstallerStep::ModpackConfig,
                    }
                }
                ModpackOutput::ShowToast(m) => InstallerInput::ShowToast(m),
            });

        let version_select = VersionSelectPage::builder()
            .launch(VersionSelectInit {
                config: Arc::clone(&init.config),
            })
            .forward(sender.input_sender(), |msg| match msg {
                VersionSelectOutput::Back => InstallerInput::VersionSelectBack,
                VersionSelectOutput::Selected(ver) => InstallerInput::VersionSelectFinish(ver),
            });

        let manual = ManualPage::builder()
            .launch(ManualInit {
                mgr: Arc::clone(&init.mgr),
                config: Arc::clone(&init.config),
            })
            .forward(sender.input_sender(), |msg| match msg {
                ManualOutput::Back => InstallerInput::CancelClicked,
                ManualOutput::LaunchBypass(id) => InstallerInput::LaunchBypass(id),
                ManualOutput::ShowToast(m) => InstallerInput::ShowToast(m),
            });

        init.config
            .subscribe(sender.input_sender(), |_| InstallerInput::Refresh);

        let model = Self {
            mgr: init.mgr,
            selected_version: None,
            step: InstallerStep::Configure,

            configure,
            loader_select,
            asset_config,
            modpack_config,
            version_select,
            version_select_return_to: InstallerStep::AssetConfig,
            manual,

            active_install: init.active_install,
        };

        let widgets = view_output!();

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match msg {
            InstallerInput::CancelClicked => {
                self.loader_select.emit(LoaderSelectInput::Abort);
                let _ = sender.output(InstallerOutput::Cancel);
            }

            // Create version metadata without downloading resource files, file downloads are handled upstream.
            InstallerInput::Create {
                name,
                loader,
                loader_version,
            } => {
                if let Err(msg) = self.validate_new_instance(&name) {
                    let _ = sender.output(InstallerOutput::ShowToast(msg));
                    return;
                }

                let Some(version_entry) = self.selected_version.clone() else {
                    let _ = sender.output(InstallerOutput::ShowToast(t!(
                        "instances-create-error-empty-version",
                        "Please select a Minecraft version"
                    )));
                    return;
                };

                let versions_dir = self.mgr.read().versions_dir.clone();
                // Only create in memory, do not save, wait until successful installation.
                let mut instance =
                    match Instance::new(&name, &name, &version_entry.id, &versions_dir) {
                        Ok(instance) => instance,
                        Err(err) => {
                            error!("Failed to create instance: {err}");
                            let msg = t!(
                                "instances-create-error-create-instance",
                                "Failed to create instance: {err}",
                                err = err
                            );
                            let _ = sender.output(InstallerOutput::ShowToast(msg));
                            return;
                        }
                    };

                if loader != ModLoader::Vanilla {
                    instance.config.mod_loader = loader;
                    instance.config.mod_loader_version = loader_version;
                }

                info!(
                    "Queued installation for instance '{}' with version '{}'",
                    name, version_entry.id
                );

                self.selected_version = None;
                self.step = InstallerStep::Configure;
                let _ = sender.output(InstallerOutput::Created(Box::new(instance), version_entry));
            }
            InstallerInput::ShowToast(message) => {
                let _ = sender.output(InstallerOutput::ShowToast(message));
            }

            InstallerInput::Refresh => {}
            InstallerInput::BackTo(step) => {
                self.step = step;
            }
            InstallerInput::OpenLoader(loader) => {
                if let Some(entry) = &self.selected_version {
                    self.loader_select.emit(LoaderSelectInput::Load {
                        loader,
                        game_version: entry.id.clone(),
                    });
                    self.step = InstallerStep::SelectLoader;
                }
            }
            InstallerInput::SetLoader(loader, version) => {
                self.configure
                    .emit(ConfigureInput::SetLoader(loader, version));
                self.step = InstallerStep::Configure;
            }
            InstallerInput::OpenVanilla(version) => {
                self.selected_version = Some(version.clone());
                self.configure
                    .emit(ConfigureInput::SetGameVersion(version.id));
                self.step = InstallerStep::Configure;
            }
            InstallerInput::InstallAsset {
                item,
                version,
                target,
            } => {
                let _ = sender.output(InstallerOutput::InstallAsset {
                    item,
                    version,
                    target,
                });
            }
            InstallerInput::OpenAsset(item) => {
                if item.project_type == ProjectType::Modpack {
                    self.modpack_config
                        .emit(ModpackInput::Open(Box::new(ModpackSource::Online(*item))));
                    self.step = InstallerStep::ModpackConfig;
                    return;
                }

                self.asset_config.emit(AssetConfigInput::Open(item));
                self.step = InstallerStep::AssetConfig;
            }
            InstallerInput::OpenDependency(dep) => {
                if let Some(item) = dep.item {
                    self.asset_config
                        .emit(AssetConfigInput::Open(Box::new(item)));
                } else {
                    let _ = sender.output(InstallerOutput::ShowToast(format!(
                        "Dependency project: {}",
                        dep.project_id
                    )));
                }
            }
            InstallerInput::OpenVersionSelect {
                title,
                target_mc_version,
                versions,
                return_to,
            } => {
                self.version_select_return_to = return_to;
                self.version_select.emit(VersionSelectInput::Load {
                    title,
                    target_mc_version,
                    versions,
                });
                self.step = InstallerStep::SelectVersion;
            }
            InstallerInput::VersionSelectFinish(ver) => match self.version_select_return_to {
                InstallerStep::ModpackConfig => {
                    self.modpack_config
                        .emit(ModpackInput::SetManualVersion(ver));
                    self.step = InstallerStep::ModpackConfig;
                }
                _ => {
                    self.asset_config
                        .emit(AssetConfigInput::SetManualVersion(ver));
                    self.step = InstallerStep::AssetConfig;
                }
            },
            InstallerInput::VersionSelectBack => match self.version_select_return_to {
                InstallerStep::ModpackConfig => {
                    self.step = InstallerStep::ModpackConfig;
                }
                _ => {
                    self.step = InstallerStep::AssetConfig;
                }
            },
            InstallerInput::InstallModpack { name, source } => {
                if let Err(msg) = self.validate_new_instance(&name) {
                    let _ = sender.output(InstallerOutput::ShowToast(msg));
                    return;
                }

                info!("Queued modpack installation for instance '{name}'");

                self.step = InstallerStep::Configure;
                let _ = sender.output(InstallerOutput::InstallModpack { name, source });
            }
            InstallerInput::OpenLocalModpack { path, manifest } => {
                self.modpack_config
                    .emit(ModpackInput::Open(Box::new(ModpackSource::Local {
                        path,
                        manifest: *manifest,
                    })));
                self.step = InstallerStep::ModpackConfig;
            }

            InstallerInput::OpenManualDownloads {
                instance_id,
                launch_after,
            } => {
                self.manual.emit(ManualInput::Open {
                    instance_id,
                    launch_after,
                });
                self.step = InstallerStep::ManualDownloads;
            }

            InstallerInput::LaunchBypass(id) => {
                let _ = sender.output(InstallerOutput::LaunchBypass(id));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acl_minecraft::{AssetFile, ReleaseType};

    fn sample_asset_version(host: ProjectHost, with_file: bool) -> AssetVersion {
        AssetVersion {
            host,
            id: "54321".to_string(),
            project_id: "12345".to_string(),
            name: "Sample 1.0.0".to_string(),
            version_number: "1.0.0".to_string(),
            release_type: ReleaseType::Release,
            game_versions: vec!["1.21.1".to_string()],
            loaders: vec!["fabric".to_string()],
            date_published: "2026-01-01T00:00:00Z".to_string(),
            downloads: 42,
            primary_file: with_file.then(|| AssetFile {
                url: "https://example.com/sample.jar".to_string(),
                filename: "sample.jar".to_string(),
                size: 1024,
                sha1: None,
                sha512: None,
            }),
            dependencies: Vec::new(),
        }
    }

    #[test]
    fn test_installer_step_default_and_as_str() {
        assert_eq!(InstallerStep::default(), InstallerStep::Configure);
        assert_eq!(InstallerStep::Configure.as_str(), "configure");
        assert_eq!(InstallerStep::SelectLoader.as_str(), "select_loader");
        assert_eq!(InstallerStep::AssetConfig.as_str(), "asset_config");
        assert_eq!(InstallerStep::SelectVersion.as_str(), "select_version");
        assert_eq!(InstallerStep::ModpackConfig.as_str(), "modpack_config");
    }

    fn sample_asset_item(host: ProjectHost, website_url: Option<&str>) -> AssetItemData {
        AssetItemData {
            host,
            project_type: ProjectType::Mod,
            project_id: "12345".to_string(),
            title: "Sample Mod".to_string(),
            author: "Tester".to_string(),
            description: "Desc".to_string(),
            icon_url: None,
            downloads: 100,
            display_categories: Vec::new(),
            latest_version: None,
            website_url: website_url.map(str::to_string),
        }
    }

    #[test]
    fn test_manual_download_url() {
        let cf_manual = sample_asset_version(ProjectHost::CurseForge, false);
        let cf_item_with_url = sample_asset_item(
            ProjectHost::CurseForge,
            Some(
                "  https://www.curseforge.com/minecraft/mc-mods/mirabilis-vanilla-biome-overhaul/  ",
            ),
        );
        assert_eq!(
            manual_download_url(&cf_item_with_url, &cf_manual).as_deref(),
            Some(
                "https://www.curseforge.com/minecraft/mc-mods/mirabilis-vanilla-biome-overhaul/files/54321"
            )
        );

        let cf_item_no_url = sample_asset_item(ProjectHost::CurseForge, None);
        assert_eq!(
            manual_download_url(&cf_item_no_url, &cf_manual).as_deref(),
            Some("https://www.curseforge.com/projects/12345")
        );

        let cf_item_blank_url = sample_asset_item(ProjectHost::CurseForge, Some("   "));
        assert_eq!(
            manual_download_url(&cf_item_blank_url, &cf_manual).as_deref(),
            Some("https://www.curseforge.com/projects/12345")
        );

        let cf_auto = sample_asset_version(ProjectHost::CurseForge, true);
        assert_eq!(manual_download_url(&cf_item_with_url, &cf_auto), None);

        let mr_item = sample_asset_item(
            ProjectHost::Modrinth,
            Some("https://modrinth.com/mod/sample"),
        );
        let mr_no_file = sample_asset_version(ProjectHost::Modrinth, false);
        assert_eq!(manual_download_url(&mr_item, &mr_no_file), None);

        let mr_with_file = sample_asset_version(ProjectHost::Modrinth, true);
        assert_eq!(manual_download_url(&mr_item, &mr_with_file), None);
    }

    #[test]
    fn test_fetch_helpers_hermetic_paths() {
        let rt = relm4::tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("failed to build current-thread tokio runtime");
        let client = reqwest::Client::new();
        let cache_dir = Path::new("/tmp/acl-unused-cache");

        rt.block_on(async {
            // Invalid CurseForge project IDs fail during ID parsing before any I/O.
            for invalid_id in ["", "   ", "not-a-number", "-1"] {
                let err = fetch_asset_versions(
                    &client,
                    ProjectHost::CurseForge,
                    invalid_id,
                    cache_dir,
                    DownloadSource::Official,
                    false,
                )
                .await
                .expect_err("invalid CurseForge project ID should fail");
                assert!(!err.is_empty());
            }

            // Empty ID batches return an empty map without network or API key access.
            for host in [ProjectHost::Modrinth, ProjectHost::CurseForge] {
                let details = fetch_project_details(
                    &client,
                    host,
                    &[],
                    cache_dir,
                    DownloadSource::Official,
                    false,
                )
                .await
                .expect("empty project ID batch should succeed");
                assert!(details.is_empty());
            }

            // Invalid CurseForge ID inside a batch fails before any network call.
            let batch_ids = vec!["12345".to_string(), "bad-id".to_string()];
            let err = fetch_project_details(
                &client,
                ProjectHost::CurseForge,
                &batch_ids,
                cache_dir,
                DownloadSource::Official,
                false,
            )
            .await
            .expect_err("batch with invalid CurseForge ID should fail");
            assert!(!err.is_empty());
        });
    }
}
