// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

pub mod configure;
pub mod loader_select;
pub mod version_select;

use std::{path::PathBuf, sync::Arc};

use acl_core::{Instance, LauncherConfig, ModLoader, manager::InstanceManager};
use acl_minecraft::VersionEntry;

use relm4::{
    Component, ComponentController, ComponentParts, ComponentSender, Controller, SharedState,
};
use tracing::{error, info};

use crate::{
    pages::instances::create::{
        configure::{ConfigureInit, ConfigureInput, ConfigureOutput, ConfigurePage},
        loader_select::{
            LoaderSelectInit, LoaderSelectInput, LoaderSelectOutput, LoaderSelectPage,
        },
        version_select::{VersionSelectInit, VersionSelectOutput, VersionSelectPage},
    },
    t,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CreateStep {
    #[default]
    SelectVersion,
    Configure,
    SelectLoader,
}

impl CreateStep {
    fn as_str(self) -> &'static str {
        match self {
            Self::SelectVersion => "select_version",
            Self::Configure => "configure",
            Self::SelectLoader => "select_loader",
        }
    }
}

pub struct CreatePageInit {
    pub mgr: Arc<SharedState<InstanceManager>>,
    pub config: Arc<SharedState<LauncherConfig>>,
    pub client: reqwest::Client,
    pub cache_dir: PathBuf,
}

#[derive(Debug)]
pub enum CreateInput {
    VersionSelected(VersionEntry),
    ShowToast(String),
    CancelClicked,
    BackTo(CreateStep),
    OpenLoader(ModLoader),
    Create {
        name: String,
        loader: ModLoader,
        loader_version: Option<String>,
    },
    Refresh,
    SetLoader(ModLoader, Option<String>),
}

#[derive(Debug, Clone)]
pub enum CreateOutput {
    Cancel,
    Created(String, VersionEntry),
    ShowToast(String),
}

pub struct CreatePage {
    mgr: Arc<SharedState<InstanceManager>>,
    step: CreateStep,
    selected_version: Option<VersionEntry>,
    version_select: Controller<VersionSelectPage>,
    configure: Controller<ConfigurePage>,
    loader_select: Controller<LoaderSelectPage>,
}

#[relm4::component(pub)]
impl Component for CreatePage {
    type Init = CreatePageInit;
    type Input = CreateInput;
    type Output = CreateOutput;
    type CommandOutput = ();

