// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

pub mod asset_config;
pub mod configure;
pub mod loader_select;
pub mod version_select;

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use acl_core::{CoreError, Instance, LauncherConfig, ModLoader, manager::InstanceManager};
use acl_minecraft::{AssetItemData, AssetVersion, ProjectType, VersionEntry};

use relm4::{
    Component, ComponentController, ComponentParts, ComponentSender, Controller, SharedState,
};
use tracing::{error, info};

use crate::{
    i18n::LocalizeError,
    pages::installer::{
        asset_config::{
            AssetConfigInit, AssetConfigInput, AssetConfigOutput, AssetConfigPage, InstallTarget,
            ResolvedDep,
        },
        configure::{ConfigureInit, ConfigureInput, ConfigureOutput, ConfigurePage},
        loader_select::{
            LoaderSelectInit, LoaderSelectInput, LoaderSelectOutput, LoaderSelectPage,
        },
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
}

impl InstallerStep {
    fn as_str(self) -> &'static str {
        match self {
            Self::Configure => "configure",
            Self::SelectLoader => "select_loader",
            Self::AssetConfig => "asset_config",
            Self::SelectVersion => "select_version",
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
    },

    SetAssetVersion(Box<AssetVersion>),
    OpenDependency(Box<ResolvedDep>),
    InstallAsset {
        item: Box<AssetItemData>,
        version: Box<AssetVersion>,
        target: InstallTarget,
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
}

pub struct InstallerPage {
    mgr: Arc<SharedState<InstanceManager>>,
    step: InstallerStep,
    selected_version: Option<VersionEntry>,

    configure: Controller<ConfigurePage>,
    loader_select: Controller<LoaderSelectPage>,
    asset_config: Controller<AssetConfigPage>,
    version_select: Controller<VersionSelectPage>,
    active_install: Arc<Mutex<Option<String>>>,
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
                },
                AssetConfigOutput::ShowToast(m) => InstallerInput::ShowToast(m),
            });

        let version_select = VersionSelectPage::builder()
            .launch(VersionSelectInit {
                config: Arc::clone(&init.config),
            })
            .forward(sender.input_sender(), |msg| match msg {
                VersionSelectOutput::Back => InstallerInput::BackTo(InstallerStep::AssetConfig),
                VersionSelectOutput::Selected(ver) => InstallerInput::SetAssetVersion(ver),
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
            version_select,
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
                // You have poisoning and I am poisoned
                if self
                    .active_install
                    .lock()
                    .map_or(true, |active| active.is_some())
                {
                    let _ = sender.output(InstallerOutput::ShowToast(t!(
                        "instances-create-error-active-install",
                        "Another installation is already in progress"
                    )));
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

                let already_exists = instance.dir.exists()
                    || self.mgr.read().instances.iter().any(|i| i.id() == name);
                if already_exists {
                    let msg = CoreError::InstanceAlreadyExists { id: name }.to_i18n();
                    let _ = sender.output(InstallerOutput::ShowToast(msg));
                    return;
                }

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
                    let _ = sender.output(InstallerOutput::ShowToast(
                        "Modpack installer is coming soon".to_string(),
                    ));
                    let _ = sender.output(InstallerOutput::Cancel);
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
            } => {
                self.version_select.emit(VersionSelectInput::Load {
                    title,
                    target_mc_version,
                    versions,
                });
                self.step = InstallerStep::SelectVersion;
            }
            InstallerInput::SetAssetVersion(ver) => {
                self.asset_config
                    .emit(AssetConfigInput::SetManualVersion(ver));
                self.step = InstallerStep::AssetConfig;
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
    }
}
