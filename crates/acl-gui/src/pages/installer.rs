// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

pub mod asset_config;
pub mod configure;
pub mod loader_select;
pub mod modpack_config;
pub mod version_select;

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use acl_core::{
    CoreError, Instance, InstanceConfig, LauncherConfig, ModLoader, manager::InstanceManager,
};
use acl_minecraft::{AssetItemData, AssetVersion, ModpackManifest, ProjectType, VersionEntry};

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
        modpack_config::{ModpackInit, ModpackInput, ModpackOutput, ModpackPage, ModpackSource},
        version_select::{
            VersionSelectInit, VersionSelectInput, VersionSelectOutput, VersionSelectPage,
        },
    },
    t,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InstallerStep {
    #[default]
    Configure,
    SelectLoader,
    AssetConfig,
    SelectVersion,
    ModpackConfig,
}

impl InstallerStep {
    fn as_str(self) -> &'static str {
        match self {
            Self::Configure => "configure",
            Self::SelectLoader => "select_loader",
            Self::AssetConfig => "asset_config",
            Self::SelectVersion => "select_version",
            Self::ModpackConfig => "modpack_config",
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_installer_step_default_and_as_str() {
        assert_eq!(InstallerStep::default(), InstallerStep::Configure);
        assert_eq!(InstallerStep::Configure.as_str(), "configure");
        assert_eq!(InstallerStep::SelectLoader.as_str(), "select_loader");
        assert_eq!(InstallerStep::AssetConfig.as_str(), "asset_config");
        assert_eq!(InstallerStep::SelectVersion.as_str(), "select_version");
    }
}