    view! {
        #[root]
        gtk::Stack {
            set_transition_type: gtk::StackTransitionType::SlideLeftRight,

            add_named[Some(CreateStep::SelectVersion.as_str())] = model.version_select.widget(),
            add_named[Some(CreateStep::Configure.as_str())] = model.configure.widget(),
            add_named[Some(CreateStep::SelectLoader.as_str())] = model.loader_select.widget(),

            #[watch]
            set_visible_child_name: model.step.as_str(),
        }

    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let version_select = VersionSelectPage::builder()
            .launch(VersionSelectInit {
                config: Arc::clone(&init.config),
                client: init.client.clone(),
                cache_dir: init.cache_dir.clone(),
            })
            .forward(sender.input_sender(), |msg| match msg {
                VersionSelectOutput::Cancel => CreateInput::CancelClicked,
                VersionSelectOutput::Selected(entry) => CreateInput::VersionSelected(entry),
                VersionSelectOutput::ShowToast(text) => CreateInput::ShowToast(text),
            });

        let configure = ConfigurePage::builder()
            .launch(ConfigureInit {
                config: Arc::clone(&init.config),
            })
            .forward(sender.input_sender(), |msg| match msg {
                ConfigureOutput::Back => CreateInput::BackTo(CreateStep::SelectVersion),
                ConfigureOutput::Create {
                    name,
                    loader,
                    loader_version,
                } => CreateInput::Create {
                    name,
                    loader,
                    loader_version,
                },
                ConfigureOutput::OpenLoader(loader) => CreateInput::OpenLoader(loader),
            });

        let loader_select = LoaderSelectPage::builder()
            .launch(LoaderSelectInit {
                config: Arc::clone(&init.config),
                client: init.client.clone(),
                cache_dir: init.cache_dir.clone(),
            })
            .forward(sender.input_sender(), |msg| match msg {
                LoaderSelectOutput::Back => CreateInput::BackTo(CreateStep::Configure),
                LoaderSelectOutput::Selected { loader, version } => {
                    CreateInput::SetLoader(loader, version)
                }
                LoaderSelectOutput::ShowToast(message) => CreateInput::ShowToast(message),
            });

        init.config
            .subscribe(sender.input_sender(), |_| CreateInput::Refresh);

        let model = Self {
            mgr: init.mgr,
            selected_version: None,
            step: CreateStep::SelectVersion,

            version_select,
            configure,
            loader_select,
        };

        let widgets = view_output!();

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match msg {
            CreateInput::CancelClicked => {
                let _ = sender.output(CreateOutput::Cancel);
            }

            // Create version metadata without downloading resource files, file downloads are handled upstream.
            CreateInput::Create {
                name,
                loader,
                loader_version,
            } => {
                let Some(version_entry) = self.selected_version.clone() else {
                    let _ = sender.output(CreateOutput::ShowToast(t!(
                        "instances-create-error-empty-version",
                        "Please select a Minecraft version"
                    )));
                    return;
                };

                let versions_dir = self.mgr.read().versions_dir.clone();
                match Instance::create(&name, &name, &version_entry.id, &versions_dir) {
                    Ok(mut instance) => {
                        if loader != ModLoader::Vanilla {
                            instance.config.mod_loader = loader;
                            instance.config.mod_loader_version = loader_version;
                            if let Err(err) = instance.save_config() {
                                error!("Failed to save instance config: {err}");
                                let _ = sender.output(CreateOutput::ShowToast(t!(
                                    "instances-create-error-save-config",
                                    "Failed to save instance config: {err}",
                                    err = err
                                )));
                            }
                        }
                        info!(
                            "Created instance '{}' with version '{}'",
                            name, version_entry.id
                        );
                        if let Err(err) = self.mgr.write().refresh() {
                            error!("Failed to refresh instances: {err}");
                            let msg = t!(
                                "instances-create-error-refresh-instances",
                                "Failed to refresh instances: {err}",
                                err = err
                            );
                            let _ = sender.output(CreateOutput::ShowToast(msg));
                        } else {
                            self.selected_version = None;
                            self.step = CreateStep::SelectVersion;
                            let _ = sender.output(CreateOutput::Created(name, version_entry));
                        }
                    }
                    Err(err) => {
                        error!("Failed to create instance: {err}");
                        let msg = t!(
                            "instances-create-error-create-instance",
                            "Failed to create instance: {err}",
                            err = err
                        );
                        let _ = sender.output(CreateOutput::ShowToast(msg));
                    }
                }
            }
            CreateInput::ShowToast(message) => {
                let _ = sender.output(CreateOutput::ShowToast(message));
            }
            CreateInput::VersionSelected(entry) => {
                self.configure
                    .emit(ConfigureInput::SetGameVersion(entry.id.clone()));
                self.selected_version = Some(entry);
                self.step = CreateStep::Configure;
            }
            CreateInput::Refresh => {}
            CreateInput::BackTo(step) => {
                self.step = step;
            }
            CreateInput::OpenLoader(loader) => {
                if let Some(entry) = &self.selected_version {
                    self.loader_select.emit(LoaderSelectInput::Load {
                        loader,
                        game_version: entry.id.clone(),
                    });
                    self.step = CreateStep::SelectLoader;
                }
            }
            CreateInput::SetLoader(loader, version) => {
                self.configure
                    .emit(ConfigureInput::SetLoader(loader, version));
                self.step = CreateStep::Configure;
            }
        }
    }
}
