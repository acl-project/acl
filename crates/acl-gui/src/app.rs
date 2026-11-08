// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{str::FromStr, sync::Arc, time::Duration};

use crate::{
    i18n::apply_language,
    pages::{
        accounts::{AccountsInit, AccountsOutput, AccountsPage},
        home::{HomeInit, HomeOutput, HomePage},
        instances::{InstancesInit, InstancesPage, InstancesPageOutput},
        settings::{SettingsOutput, SettingsPage},
    },
    t,
    widgets::ms_login::{MsLoginDialog, MsLoginInit, MsLoginInput, MsLoginOutput},
};
use adw::prelude::*;
use gtk::prelude::GtkWindowExt;
use libadwaita::NavigationPage;
use relm4::{SharedState, prelude::*};

use acl_auth::AccountStore;
use acl_core::{LauncherConfig, LauncherDirs, manager::InstanceManager};
use tracing::error;

const USER_AGENT: &str = concat!(
    "Mozilla/5.0 (X11; Linux x86_64) AdwaitaCraftLauncher/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/acl-project/acl)"
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NavPage {
    #[default]
    Home,
    Instances,
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
        }
    }

    fn title(self) -> String {
        match self {
            Self::Home => t!("navigation-page-home", "Home"),
            Self::Instances => t!("navigation-page-instances", "Instances"),
            Self::Accounts => t!("navigation-page-accounts", "Accounts"),
            Self::Settings => t!("navigation-page-settings", "Settings"),
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
            _ => Err(()),
        }
    }
}

pub struct AppModel {
    current_page: NavPage,
    /// Used to pop up hint messages or errors below.
    toast_overlay: adw::ToastOverlay,
    home_page: Controller<HomePage>,
    instances_page: Controller<InstancesPage>,
    accounts_page: Controller<AccountsPage>,
    settings_page: Controller<SettingsPage>,
    ms_login: Controller<MsLoginDialog>,

    stack: adw::ViewStack,

    #[allow(dead_code)]
    config: Arc<SharedState<LauncherConfig>>,
    #[allow(dead_code)]
    accounts: Arc<SharedState<AccountStore>>,
    #[allow(dead_code)]
    instances: Arc<SharedState<InstanceManager>>,
    #[allow(dead_code)]
    client: reqwest::Client,
}

#[derive(Debug)]
pub enum AppMsg {
    Navigate(NavPage),
    ShowToast(String),
    StartMicrosoftLogin,
    PromptReauth(String),
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

                #[name = "split_view"]
                adw::NavigationSplitView {
                    #[wrap(Some)]
                    set_sidebar = &adw::NavigationPage {

                        set_title: "ACL",

                        #[wrap(Some)]
                        set_child = &adw::ToolbarView {
                            add_top_bar = &adw::HeaderBar {},
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

                                add_titled_with_icon: (model.home_page.widget(), Some(NavPage::Home.as_str()), &NavPage::Home.title(), "go-home-symbolic"),
                                add_titled_with_icon: (model.instances_page.widget(), Some(NavPage::Instances.as_str()), &NavPage::Instances.title(), "view-grid-symbolic"),
                                add_titled_with_icon: (model.accounts_page.widget(), Some(NavPage::Accounts.as_str()), &NavPage::Accounts.title(), "avatar-default-symbolic"),
                                add_titled_with_icon: (model.settings_page.widget(), Some(NavPage::Settings.as_str()), &NavPage::Settings.title(), "emblem-system-symbolic"),

                                #[watch]
                                set_visible_child_name: model.current_page.as_str(),
                            },
                        }
                    }

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
            });
        let instances_page = InstancesPage::builder()
            .launch(InstancesInit {
                mgr: Arc::clone(&instances_state),
                config: Arc::clone(&config_state),
                client: client.clone(),
                dirs: dirs.clone(),
            })
            .forward(sender.input_sender(), |msg| match msg {
                InstancesPageOutput::ShowToast(message) => AppMsg::ShowToast(message),
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

        config_state.subscribe(sender.input_sender(), |_| AppMsg::Refresh);

        let toast_overlay = adw::ToastOverlay::new();
        let model = AppModel {
            current_page: NavPage::Home,
            toast_overlay,
            home_page,
            instances_page,
            accounts_page,
            settings_page,
            ms_login,
            stack: adw::ViewStack::new(),
            config: config_state,
            accounts: accounts_state,
            instances: instances_state,
            client,
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
