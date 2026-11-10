// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Arc;

use libadwaita::prelude::*;

use acl_core::{InstanceConfig, LauncherConfig, ModLoader};
use relm4::SimpleComponent;
use relm4::{ComponentParts, ComponentSender, SharedState, adw};

use crate::t;

#[derive(Debug, Clone)]
pub enum ConfigureInput {
    SetGameVersion(String),
    SetLoader(ModLoader, Option<String>),
    NameChanged(String),
    BackClicked,
    CreateClicked,
    Refresh,
}

#[derive(Debug, Clone)]
pub enum ConfigureOutput {
    Back,
    OpenLoader(ModLoader),
    Create {
        name: String,
        loader: ModLoader,
        loader_version: Option<String>,
    },
}

#[derive(Debug, Clone)]

pub struct ConfigureInit {
    pub config: Arc<SharedState<LauncherConfig>>,
}

pub struct ConfigurePage {
    game_version: String,
    instance_name: String,
    selected_loader: ModLoader,
    selected_loader_version: Option<String>,
}

impl ConfigurePage {
    fn effective_name(&self) -> String {
        let trimmed = self.instance_name.trim();
        if !trimmed.is_empty() {
            trimmed.to_string()
        } else {
            if self.selected_loader != ModLoader::Vanilla {
                format!(
                    "{}-{}-{}",
                    self.game_version,
                    self.selected_loader.label(),
                    self.selected_loader_version
                        .clone()
                        .unwrap_or("unknown".to_string())
                )
            } else {
                self.game_version.to_string()
            }
        }
    }

    fn can_create(&self) -> bool {
        !self.game_version.is_empty()
            && InstanceConfig::validate_instance_id(&self.effective_name()).is_ok()
    }

    fn loader_subtitle(&self, loader: ModLoader) -> String {
        if self.selected_loader == loader {
            self.selected_loader_version
                .clone()
                .unwrap_or_else(|| t!("instances-create-none", "None"))
        } else {
            if self.is_loader_compatible(loader) {
                t!("instances-create-none", "None")
            } else {
                t!(
                    "instances-create-loader-incompatible",
                    "{candidate} is incompatible with {selected}",
                    candidate = loader.label(),
                    selected = &self.selected_loader.label()
                )
            }
        }
    }

    fn is_loader_compatible(&self, loader: ModLoader) -> bool {
        (self.selected_loader == loader) || (self.selected_loader == ModLoader::Vanilla)
    }
}

#[relm4::component(pub)]
impl SimpleComponent for ConfigurePage {
    type Init = ConfigureInit;
    type Input = ConfigureInput;
    type Output = ConfigureOutput;

    view! {

        #[root]
        gtk::Box {
            set_orientation: gtk::Orientation::Vertical,

            adw::Clamp {
                set_maximum_size: 720,
                set_margin_top: 24,
                set_margin_start: 12,
                set_margin_end: 12,
                gtk::Box {
                    set_orientation: gtk::Orientation::Vertical,
                    set_spacing: 12,

                    // Top bar
                    gtk::Box {
                        set_orientation: gtk::Orientation::Vertical,
                        set_spacing: 12,

                        gtk::Box {
                            set_orientation: gtk::Orientation::Horizontal,

                            gtk::Button {
                                connect_clicked => ConfigureInput::BackClicked,
                                adw::ButtonContent {
                                    set_icon_name: "go-previous-symbolic",
                                    #[watch]
                                    set_label: &t!("instances-create-button-back", "Back"),
                                },
                            },

                            gtk::Button {
                                set_hexpand: true,
                                set_halign: gtk::Align::End,
                                add_css_class: "suggested-action",
                                #[watch]
                                set_sensitive: model.can_create(),
                                connect_clicked => ConfigureInput::CreateClicked,
                                adw::ButtonContent {
                                    set_icon_name: "list-add-symbolic",
                                    #[watch]
                                    set_label: &t!("instances-create-button-create", "Create"),
                                },
                            },
                        },

                    },

                    // Base settings group
                    adw::PreferencesGroup {
                        #[watch]
                        set_title: &t!("instances-create-settings-title", "Instance Settings"),

                        adw::EntryRow {
                            add_css_class: "monospace",
                            #[watch]
                            set_title: &t!("instances-create-settings-instance-name", "Instance Name"),
                            connect_changed[sender] => move |entry| {
                                sender.input(ConfigureInput::NameChanged(entry.text().to_string()));
                            },
                        },

                        adw::ActionRow {
                            #[watch]
                            set_title: &t!("instances-create-settings-game-version", "Game Version"),
                            #[watch]
                            set_subtitle: &model.game_version,
                        },

                    },

                    // Mod Loader settings group
                    adw::PreferencesGroup {
                        #[watch]
                        set_title: &t!("instances-create-settings-loader-title","Mod Loader"),

                        adw::ActionRow {
                            // No translation needed
                            set_title: "Fabric",

                            #[watch]
                            set_subtitle: &model.loader_subtitle(ModLoader::Fabric),
                            #[watch]
                            set_activatable: model.is_loader_compatible(ModLoader::Fabric),

                            connect_activated[sender] => move |_| {
                                let _ = sender.output(ConfigureOutput::OpenLoader(ModLoader::Fabric));
                            },
                            add_suffix = &gtk::Image::from_icon_name("go-next-symbolic"),

                        }
                    }
                },
            }
        }

    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: relm4::prelude::ComponentSender<Self>,
    ) -> relm4::prelude::ComponentParts<Self> {
        init.config
            .subscribe(sender.input_sender(), |_| ConfigureInput::Refresh);

        let model = ConfigurePage {
            game_version: String::new(),
            instance_name: String::new(),
            selected_loader: ModLoader::Vanilla,
            selected_loader_version: None,
        };

        let widgets = view_output!();

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            ConfigureInput::BackClicked => {
                let _ = sender.output(ConfigureOutput::Back);
            }
            ConfigureInput::CreateClicked => {
                let name = self.effective_name().to_owned();
                let _ = sender.output(ConfigureOutput::Create {
                    name,
                    loader: self.selected_loader,
                    loader_version: self.selected_loader_version.clone(),
                });
            }
            ConfigureInput::NameChanged(name) => {
                self.instance_name = name;
            }
            ConfigureInput::Refresh => {}
            ConfigureInput::SetGameVersion(version) => {
                self.game_version = version;
                self.instance_name = String::new();
                self.selected_loader = ModLoader::Vanilla;
                self.selected_loader_version = None;
            }
            ConfigureInput::SetLoader(loader, version) => {
                self.selected_loader = loader;
                self.selected_loader_version = version;
            }
        }
    }
}
