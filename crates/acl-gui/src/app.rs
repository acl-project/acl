// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{str::FromStr, sync::Arc, time::Duration};

use crate::{
    i18n::apply_language,
    pages::{
        accounts::{AccountsInit, AccountsOutput, AccountsPage},
        home::{HomeInit, HomeOutput, HomePage},
        instance_detail::{
            InstanceDetail, InstanceDetailInit, InstanceDetailInput, InstanceDetailOutput,
        },
        instances::{InstancesInit, InstancesPage, InstancesPageOutput},
        settings::{SettingsOutput, SettingsPage},
        tasks::{TasksInit, TasksPage},
    },
    t,
    widgets::ms_login::{MsLoginDialog, MsLoginInit, MsLoginInput, MsLoginOutput},
};
use adw::prelude::*;
use gtk::prelude::GtkWindowExt;
use libadwaita::NavigationPage;
use relm4::{SharedState, prelude::*};

use acl_auth::AccountStore;
use acl_core::{LauncherConfig, LauncherDirs, manager::InstanceManager, task::TaskManager};
use tracing::error;

const USER_AGENT: &str = concat!(
    "Mozilla/5.0 (X11; Linux x86_64) AdwaitaCraftLauncher/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/acl-project/acl)"
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppView {
    #[default]
    Main,
    InstanceDetail,
    Download,
    Installer,
}
impl AppView {
    fn as_str(self) -> &'static str {
        match self {
            Self::Main => "main",
            Self::InstanceDetail => "instance-detail",
            Self::Download => "download",
            Self::Installer => "installer",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NavPage {
    #[default]
    Home,
    Instances,
    Tasks,
    Accounts,
    Settings,
}

impl NavPage {
    fn as_str(self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::Instances => "instances",
            Self::Accounts => "accounts",
            Self::Settings => "settings",
            Self::Tasks => "tasks",
        }
    }

    fn title(self) -> String {
        match self {
            Self::Home => t!("navigation-page-home", "Home"),
            Self::Instances => t!("navigation-page-instances", "Instances"),
            Self::Accounts => t!("navigation-page-accounts", "Accounts"),
            Self::Settings => t!("navigation-page-settings", "Settings"),
            Self::Tasks => t!("navigation-page-tasks", "Tasks"),
        }
    }
}

impl FromStr for NavPage {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "home" => Ok(Self::Home),
            "instances" => Ok(Self::Instances),
            "accounts" => Ok(Self::Accounts),
            "settings" => Ok(Self::Settings),
            "tasks" => Ok(Self::Tasks),
            _ => Err(()),
        }
    }
}

pub struct AppModel {
    current_page: NavPage,
    current_view: AppView,
    /// Used to pop up hint messages or errors below.
    toast_overlay: adw::ToastOverlay,
    home_page: Controller<HomePage>,
    instances_page: Controller<InstancesPage>,
    accounts_page: Controller<AccountsPage>,
    settings_page: Controller<SettingsPage>,
    tasks_page: Controller<TasksPage>,
    ms_login: Controller<MsLoginDialog>,

    instance_detail: Controller<InstanceDetail>,
    stack: adw::ViewStack,

    #[allow(dead_code)]
    config: Arc<SharedState<LauncherConfig>>,
    #[allow(dead_code)]
    accounts: Arc<SharedState<AccountStore>>,
    #[allow(dead_code)]
    instances: Arc<SharedState<InstanceManager>>,
    #[allow(dead_code)]
    client: reqwest::Client,
    #[allow(dead_code)]
    tasks: Arc<TaskManager>,
}

#[derive(Debug)]
pub enum AppMsg {
    Navigate(NavPage),
    ShowToast(String),
    StartMicrosoftLogin,
    /// Open the reauth dialog; the String is the expired username.
    PromptReauth(String),
    /// Open the instance details page; the String is the instance id.
    OpenInstanceDetail(String),
    CloseInstanceDetail,
    OpenDownload,
    Refresh,
    Quit,
}

#[relm4::component(pub)]
impl SimpleComponent for AppModel {
    type Input = AppMsg;

    type Output = ();

