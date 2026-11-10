// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::Path;
use std::sync::Arc;

use acl_core::LauncherConfig;
use acl_core::manager::InstanceManager;
use adw::prelude::*;
use libadwaita::NavigationPage;
use relm4::prelude::*;
use relm4::{SharedState, SimpleComponent};
use tracing::error;

use crate::pages::instance_detail::settings::{
    InstanceSettings, InstanceSettingsInit, InstanceSettingsInput, InstanceSettingsOutput,
};
use crate::t;

pub mod settings;

#[derive(Debug, Clone, Copy)]
pub enum FolderKind {
    Root,
    Saves,
    Screenshots,
    Mods,
    ResourcePacks,
    ShaderPacks,
    Schematics,
    Logs,
}

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
    current: Option<String>,
    stack: adw::ViewStack,
    instances: Arc<SharedState<InstanceManager>>,
    settings: Controller<InstanceSettings>,
}

impl InstanceDetail {
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

    fn open_directory(path: &Path) -> Result<(), String> {
        if let Err(err) = std::fs::create_dir_all(path) {
            error!("Failed to create directory '{}': {err}", path.display());
            return Err(t!(
                "instance-detail-error-create-directory",
                "Failed to create directory: {err}",
                err = err,
            ));
        }

        let file = gtk::gio::File::for_path(path);
        if let Err(err) =
            gtk::gio::AppInfo::launch_default_for_uri(&file.uri(), gtk::gio::AppLaunchContext::NONE)
        {
            error!("Failed to open directory '{}': {err}", path.display());
            return Err(t!(
                "instance-detail-error-open-directory",
                "Failed to open directory: {err}",
                err = err,
            ));
        }

        Ok(())
    }
}

#[derive(Debug)]
pub enum InstanceDetailInput {
    ShowToast(String),
    SetCurrentInstance(String),
    OpenFolder(FolderKind),
    Refresh,
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

