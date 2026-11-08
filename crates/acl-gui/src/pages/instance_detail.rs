// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Arc;

use acl_core::LauncherConfig;
use acl_core::manager::InstanceManager;
use adw::prelude::*;
use libadwaita::NavigationPage;
use relm4::prelude::*;
use relm4::{SharedState, SimpleComponent};

use crate::pages::instance_detail::settings::{
    InstanceSettings, InstanceSettingsInit, InstanceSettingsInput, InstanceSettingsOutput,
};
use crate::t;

pub mod settings;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NavPage {
    #[default]
    Settings,
}

impl NavPage {
    fn as_str(self) -> &'static str {
        match self {
            Self::Settings => "settings",
        }
    }

    fn title(self) -> String {
        match self {
            Self::Settings => t!("instance-navigation-page-settings", "Settings"),
        }
    }
}

pub struct InstanceDetail {
    current_page: NavPage,

    stack: adw::ViewStack,

    settings: Controller<InstanceSettings>,
}

#[derive(Debug)]
pub enum InstanceDetailInput {
    ShowToast(String),
    SetCurrentInstance(String),
}

#[derive(Debug)]
pub enum InstanceDetailOutput {
    ShowToast(String),
    Back,
}

pub struct InstanceDetailInit {
    pub config: Arc<SharedState<LauncherConfig>>,
    pub instances: Arc<SharedState<InstanceManager>>,
}

#[relm4::component(pub)]
impl SimpleComponent for InstanceDetail {
    type Init = InstanceDetailInit;
    type Input = InstanceDetailInput;
    type Output = InstanceDetailOutput;

    view! {
        #[root]
        adw::NavigationSplitView {
            #[wrap(Some)]
            set_sidebar = &adw::NavigationPage {

                set_title: "ACL",



                #[wrap(Some)]
                set_child = &adw::ToolbarView {
                    add_top_bar = &adw::HeaderBar {
                        pack_start = &gtk::Button {
                            set_icon_name: "go-previous-symbolic",
                            #[watch]
                            set_tooltip_text: Some(&t!("instance-detail-button-back", "Back")),
                            connect_clicked[sender] => move |_|{
                                let _ = sender.output(InstanceDetailOutput::Back);
                            }
                        },
                    },
                    #[wrap(Some)]
                    set_content = &adw::ViewSwitcherSidebar {
                        set_stack: Some(stack),
                    },
                },
            },

            #[wrap(Some)]
            set_content = &NavigationPage {

                #[watch]
                set_title: &model.current_page.title(),

                #[wrap(Some)]
                set_child = &adw::ToolbarView {

                    add_top_bar = &adw::HeaderBar {},

                    #[wrap(Some)]
                    #[local_ref]
                    set_content = stack ->adw::ViewStack {

                        add_titled_with_icon: (model.settings.widget(), Some(NavPage::Settings.as_str()), &NavPage::Settings.title(), "emblem-system-symbolic"),

                        #[watch]
                        set_visible_child_name: model.current_page.as_str(),
                    },
                }
            }

        },
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: relm4::prelude::ComponentSender<Self>,
    ) -> relm4::prelude::ComponentParts<Self> {
        let settings = InstanceSettings::builder()
            .launch(InstanceSettingsInit {
                config: init.config,
                instances: init.instances,
            })
            .forward(sender.input_sender(), |msg| match msg {
                InstanceSettingsOutput::ShowToast(message) => {
                    InstanceDetailInput::ShowToast(message)
                }
            });
        let model = Self {
            current_page: NavPage::Settings,
            stack: adw::ViewStack::new(),
            settings,
        };

        let stack = &model.stack;

        let widgets = view_output!();

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            InstanceDetailInput::ShowToast(message) => {
                let _ = sender.output(InstanceDetailOutput::ShowToast(message));
            }
            InstanceDetailInput::SetCurrentInstance(id) => {
                self.settings
                    .emit(InstanceSettingsInput::SetCurrentInstance(id));
            }
        }
    }
}
