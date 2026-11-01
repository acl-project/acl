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
    /// Set when max memory changes; cleared after it has been persisted.
    max_memory_dirty: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsMsg {
    ShowToast(String),
}

#[derive(Debug)]
pub enum SettingsInput {
    SetMinMemory(u32),
    SetMaxMemory(u32),
    /// Persist max memory if it changed (sent on pointer/touch/key release or scroll).
    CommitMaxMemory,
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

        let mut java_names: Vec<String> = Vec::with_capacity(cfg.java_list.len() + 1);
        java_names.push("Auto-detect (Recommended)".to_string());
        java_names.extend(cfg.java_list.iter().map(|j| {
            let vendor = j.info.vendor.as_deref().unwrap_or("Java");
            format!("{} {}", vendor, j.info.version)
        }));

        let name_refs: Vec<&str> = java_names.iter().map(|s| s.as_str()).collect();

        self.java_model
            .splice(0, self.java_model.n_items(), &name_refs);
    }

    /// Calculate the index of the default-selected Java.
    fn default_java_index(&self) -> u32 {
        let cfg = self.config.read();

        match &cfg.default_java {
            None => 0,
            Some(def) => cfg
                .java_list
                .iter()
                .position(|j| j.path == def.path)
                .map(|pos| (pos + 1) as u32)
                .unwrap_or(0),
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

                        // Not a gesture, so it is never cancelled by GtkRange's internal gestures.
                        add_controller = gtk::EventControllerLegacy {
                            set_propagation_phase: gtk::PropagationPhase::Capture,
                            connect_event[sender] => move |_, event| {
                                use gtk::gdk::EventType;
                                if matches!(
                                    event.event_type(),
                                    EventType::ButtonRelease
                                        | EventType::TouchEnd
                                        | EventType::TouchCancel
                                        | EventType::KeyRelease
                                        | EventType::Scroll
                                ) {
                                    sender.input(SettingsInput::CommitMaxMemory);
                                }
                                // Let GtkScale handle the event as usual.
                                gtk::glib::Propagation::Proceed
                            },
                        },

                    },

                },
                add = &adw::ComboRow {
                    set_title: "Java Runtime",
                   #[watch]
                    set_subtitle: &model.config.read().default_java.as_ref().map_or_else(
                        || "Automatically match based on game version".to_string(),
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
            max_memory_dirty: false,
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
                self.max_memory_dirty = true;
            }
            SettingsInput::CommitMaxMemory => {
                if self.max_memory_dirty {
                    self.max_memory_dirty = false;
                    self.save_config(&sender);
                }
            }
            SettingsInput::SetMaxConcurrentDownloads(val) => {
                self.config.write().max_concurrent_downloads = val;
            }
            SettingsInput::SetCloseOnLaunch(val) => {
                self.config.write().close_launcher_on_launch = val;
            }
            SettingsInput::SelectDefaultJava(index) => {
                let mut cfg = self.config.write();
                if index == 0 {
                    cfg.default_java = None;
                } else if let Some(java) = cfg.java_list.get(index - 1).cloned() {
                    cfg.default_java = Some(java);
                }
                drop(cfg);
            }
            SettingsInput::ScanJava => {
                self.config.write().search_java();
                self.refresh_java_model();
                self.save_config(&sender);
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

#[cfg(test)]
mod tests {
    use super::*;
    use acl_core::java::{JavaInfo, JavaInstallation};

    fn make_mock_java(path: &str, major_version: u32) -> JavaInstallation {
        JavaInstallation {
            path: PathBuf::from(path),
            info: JavaInfo {
                version: format!("{major_version}.0.1"),
                major_version,
                vendor: Some("MockVendor".to_string()),
                is_64bit: true,
            },
        }
    }

    #[test]
    fn test_settings_msg_equality() {
        let msg1 = SettingsMsg::ShowToast("Success".to_string());
        let msg2 = SettingsMsg::ShowToast("Success".to_string());
        let msg3 = SettingsMsg::ShowToast("Error".to_string());
        assert_eq!(msg1, msg2);
        assert_ne!(msg1, msg3);
    }

    #[test]
    fn test_default_java_index_mapping() {
        let java1 = make_mock_java("/path/java17", 17);
        let java2 = make_mock_java("/path/java21", 21);

        let compute_index = |cfg: &LauncherConfig| -> u32 {
            match &cfg.default_java {
                None => 0,
                Some(def) => cfg
                    .java_list
                    .iter()
                    .position(|j| j.path == def.path)
                    .map(|pos| (pos + 1) as u32)
                    .unwrap_or(0),
            }
        };

        let mut cfg = LauncherConfig {
            java_list: vec![java1.clone(), java2.clone()],
            default_java: None,
            ..Default::default()
        };

        // When default_java is None, index must be 0 (Auto-detect)
        assert_eq!(compute_index(&cfg), 0);

        // When default_java is the first Java in list, index is 1
        cfg.default_java = Some(java1);
        assert_eq!(compute_index(&cfg), 1);

        // When default_java is the second Java in list, index is 2
        cfg.default_java = Some(java2);
        assert_eq!(compute_index(&cfg), 2);

        // When default_java is not in java_list, fallback to 0 (Auto-detect)
        let unlisted_java = make_mock_java("/path/unlisted", 8);
        cfg.default_java = Some(unlisted_java);
        assert_eq!(compute_index(&cfg), 0);
    }

    #[test]
    fn test_select_default_java_logic() {
        let java1 = make_mock_java("/path/java17", 17);
        let java2 = make_mock_java("/path/java21", 21);

        let mut cfg = LauncherConfig {
            java_list: vec![java1.clone(), java2.clone()],
            default_java: Some(java1.clone()),
            ..Default::default()
        };

        // Selecting index 0 sets default_java to None (Auto-detect)
        let select_idx = 0;
        if select_idx == 0 {
            cfg.default_java = None;
        } else if let Some(j) = cfg.java_list.get(select_idx - 1).cloned() {
            cfg.default_java = Some(j);
        }
        assert_eq!(cfg.default_java, None);

        // Selecting index 2 sets default_java to java2
        let select_idx = 2;
        if select_idx == 0 {
            cfg.default_java = None;
        } else if let Some(j) = cfg.java_list.get(select_idx - 1).cloned() {
            cfg.default_java = Some(j);
        }
        assert_eq!(cfg.default_java, Some(java2));

        // Selecting out-of-bounds index (e.g. 99) does not modify default_java
        let select_idx = 99;
        if select_idx == 0 {
            cfg.default_java = None;
        } else if let Some(j) = cfg.java_list.get(select_idx - 1).cloned() {
            cfg.default_java = Some(j);
        }
        assert_eq!(cfg.default_java, Some(make_mock_java("/path/java21", 21)));
    }
}