                #[watch]
                set_title: &model.current_name(),

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
                        pack_end = &gtk::MenuButton {
                            set_icon_name: "folder-symbolic",
                            #[watch]
                            set_tooltip_text: Some(&t!("instance-detail-tooltip-open-folder", "Open Folder")),
                            set_popover: Some(&folder_popover),
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

        #[name = "folder_popover"]
        gtk::Popover {
            add_css_class: "folder-menu",
            gtk::Box {
                set_orientation: gtk::Orientation::Vertical,
                set_spacing: 4,
                set_margin_top: 6,
                set_margin_bottom: 6,
                set_margin_start: 6,
                set_margin_end: 6,

                gtk::Button {
                    add_css_class: "flat",
                    connect_clicked[sender, folder_popover] => move |_| {
                        folder_popover.popdown();
                        sender.input(InstanceDetailInput::OpenFolder(FolderKind::Root));
                    },
                    adw::ButtonContent {
                        set_icon_name: "folder-symbolic",
                        #[watch]
                        set_label: &t!("instance-folder-root", "Instance Directory"),
                    }
                },

                gtk::Button {
                    add_css_class: "flat",
                    connect_clicked[sender, folder_popover] => move |_| {
                        folder_popover.popdown();
                        sender.input(InstanceDetailInput::OpenFolder(FolderKind::Saves));
                    },
                    adw::ButtonContent {
                        set_icon_name: "document-save-symbolic",
                        #[watch]
                        set_label: &t!("instance-folder-saves", "Saves"),
                    }
                },

                gtk::Button {
                    add_css_class: "flat",
                    connect_clicked[sender, folder_popover] => move |_| {
                        folder_popover.popdown();
                        sender.input(InstanceDetailInput::OpenFolder(FolderKind::Screenshots));
                    },
                    adw::ButtonContent {
                        set_icon_name: "image-x-generic-symbolic",
                        #[watch]
                        set_label: &t!("instance-folder-screenshots", "Screenshots"),
                    }
                },

                gtk::Button {
                    add_css_class: "flat",
                    connect_clicked[sender, folder_popover] => move |_| {
                        folder_popover.popdown();
                        sender.input(InstanceDetailInput::OpenFolder(FolderKind::Mods));
                    },
                    adw::ButtonContent {
                        set_icon_name: "application-x-addon-symbolic",
                        #[watch]
                        set_label: &t!("instance-folder-mods", "Mods"),
                    }
                },

                gtk::Button {
                    add_css_class: "flat",
                    connect_clicked[sender, folder_popover] => move |_| {
                        folder_popover.popdown();
                        sender.input(InstanceDetailInput::OpenFolder(FolderKind::ResourcePacks));
                    },
                    adw::ButtonContent {
                        set_icon_name: "package-x-generic-symbolic",
                        #[watch]
                        set_label: &t!("instance-folder-resourcepacks", "Resource Packs"),
                    }
                },

                gtk::Button {
                    add_css_class: "flat",
                    connect_clicked[sender, folder_popover] => move |_| {
                        folder_popover.popdown();
                        sender.input(InstanceDetailInput::OpenFolder(FolderKind::ShaderPacks));
                    },
                    adw::ButtonContent {
                        set_icon_name: "weather-clear-symbolic",
                        #[watch]
                        set_label: &t!("instance-folder-shaderpacks", "Shader Packs"),
                    }
                },

                gtk::Button {
                    add_css_class: "flat",
                    connect_clicked[sender, folder_popover] => move |_| {
                        folder_popover.popdown();
                        sender.input(InstanceDetailInput::OpenFolder(FolderKind::Schematics));
                    },
                    adw::ButtonContent {
                         set_icon_name: "x-office-drawing-symbolic",
                        #[watch]
                        set_label: &t!("instance-folder-schematics", "Schematics"),
                    }
                },

                gtk::Button {
                    add_css_class: "flat",
                    connect_clicked[sender, folder_popover] => move |_| {
                        folder_popover.popdown();
                        sender.input(InstanceDetailInput::OpenFolder(FolderKind::Logs));
                    },
                    adw::ButtonContent {
                        set_icon_name: "utilities-terminal-symbolic",
                        #[watch]
                        set_label: &t!("instance-folder-logs", "Logs"),
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
        init.instances
            .subscribe(sender.input_sender(), |_| InstanceDetailInput::Refresh);
        init.config
            .subscribe(sender.input_sender(), |_| InstanceDetailInput::Refresh);

        let settings = InstanceSettings::builder()
            .launch(InstanceSettingsInit {
                config: init.config,
                instances: Arc::clone(&init.instances),
            })
            .forward(sender.input_sender(), |msg| match msg {
                InstanceSettingsOutput::ShowToast(message) => {
                    InstanceDetailInput::ShowToast(message)
                }
            });

        let model = Self {
            current_page: NavPage::Settings,
            current: None,
            stack: adw::ViewStack::new(),
            instances: init.instances,
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
                self.current = Some(id.clone());
                self.settings
                    .emit(InstanceSettingsInput::SetCurrentInstance(id));
            }
            InstanceDetailInput::OpenFolder(kind) => {
                let Some(id) = self.current.as_deref() else {
                    return;
                };
                let instances = self.instances.read();
                let Some(instance) = instances.instance(id) else {
                    return;
                };

                let target_dir = match kind {
                    FolderKind::Root => instance.game_dir().to_path_buf(),
                    FolderKind::Saves => instance.saves_dir(),
                    FolderKind::Screenshots => instance.screenshots_dir(),
                    FolderKind::Mods => instance.mods_dir(),
                    FolderKind::ResourcePacks => instance.resourcepacks_dir(),
                    FolderKind::ShaderPacks => instance.shaderpacks_dir(),
                    FolderKind::Schematics => instance.schematics_dir(),
                    FolderKind::Logs => instance.logs_dir(),
                };

                if let Err(err) = Self::open_directory(&target_dir) {
                    let _ = sender.output(InstanceDetailOutput::ShowToast(err));
                }
            }
            InstanceDetailInput::Refresh => {
                self.stack
                    .page(self.settings.widget())
                    .set_title(Some(&NavPage::Settings.title()));
            }
        }
    }
}
