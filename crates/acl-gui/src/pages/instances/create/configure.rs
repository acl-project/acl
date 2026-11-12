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
    name_entry: adw::EntryRow,
}

impl ConfigurePage {
    fn effective_name(&self) -> String {
        let trimmed = self.instance_name.trim();
        if !trimmed.is_empty() {
            trimmed.to_string()
        } else {
            if self.selected_loader != ModLoader::Vanilla {
                format!("{}-{}", self.game_version, self.selected_loader.label(),)
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

                        #[local_ref]
                        name_entry -> adw::EntryRow {
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

                        },

                        adw::ActionRow {
                            // No translation needed
                            set_title: "Forge",

                            #[watch]
                            set_subtitle: &model.loader_subtitle(ModLoader::Forge),
                            #[watch]
                            set_activatable: model.is_loader_compatible(ModLoader::Forge),

                            connect_activated[sender] => move |_| {
                                let _ = sender.output(ConfigureOutput::OpenLoader(ModLoader::Forge));
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
            name_entry: adw::EntryRow::new(),
        };

        let name_entry = model.name_entry.clone();

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
                self.name_entry.set_text("");
            }
            ConfigureInput::SetLoader(loader, version) => {
                self.selected_loader = loader;
                self.selected_loader_version = version;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_configure_page_logic() {
        if gtk::init().is_err() {
            return;
        }

        let mut page = ConfigurePage {
            game_version: String::new(),
            instance_name: String::new(),
            selected_loader: ModLoader::Vanilla,
            selected_loader_version: None,
            name_entry: adw::EntryRow::new(),
        };

        // 1. effective_name tests
        page.game_version = "1.21.1".to_string();

        page.instance_name = "  My Custom Instance  ".to_string();
        assert_eq!(page.effective_name(), "My Custom Instance");

        page.instance_name = "   ".to_string();
        page.selected_loader = ModLoader::Vanilla;
        assert_eq!(page.effective_name(), "1.21.1");

        page.selected_loader = ModLoader::Fabric;
        assert_eq!(page.effective_name(), "1.21.1-Fabric");

        // 2. can_create tests
        page.game_version = "".to_string();
        page.instance_name = "valid-name".to_string();
        assert!(!page.can_create());

        page.game_version = "1.21.1".to_string();
        assert!(page.can_create());

        page.instance_name = "invalid/name".to_string();
        assert!(!page.can_create());

        page.instance_name = "..".to_string();
        assert!(!page.can_create());

        // 3. loader compatibility and subtitle tests
        page.selected_loader = ModLoader::Vanilla;
        assert!(page.is_loader_compatible(ModLoader::Fabric));
        assert!(page.is_loader_compatible(ModLoader::Vanilla));
        assert_eq!(page.loader_subtitle(ModLoader::Fabric), "None");

        page.selected_loader = ModLoader::Fabric;
        page.selected_loader_version = Some("0.16.10".to_string());
        assert!(page.is_loader_compatible(ModLoader::Fabric));
        assert_eq!(page.loader_subtitle(ModLoader::Fabric), "0.16.10");

        assert!(!page.is_loader_compatible(ModLoader::Forge));
        assert!(page.loader_subtitle(ModLoader::Forge).contains("Forge"));
        assert!(page.loader_subtitle(ModLoader::Forge).contains("Fabric"));
    }
}
