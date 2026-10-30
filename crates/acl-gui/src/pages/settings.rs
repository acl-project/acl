use std::path::PathBuf;
use std::sync::Arc;

use acl_core::LauncherConfig;
use libadwaita::prelude::*;
use relm4::{ComponentParts, SimpleComponent};
use relm4::{SharedState, prelude::*};
use tracing::error;

use crate::system::memory_info;
pub struct SettingsPage {
    config: Arc<SharedState<LauncherConfig>>,
    config_path: PathBuf,
    java_model: gtk::StringList,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsMsg {
    ShowToast(String),
}

#[derive(Debug)]
pub enum SettingsInput {
    SetMinMemory(u32),
    SetMaxMemory(u32),
    SetMaxConcurrentDownloads(usize),
    SetCloseOnLaunch(bool),
    SelectDefaultJava(usize),
    ScanJava,
    SaveConfig, // Trigger a save when the mouse is released or focus is lost.
}

impl SettingsPage {
    fn save_config(&self, sender: &ComponentSender<Self>) {
        if let Err(err) = self.config.read().save(&self.config_path) {
            let msg = format!("Failed to save configuration: {err}");
            error!("{err}");
            let _ = sender.output(SettingsMsg::ShowToast(msg));
        }
    }

    fn refresh_java_model(&mut self) {
        let cfg = self.config.read();
        let java_names: Vec<String> = cfg
            .java_list
            .iter()
            .map(|j| {
                let vendor = j.info.vendor.as_deref().unwrap_or("Java");
                format!("{} {}", vendor, j.info.version)
            })
            .collect();

        let name_refs: Vec<&str> = java_names.iter().map(|s| s.as_str()).collect();

        self.java_model
            .splice(0, self.java_model.n_items(), &name_refs);
    }

    /// Calculate the index of the default-selected Java.
    fn default_java_index(&self) -> u32 {
        let cfg = self.config.read();
        cfg.default_java
            .as_ref()
            .and_then(|def| cfg.java_list.iter().position(|j| j.path == def.path))
            .unwrap_or(gtk::INVALID_LIST_POSITION as usize) as u32
    }
}

#[relm4::component(pub)]
impl SimpleComponent for SettingsPage {
    type Init = (Arc<SharedState<LauncherConfig>>, PathBuf);
    type Output = SettingsMsg;
    type Input = SettingsInput;

    view! {
        adw::PreferencesPage {
            adw::PreferencesGroup {
                set_title: "Java",

                #[wrap(Some)]
                set_header_suffix = &gtk::Button {
                    set_icon_name: "view-refresh-symbolic",
                    add_css_class: "flat",
                    set_valign: gtk::Align::Center,
                    set_tooltip_text: Some("Rescan for installed Java runtimes"),

                    connect_clicked[sender] => move |_| {
                        sender.input(SettingsInput::ScanJava);
                    },
                },

                add = &adw::ActionRow {
                    set_title: "Max Memory",
                    #[watch]
                    set_subtitle: &format!("{} MB", model.config.read().max_memory_mb),
                    add_suffix = &gtk::Scale {
                        set_width_request: 240,
                        set_hexpand: true,
                        set_draw_value: false,
                        set_digits: 0,
                        set_adjustment: &gtk::Adjustment::new(
                            model.config.read().max_memory_mb as f64,
                            model.config.read().min_memory_mb as f64,
                            ((memory_info().total / (1024 * 1024)) as f64).max(model.config.read().min_memory_mb as f64),
                            256.0,
                            512.0,
                            0.0,
                        ),

                        connect_value_changed[sender] => move |scale| {
                            sender.input(SettingsInput::SetMaxMemory(scale.value() as u32));
                        },

                        add_controller = gtk::GestureClick {
                            connect_released[sender] => move |_, _, _, _| {
                                sender.input(SettingsInput::SaveConfig);
                            },
                        },
                    },

                },
                add = &adw::ComboRow {
                    set_title: "Java Runtime",
                   #[watch]
                    set_subtitle: &model.config.read().default_java.as_ref().map_or_else(
                        || "No Java runtime selected".to_string(),
                        |j| j.path.display().to_string(),
                    ),
                    set_model: Some(&java_model),
                    #[watch]
                    set_selected: model.default_java_index(),
                    connect_selected_notify[sender] => move |row| {
                        let idx = row.selected();
                        if idx != gtk::INVALID_LIST_POSITION {
                            sender.input(SettingsInput::SelectDefaultJava(idx as usize));
                            sender.input(SettingsInput::SaveConfig);
                        }
                    }
                }
            },

            adw::PreferencesGroup {
                set_title: "Downloads",
                add = &adw::SpinRow {
                    set_title: "Max Concurrent Downloads",
                    set_subtitle: "How many files can be downloaded at the same time",
                    set_digits: 0,

                    set_adjustment: Some(&gtk::Adjustment::new(
                        model.config.read().max_concurrent_downloads as f64,
                        1.0,
                        32.0,
                        1.0,
                        8.0,
                        0.0
                    )),

                    connect_value_notify[sender] => move |row| {
                        sender.input(SettingsInput::SetMaxConcurrentDownloads(row.value() as usize));
                        sender.input(SettingsInput::SaveConfig);
                    },

                }
            },

            adw::PreferencesGroup {
                set_title: "Launcher",
                add = &adw::SwitchRow {
                    set_title: "Close Launcher On Launch",
                    set_active: model.config.read().close_launcher_on_launch,

                    connect_active_notify[sender] => move |checkbox| {
                        sender.input(SettingsInput::SetCloseOnLaunch(checkbox.is_active()));
                        sender.input(SettingsInput::SaveConfig);
                    }
                }
            }

        }
    }

    fn init(
        (config, config_path): Self::Init,
        root: Self::Root,
        sender: relm4::prelude::ComponentSender<Self>,
    ) -> relm4::prelude::ComponentParts<Self> {
        let java_model = gtk::StringList::new(&[]);
        let mut model = SettingsPage {
            config,
            config_path,
            java_model: java_model.clone(),
        };

        model.refresh_java_model();

        let widgets = view_output!();

        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
        match message {
            SettingsInput::SetMinMemory(val) => {
                self.config.write().min_memory_mb = val;
            }
            SettingsInput::SetMaxMemory(val) => {
                self.config.write().max_memory_mb = val;
            }
            SettingsInput::SetMaxConcurrentDownloads(val) => {
                self.config.write().max_concurrent_downloads = val;
            }
            SettingsInput::SetCloseOnLaunch(val) => {
                self.config.write().close_launcher_on_launch = val;
            }
            SettingsInput::SelectDefaultJava(index) => {
                let mut cfg = self.config.write();
                if let Some(java) = cfg.java_list.get(index).cloned() {
                    cfg.default_java = Some(java);
                }
                drop(cfg);
            }
            SettingsInput::ScanJava => {
                self.config.write().search_java();
                self.refresh_java_model();
                let _ = sender.output(SettingsMsg::ShowToast(
                    "Successfully refreshed the Java list.".to_owned(),
                ));
            }
            SettingsInput::SaveConfig => {
                self.save_config(&sender);
            }
        }
    }
}
