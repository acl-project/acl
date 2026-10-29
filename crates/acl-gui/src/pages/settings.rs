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
            error!(msg);
            let _ = sender.output(SettingsMsg::ShowToast(msg));
        }
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
                            (memory_info().total / (1024 * 1024)) as f64,
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

                }
            },

            adw::PreferencesGroup {
                set_title: "Download",
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

                }
            }

        }
    }

    fn init(
        (config, config_path): Self::Init,
        root: Self::Root,
        sender: relm4::prelude::ComponentSender<Self>,
    ) -> relm4::prelude::ComponentParts<Self> {
        let model = SettingsPage {
            config,
            config_path,
        };
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
            }
            SettingsInput::SaveConfig => {
                self.save_config(&sender);
            }
        }
    }
}
