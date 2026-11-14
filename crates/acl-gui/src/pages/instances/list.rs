// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Arc;

use acl_core::{Instance, LauncherConfig, manager::InstanceManager};

use libadwaita::prelude::*;
use relm4::{SharedState, prelude::*};
use tracing::error;

use crate::t;

#[derive(Debug, Clone)]
pub struct InstanceRow {
    instance: Instance,
    is_active: bool,
}

#[derive(Debug)]
pub enum InstanceRowInput {
    SetActive(bool),
}

#[derive(Debug)]
pub enum InstanceRowOutput {
    Select(String),
    Remove(String),
    OpenDetail(String),
}

impl InstanceRow {
    fn subtitle(&self) -> String {
        format!(
            "{} • {}",
            self.instance.config.game_version,
            self.instance.config.mod_loader.label()
        )
    }
}

#[relm4::factory(pub)]
impl FactoryComponent for InstanceRow {
    type Init = (Instance, bool);
    type Input = InstanceRowInput;
    type Output = InstanceRowOutput;
    type CommandOutput = ();
    type ParentWidget = adw::PreferencesGroup;

    view! {
        #[root]
        #[name = "row"]
        adw::ActionRow {
            #[watch]
            set_title: &self.instance.config.name,
            #[watch]
            set_subtitle: &self.subtitle(),

            set_selectable: false,
            set_focus_on_click: false,
            set_activatable: true,

            connect_activated[sender, id = self.instance.id().to_owned()] => move |_| {
                let _ = sender.output(InstanceRowOutput::Select(id.clone()));
            },

            #[name = "select_check"]
            add_suffix = &gtk::CheckButton {

                set_valign: gtk::Align::Center,
                add_css_class: "selection-mode",
                set_can_focus: false,
                set_can_target: false,
                #[watch]
                set_active: self.is_active,
            },

            add_suffix = &gtk::Button {
                set_icon_name: "emblem-system-symbolic",
                set_valign: gtk::Align::Center,
                    add_css_class: "flat",
                    #[watch]
                    set_tooltip_text: Some(&t!("list-tooltip-settings", "Instance settings")),
                    connect_clicked[sender, id = self.instance.id().to_owned()] => move |_| {
                        let _ = sender.output(InstanceRowOutput::OpenDetail(id.clone()));
                    },
            },

            add_suffix = &gtk::Button {

                set_icon_name: "user-trash-symbolic",
                set_valign: gtk::Align::Center,
                add_css_class: "flat",
                #[watch]
                set_tooltip_text: Some(&t!("list-tooltip-remove","Remove instance")),
                connect_clicked[delete_dialog, row] => move |_| {
                    delete_dialog.present(Some(&row));
                },
            },



        },

        #[name = "delete_dialog"]
        adw::AlertDialog {
            #[watch]
            set_heading: Some(&t!("delete-dialog-heading","Delete Instance")),
            #[watch]
            set_body: &t!("delete-dialog-body","Are you sure you want to delete instance \"{name}\"?", name = self.instance.config.name),

            add_response: ("cancel", &t!("delete-dialog-cancel","Cancel")),
            add_response: ("delete",  &t!("delete-dialog-delete","Delete")),

            #[watch]
            set_response_label: ("cancel", &t!("delete-dialog-cancel","Cancel")),
            #[watch]
            set_response_label: ("delete",  &t!("delete-dialog-delete","Delete")),

            set_response_appearance: ("delete", adw::ResponseAppearance::Destructive),
            set_default_response: Some("cancel"),
            set_close_response: "cancel",

            connect_response: (Some("delete"), {
                let sender = sender.clone();
                let id = self.instance.id().to_owned();
                move |_, _| {
                    let _ = sender.output(InstanceRowOutput::Remove(id.clone()));
                }
            }),

        }
    }

    fn init_model(
        (instance, is_active): Self::Init,
        _index: &Self::Index,
        _sender: FactorySender<Self>,
    ) -> Self {
        Self {
            instance,
            is_active,
        }
    }

