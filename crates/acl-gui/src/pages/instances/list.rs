use std::sync::Arc;

use acl_core::{Instance, manager::InstanceManager};

use gtk::prelude::*;
use libadwaita::prelude::*;
use relm4::{SharedState, prelude::*};
use tracing::error;

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
            set_title: &self.instance.config.name,
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
                set_icon_name: "user-trash-symbolic",
                set_valign: gtk::Align::Center,
                add_css_class: "flat",
                set_tooltip_text: Some("Remove instance"),
                connect_clicked[delete_dialog, row] => move |_| {
                    delete_dialog.present(Some(&row));
                },
            },



        },

        #[name = "delete_dialog"]
        adw::AlertDialog {
            set_heading: Some("Delete Instance"),
            #[watch]
            set_body: &format!("Are you sure you want to delete instance \"{}\"?", self.instance.config.name),

            add_response: ("cancel", "Cancel"),
            add_response: ("delete", "Delete"),
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

#[relm4::component(pub)]
impl SimpleComponent for ListPage {
    type Init = Arc<SharedState<InstanceManager>>;
    type Input = ListInput;
    type Output = ListOutput;

    view! {
        #[root]
        #[name = "page"]
        adw::PreferencesPage {
            set_title: "Instances",

            #[local_ref]
            instances_group -> adw::PreferencesGroup {
                set_title: "Instances",
                set_description: Some("Select an instance to launch Minecraft"),

                #[wrap(Some)]
                set_header_suffix = &gtk::Button {
                    add_css_class: "flat",
                    set_valign: gtk::Align::Center,
                    set_tooltip_text: Some("Create an instance"),

                    adw::ButtonContent {
                        set_icon_name: "list-add-symbolic",
                        set_label: "Create",
                    },

                    connect_clicked[sender] => move |_| {
                        let _ = sender.output(ListOutput::OpenCreate);
                    },
                },

            },

        },


    }

    fn init(
        mgr: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let instance_rows = FactoryVecDeque::builder()
            .launch(adw::PreferencesGroup::default())
            .forward(sender.input_sender(), |output| match output {
                InstanceRowOutput::Select(id) => ListInput::SelectInstance(id),
                InstanceRowOutput::Remove(id) => ListInput::RemoveInstance(id),
            });

        let mut model = Self { mgr, instance_rows };

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
                    let msg = format!("Failed to select instance: {err}");
                    error!("{msg}");
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
                    let msg = format!("Failed to remove instance: {err}");
                    error!("{msg}");
                    let _ = sender.output(ListOutput::ShowToast(msg));
                    return;
                }
                self.save_config(&sender);
                self.rebuild_rows();
            }
        }
    }
}
