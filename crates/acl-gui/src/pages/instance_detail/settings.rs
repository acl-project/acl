// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Arc;

use acl_core::LauncherConfig;
use acl_core::manager::InstanceManager;
use gtk::gio::prelude::ListModelExt;

use libadwaita::prelude::*;
use relm4::{ComponentParts, SimpleComponent};
use relm4::{SharedState, prelude::*};
use tracing::error;

use crate::system::memory_info;
use crate::t;

#[derive(Debug)]
pub enum InstanceSettingsOutput {
    ShowToast(String),
}

#[derive(Debug)]
pub enum InstanceSettingsInput {
    SetCurrentInstance(String),
    SetCustomSettings(bool),
    SaveConfig,
    SetName(String),
    SetMaxMemory(u32),
    SelectDefaultJava(usize),
    CommitMaxMemory,
}

pub struct InstanceSettingsInit {
    pub config: Arc<SharedState<LauncherConfig>>,
    pub instances: Arc<SharedState<InstanceManager>>,
}

pub struct InstanceSettings {
    config: Arc<SharedState<LauncherConfig>>,
    instances: Arc<SharedState<InstanceManager>>,
    current: Option<String>,
    java_model: gtk::StringList,
    max_memory_dirty: bool,
}

impl InstanceSettings {
    fn save_config(&self, sender: &ComponentSender<Self>) {
        let Some(id) = self.current.as_deref() else {
            return;
        };
        let instances = self.instances.read();
        let Some(instance) = instances.instance(id) else {
            return;
        };

        if let Err(err) = instance.config.save(&instance.config_file()) {
            error!("Failed to save configuration: {err}");
            let msg = t!(
                "instance-settings-error-save-configuration",
                "Failed to save instance {id} configuration: {err}",
                id = id,
                err = err,
            );
            let _ = sender.output(InstanceSettingsOutput::ShowToast(msg));
        }
    }

    fn refresh_java_model(&mut self) {
        let cfg = self.config.read();

        let mut java_names: Vec<String> = Vec::with_capacity(cfg.java_list.len() + 1);
        java_names.push(t!("settings-java-auto-detect", "Auto-detect (Recommended)"));
        java_names.extend(cfg.java_list.iter().map(|j| {
            let vendor = j.info.vendor.as_deref().unwrap_or("Java");
            format!("{} {}", vendor, j.info.version)
        }));

        let name_refs: Vec<&str> = java_names.iter().map(|s| s.as_str()).collect();

        self.java_model
            .splice(0, self.java_model.n_items(), &name_refs);
    }

    /// Calculate the selected index in "java_model" for the current instance.
    fn current_java_index(&self) -> u32 {
        let cfg = self.config.read();
        let instance_java = self.current.as_deref().and_then(|id| {
            self.instances
                .read()
                .instance(id)
                .and_then(|ins| ins.config.java.clone())
        });

        let target_java = if self.is_custom_enabled() {
            instance_java.as_ref()
        } else {
            cfg.default_java.as_ref()
        };

        match target_java {
            None => 0,
            Some(def) => cfg
                .java_list
                .iter()
                .position(|j| j.path == def.path)
                .map(|pos| (pos + 1) as u32)
                .unwrap_or(0),
        }
    }

    fn current_java_name(&self) -> String {
        let cfg = self.config.read();
        let instance_java = self.current.as_deref().and_then(|id| {
            self.instances
                .read()
                .instance(id)
                .and_then(|ins| ins.config.java.clone())
        });

        let target_java = if self.is_custom_enabled() {
            instance_java.as_ref()
        } else {
            cfg.default_java.as_ref()
        };

        match target_java {
            None => t!(
                "settings-java-runtime-subtitle",
                "Automatically match based on game version"
            ),
            Some(def) => def.path.display().to_string(),
        }
    }

    fn current_name(&self) -> String {
        let Some(current) = self.current.as_deref() else {
            return t!("instance-settings-none", "None");
        };
        self.instances
            .read()
            .instance(current)
            .map(|ins| ins.config.name.clone())
            .unwrap_or(t!("instance-settings-none", "None"))
    }

    /// Whether instance-specific settings are enabled for the current instance.
    fn is_custom_enabled(&self) -> bool {
        let Some(current) = self.current.as_deref() else {
            return false;
        };

        self.instances
            .read()
            .instance(current)
            .is_some_and(|ins| ins.config.custom_settings)
    }

    /// Effective max memory to display (instance override if set, otherwise global).
    fn current_max_memory(&self) -> u32 {
        let global_max = self.config.read().max_memory_mb;
        let Some(current) = self.current.as_deref() else {
            return global_max;
        };

        if !self.is_custom_enabled() {
            return global_max;
        }

        self.instances
            .read()
            .instance(current)
            .and_then(|ins| ins.config.max_memory_mb)
            .unwrap_or(global_max)
    }
}