    type Init = LauncherDirs;

    view! {

        adw::ApplicationWindow {
            set_title: Some("Adwaita Craft Launcher"),
            set_default_width: 860,
            set_default_height: 540,
           #[local_ref]
            toast_overlay -> adw::ToastOverlay {

                gtk::Stack  {
                    set_transition_type: gtk::StackTransitionType::SlideLeftRight,
                    #[name = "split_view"]
                    add_named[Some(AppView::Main.as_str())] = &adw::NavigationSplitView {
                        #[wrap(Some)]
                        set_sidebar = &adw::NavigationPage {

                            set_title: "ACL",

                            #[wrap(Some)]
                            set_child = &adw::ToolbarView {
                                add_top_bar = &adw::HeaderBar {},
                                #[wrap(Some)]
                                set_content = &gtk::Box {
                                    set_orientation: gtk::Orientation::Vertical,

                                    adw::ViewSwitcherSidebar {
                                        set_vexpand: true,
                                        set_stack: Some(stack),
                                    },

                                    gtk::Separator {},

                                    gtk::Button {
                                        add_css_class: "flat",
                                        set_margin_all: 6,
                                        connect_clicked => AppMsg::OpenDownload,

                                        gtk::Box {
                                            set_orientation: gtk::Orientation::Horizontal,
                                            set_spacing: 12,
                                            set_margin_start: 6,
                                            set_margin_end: 6,
                                            set_margin_top: 6,
                                            set_margin_bottom: 6,

                                            gtk::Image {
                                                set_icon_name: Some("folder-download-symbolic"),
                                            },

                                            gtk::Label {
                                                set_hexpand: true,
                                                set_halign: gtk::Align::Start,
                                                #[watch]
                                                set_label: &t!("navigation-page-download", "Download Center"),
                                            },

                                            gtk::Image {
                                                set_icon_name: Some("go-next-symbolic"),
                                                add_css_class: "dim-label",
                                            },

                                        }
                                    },

                                }
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

                                    add_titled_with_icon: (model.home_page.widget(), Some(NavPage::Home.as_str()), &NavPage::Home.title(), "go-home-symbolic"),
                                    add_titled_with_icon: (model.instances_page.widget(), Some(NavPage::Instances.as_str()), &NavPage::Instances.title(), "view-grid-symbolic"),
                                    add_titled_with_icon: (model.tasks_page.widget(), Some(NavPage::Tasks.as_str()), &NavPage::Tasks.title(), "view-list-symbolic"),
                                    add_titled_with_icon: (model.accounts_page.widget(), Some(NavPage::Accounts.as_str()), &NavPage::Accounts.title(), "avatar-default-symbolic"),
                                    add_titled_with_icon: (model.settings_page.widget(), Some(NavPage::Settings.as_str()), &NavPage::Settings.title(), "emblem-system-symbolic"),

                                    #[watch]
                                    set_visible_child_name: model.current_page.as_str(),
                                },
                            }
                        }

                    },

                    add_named[Some(AppView::InstanceDetail.as_str())] = model.instance_detail.widget(),

                    #[watch]
                    set_visible_child_name: model.current_view.as_str(),
                },
            },
        }
    }

    fn init(
        dirs: Self::Init,
        root: Self::Root,
        sender: relm4::prelude::ComponentSender<Self>,
    ) -> relm4::prelude::ComponentParts<Self> {
        // Initialize launcher core

        let config = match LauncherConfig::load_or_recommended(&dirs.config_file()) {
            Ok(cfg) => cfg,
            Err(err) => {
                error!("Failed to load config, using defaults: {err}");
                let err = t!(
                    "app-init-error-load-config",
                    "Failed to load config, using defaults: {err}",
                    err = err
                );
                sender.input(AppMsg::ShowToast(err));
                LauncherConfig::default()
            }
        };

        apply_language(config.language);

        let config_state = Arc::new(SharedState::<LauncherConfig>::new());
        *config_state.write() = config;

        let accounts_path = dirs.accounts_file();
        let store = match AccountStore::load_or_default(&accounts_path) {
            Ok(s) => s,
            Err(err) => {
                error!("Failed to load accounts, using defaults: {err}");
                let err_msg = t!(
                    "app-init-error-load-accounts",
                    "Failed to load accounts, using defaults: {err}",
                    err = err
                );
                sender.input(AppMsg::ShowToast(err_msg));
                AccountStore::default()
            }
        };
        let accounts_state = Arc::new(SharedState::<AccountStore>::new());
        *accounts_state.write() = store;

        let instances_mgr =
            match InstanceManager::load(dirs.versions_dir(), dirs.config_dir().to_path_buf()) {
                Ok(s) => s,
                Err(err) => {
                    error!("Failed to load instances, using defaults: {err}");
                    let err_msg = t!(
                        "app-init-error-load-instances",
                        "Failed to load instances, using defaults: {err}",
                        err = err
                    );
                    sender.input(AppMsg::ShowToast(err_msg));
                    InstanceManager::default()
                }
            };

        let instances_state = Arc::new(SharedState::<InstanceManager>::new());
        *instances_state.write() = instances_mgr;

        // Network client
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .connect_timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_else(|err| {
                error!("Failed to build custom HTTP client, using default: {err}");
                reqwest::Client::new()
            });

        let tasks = TaskManager::new();

        // Initialize ui widgets

        let ms_login = MsLoginDialog::builder()
            .launch(MsLoginInit {
                parent: root.clone().upcast(),
                store: Arc::clone(&accounts_state),
                accounts_path: accounts_path.clone(),
                client: client.clone(),
            })
            .forward(sender.input_sender(), |msg| match msg {
                MsLoginOutput::ShowToast(message) => AppMsg::ShowToast(message),
            });

        let instance_detail = InstanceDetail::builder()
            .launch(InstanceDetailInit {
                config: Arc::clone(&config_state),
                instances: Arc::clone(&instances_state),
            })
            .forward(sender.input_sender(), |msg| match msg {
                InstanceDetailOutput::ShowToast(message) => AppMsg::ShowToast(message),
                InstanceDetailOutput::Back => AppMsg::CloseInstanceDetail,
            });

        let home_page = HomePage::builder()
            .launch(HomeInit {
                mgr: Arc::clone(&instances_state),
                accounts: Arc::clone(&accounts_state),
                config: Arc::clone(&config_state),
                client: client.clone(),
                dirs: dirs.clone(),
            })
            .forward(sender.input_sender(), |msg| match msg {
                HomeOutput::ShowToast(message) => AppMsg::ShowToast(message),
                HomeOutput::RequestClose => AppMsg::Quit,
                HomeOutput::PromptReauth(username) => AppMsg::PromptReauth(username),
                HomeOutput::OpenInstanceDetail(id) => AppMsg::OpenInstanceDetail(id),
            });
        let instances_page = InstancesPage::builder()
            .launch(InstancesInit {
                mgr: Arc::clone(&instances_state),
                config: Arc::clone(&config_state),
                client: client.clone(),
                dirs: dirs.clone(),
                tasks: Arc::clone(&tasks),
            })
            .forward(sender.input_sender(), |msg| match msg {
                InstancesPageOutput::ShowToast(message) => AppMsg::ShowToast(message),
                InstancesPageOutput::OpenDetail(id) => AppMsg::OpenInstanceDetail(id),
            });

        let accounts_page = AccountsPage::builder()
            .launch(AccountsInit {
                store: Arc::clone(&accounts_state),
                config: Arc::clone(&config_state),
                accounts_path,
                client: client.clone(),
            })
            .forward(sender.input_sender(), |msg| match msg {
                AccountsOutput::ShowToast(message) => AppMsg::ShowToast(message),
                AccountsOutput::RequestMicrosoftLogin => AppMsg::StartMicrosoftLogin,
            });

        let settings_page = SettingsPage::builder()
            .launch((Arc::clone(&config_state), dirs.config_file()))
            .forward(sender.input_sender(), |msg| match msg {
                SettingsOutput::ShowToast(message) => AppMsg::ShowToast(message),
            });

        let tasks_page = TasksPage::builder()
            .launch(TasksInit {
                config: Arc::clone(&config_state),
                mgr: Arc::clone(&instances_state),
                client: client.clone(),
                dirs: dirs.clone(),
                tasks: Arc::clone(&tasks),
            })
            .forward(sender.input_sender(), |msg| match msg {});

        config_state.subscribe(sender.input_sender(), |_| AppMsg::Refresh);

        let toast_overlay = adw::ToastOverlay::new();
        let model = AppModel {
            current_page: NavPage::Home,
            current_view: AppView::Main,
            toast_overlay,
            home_page,
            instances_page,
            accounts_page,
            settings_page,
            tasks_page,
            ms_login,
            instance_detail,
            stack: adw::ViewStack::new(),
            config: config_state,
            accounts: accounts_state,
            instances: instances_state,
            client,
            tasks: Arc::clone(&tasks),
        };

        let toast_overlay = &model.toast_overlay;
        let stack = &model.stack;
        let widgets = view_output!();

        model.stack.connect_visible_child_name_notify({
            let sender = sender.clone();
            let split_view = widgets.split_view.clone();
            move |stack| {
                if let Some(name) = stack.visible_child_name()
                    && let Ok(page) = name.parse::<NavPage>()
                {
                    sender.input(AppMsg::Navigate(page));
                }
                split_view.set_show_content(true);
            }
        });

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, _sender: ComponentSender<Self>) {
        match msg {
            AppMsg::Navigate(page) => {
                self.current_page = page;
            }
            AppMsg::ShowToast(msg) => {
                let toast = adw::Toast::builder().title(msg).timeout(3).build();
                self.toast_overlay.add_toast(toast);
            }
            AppMsg::Quit => {
                relm4::main_application().quit();
            }
            AppMsg::StartMicrosoftLogin => {
                self.ms_login.emit(MsLoginInput::Start);
            }
            AppMsg::PromptReauth(username) => {
                self.ms_login.emit(MsLoginInput::PromptReauth { username });
            }
            AppMsg::Refresh => {
                self.stack
                    .page(self.home_page.widget())
                    .set_title(Some(&NavPage::Home.title()));
                self.stack
                    .page(self.instances_page.widget())
                    .set_title(Some(&NavPage::Instances.title()));
                self.stack
                    .page(self.accounts_page.widget())
                    .set_title(Some(&NavPage::Accounts.title()));
                self.stack
                    .page(self.settings_page.widget())
                    .set_title(Some(&NavPage::Settings.title()));
            }
            AppMsg::OpenInstanceDetail(id) => {
                self.instance_detail
                    .emit(InstanceDetailInput::SetCurrentInstance(id));
                self.current_view = AppView::InstanceDetail;
            }
            AppMsg::CloseInstanceDetail => self.current_view = AppView::Main,
            AppMsg::OpenDownload => self.current_view = AppView::Download,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nav_page_default() {
        assert_eq!(NavPage::default(), NavPage::Home);
    }

    #[test]
    fn test_nav_page_as_str_and_title() {
        let cases = [
            (NavPage::Home, "home", "Home"),
            (NavPage::Instances, "instances", "Instances"),
            (NavPage::Accounts, "accounts", "Accounts"),
            (NavPage::Settings, "settings", "Settings"),
        ];

        for (page, expected_str, expected_title) in cases {
            assert_eq!(page.as_str(), expected_str);
            assert_eq!(page.title(), expected_title);
        }
    }

    #[test]
    fn test_nav_page_from_str_success_and_roundtrip() {
        let pages = [
            NavPage::Home,
            NavPage::Instances,
            NavPage::Accounts,
            NavPage::Settings,
        ];

        for page in pages {
            let str_val = page.as_str();
            let parsed: Result<NavPage, ()> = str_val.parse();
            assert_eq!(parsed, Ok(page));
        }
    }

    #[test]
    fn test_nav_page_from_str_invalid() {
        let invalid_inputs = ["", "Home", "instances ", "unknown", "SETTINGS"];

        for input in invalid_inputs {
            let parsed: Result<NavPage, ()> = input.parse();
            assert!(parsed.is_err());
        }
    }
}