    fn update(&mut self, msg: Self::Input, _sender: FactorySender<Self>) {
        match msg {
            InstanceRowInput::SetActive(active) => {
                self.is_active = active;
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListInput {
    SelectInstance(String),
    RemoveInstance(String),
    OpenDetail(String),
    Refresh,
}

/// Management page for installed instances.
pub struct ListPage {
    mgr: Arc<SharedState<InstanceManager>>,
    instance_rows: FactoryVecDeque<InstanceRow>,
}

#[derive(Debug, Clone)]
pub enum ListOutput {
    ShowToast(String),
    OpenCreate,
    OpenDetail(String),
}

impl ListPage {
    fn save_config(&self, sender: &ComponentSender<Self>) {
        if let Err(err) = self.mgr.read().save() {
            let msg = format!("Failed to save instances: {err}");
            error!("{msg}");
            let _ = sender.output(ListOutput::ShowToast(msg));
        }
    }

    fn rebuild_rows(&mut self) {
        let instances = self.mgr.read();
        let active_id = instances.config.active_id.clone();
        let mut guard = self.instance_rows.guard();
        guard.clear();
        for instance in &instances.instances {
            let is_active = Some(instance.id().to_string()) == active_id;
            guard.push_back((instance.clone(), is_active));
        }
    }
}

pub struct ListPageInit {
    pub mgr: Arc<SharedState<InstanceManager>>,
    pub config: Arc<SharedState<LauncherConfig>>,
}

#[relm4::component(pub)]
impl SimpleComponent for ListPage {
    type Init = ListPageInit;
    type Input = ListInput;
    type Output = ListOutput;

    view! {
        #[root]
        #[name = "page"]
        adw::PreferencesPage {
            #[watch]
            set_title: &t!("instances-list-title","Instances"),

            #[local_ref]
            instances_group -> adw::PreferencesGroup {
                #[watch]
                set_title: &t!("instances-list-title","Instances"),
                #[watch]
                set_description: Some(&t!("instances-list-description","Select an instance to launch Minecraft")),

                #[wrap(Some)]
                set_header_suffix = &gtk::Button {
                    add_css_class: "flat",
                    set_valign: gtk::Align::Center,
                    #[watch]
                    set_tooltip_text: Some(&t!("instances-list-tooltip-create","Create an instance")),

                    adw::ButtonContent {
                        set_icon_name: "list-add-symbolic",
                        #[watch]
                        set_label: &t!("instances-list-button-create","Create"),
                    },

                    connect_clicked[sender] => move |_| {
                        let _ = sender.output(ListOutput::OpenCreate);
                    },
                },

            },

        },


    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let instance_rows = FactoryVecDeque::builder()
            .launch(adw::PreferencesGroup::default())
            .forward(sender.input_sender(), |output| match output {
                InstanceRowOutput::Select(id) => ListInput::SelectInstance(id),
                InstanceRowOutput::Remove(id) => ListInput::RemoveInstance(id),
                InstanceRowOutput::OpenDetail(id) => ListInput::OpenDetail(id),
            });

        init.config
            .subscribe(sender.input_sender(), |_| ListInput::Refresh);
        init.mgr
            .subscribe(sender.input_sender(), |_| ListInput::Refresh);

        let mut model = Self {
            mgr: init.mgr,
            instance_rows,
        };

        model.rebuild_rows();

        let instances_group = model.instance_rows.widget();
        let widgets = view_output!();

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            ListInput::SelectInstance(id) => {
                let res = self.mgr.write().set_active_instance(&id).map(|_| ());

                if let Err(err) = res {
                    error!("Failed to select instance: {err}");
                    let msg = t!(
                        "instance-list-error-select-instance",
                        "Failed to select instance: {err}",
                        err = err
                    );
                    let _ = sender.output(ListOutput::ShowToast(msg));
                    return;
                }
                self.save_config(&sender);

                for (i, row) in self.instance_rows.iter().enumerate() {
                    let is_active = row.instance.id() == id;
                    self.instance_rows
                        .send(i, InstanceRowInput::SetActive(is_active));
                }
            }
            ListInput::RemoveInstance(id) => {
                if let Err(err) = self.mgr.write().remove_instance(&id) {
                    error!("Failed to remove instance: {err}");
                    let msg = t!(
                        "instance-list-error-remove-instance",
                        "Failed to remove instance: {err}",
                        err = err
                    );
                    let _ = sender.output(ListOutput::ShowToast(msg));
                    return;
                }
                self.save_config(&sender);
                self.rebuild_rows();
            }
            ListInput::Refresh => {
                self.rebuild_rows();
            }

            ListInput::OpenDetail(id) => {
                let _ = sender.output(ListOutput::OpenDetail(id));
            }
        }
    }
}