#[relm4::component(pub)]
impl SimpleComponent for InstanceSettings {
    type Init = InstanceSettingsInit;
    type Input = InstanceSettingsInput;
    type Output = InstanceSettingsOutput;

    view! {
        adw::PreferencesPage{
            adw::PreferencesGroup {
                #[watch]
                set_title: &t!("instance-settings-base-title","Base"),

                add = &adw::ActionRow {
                    #[watch]
                    set_title: &t!("instance-settings-name-title","Instance Name"),
                    #[watch]
                    set_subtitle: &model.current_name(),
                },

                add = &adw::SwitchRow {
                    #[watch]
                    set_title: &t!("instance-settings-custom-title", "Instance-Specific Settings"),
                    #[watch]
                    set_active: model.is_custom_enabled(),
                    connect_active_notify[sender] => move |row| {
                        sender.input(InstanceSettingsInput::SetCustomSettings(row.is_active()));
                        sender.input(InstanceSettingsInput::SaveConfig);
                    },
                }

            },


            adw::PreferencesGroup {
                #[watch]
                set_title: &t!("settings-java-title","Java"),

                #[watch]
                set_sensitive: model.is_custom_enabled(),

                add = &adw::ActionRow {
                    #[watch]
                    set_title: &t!("settings-java-max-memory-title","Max Memory"),
                    #[watch]
                    set_subtitle: &format!("{} MB", model.current_max_memory()),
                    add_suffix = &gtk::Scale {
                        set_width_request: 240,
                        set_hexpand: true,
                        set_draw_value: false,
                        set_digits: 0,
                        set_adjustment: &gtk::Adjustment::new(
                            model.current_max_memory() as f64,
                            model.config.read().min_memory_mb as f64,
                            ((memory_info().total / (1024 * 1024)) as f64).max(model.config.read().min_memory_mb as f64),
                            256.0,
                            512.0,
                            0.0,
                        ),

                        #[watch]
                        set_value: model.current_max_memory() as f64,

                        connect_value_changed[sender] => move |scale| {
                            sender.input(InstanceSettingsInput::SetMaxMemory(scale.value() as u32));
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
                                    sender.input(InstanceSettingsInput::CommitMaxMemory);
                                }
                                // Let GtkScale handle the event as usual.
                                gtk::glib::Propagation::Proceed
                            },
                        },

                    },

                },
                add = &adw::ComboRow {
                    #[watch]
                    set_title: &t!("settings-java-runtime-title","Java Runtime"),
                    #[watch]
                    set_subtitle: &model.current_java_name(),
                    set_model: Some(&java_model),
                    #[watch]
                    set_selected: model.current_java_index(),
                    connect_selected_notify[sender] => move |row| {
                        let idx = row.selected();
                        if idx != gtk::INVALID_LIST_POSITION {
                            sender.input(InstanceSettingsInput::SelectDefaultJava(idx as usize));
                            sender.input(InstanceSettingsInput::SaveConfig);
                        }
                    }
                }
            },


        }
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let java_model = gtk::StringList::new(&[]);

        let mut model = Self {
            config: init.config,
            instances: init.instances,
            current: None,
            java_model: java_model.clone(),
            max_memory_dirty: false,
        };

        model.refresh_java_model();

        let widgets = view_output!();

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            InstanceSettingsInput::SetCurrentInstance(id) => {
                self.current = Some(id);
            }
            InstanceSettingsInput::SetMaxMemory(val) => {
                if !self.is_custom_enabled() {
                    return;
                }
                let Some(current) = self.current.as_deref() else {
                    return;
                };
                if let Some(ins) = self.instances.write().instance_mut(current) {
                    ins.config.max_memory_mb = Some(val);
                    self.max_memory_dirty = true;
                }
            }
            InstanceSettingsInput::CommitMaxMemory => {
                if self.max_memory_dirty {
                    self.max_memory_dirty = false;
                    self.save_config(&sender);
                }
            }
            InstanceSettingsInput::SelectDefaultJava(index) => {
                if !self.is_custom_enabled() {
                    return;
                }
                let Some(current) = self.current.as_deref() else {
                    return;
                };
                let java = if index == 0 {
                    None
                } else {
                    self.config.read().java_list.get(index - 1).cloned()
                };
                if let Some(ins) = self.instances.write().instance_mut(current) {
                    ins.config.java = java;
                }
            }
            InstanceSettingsInput::SetName(name) => {
                let Some(current) = self.current.as_deref() else {
                    return;
                };

                if let Some(ins) = self.instances.write().instance_mut(current) {
                    ins.config.name = name;
                }
            }
            InstanceSettingsInput::SaveConfig => {
                self.save_config(&sender);
            }
            InstanceSettingsInput::SetCustomSettings(enabled) => {
                let Some(current) = self.current.as_deref() else {
                    return;
                };
                if let Some(ins) = self.instances.write().instance_mut(current) {
                    ins.config.custom_settings = enabled;
                }
            }
        }
    }
}
