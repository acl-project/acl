use std::{str::FromStr, sync::Arc};

use crate::pages::{
    accounts::{AccountsMsg, AccountsPage},
    home::HomePage,
    instances::InstancesPage,
    settings::{SettingsMsg, SettingsPage},
};
use adw::prelude::*;
use gtk::prelude::GtkWindowExt;
use libadwaita::NavigationPage;
use relm4::{SharedState, prelude::*};

use acl_auth::AccountStore;
use acl_core::{Instance, LauncherConfig, LauncherDirs};
use tracing::error;

pub type InstancesVec = Arc<SharedState<Vec<Instance>>>;

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

    fn title(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Instances => "Instances",
            Self::Accounts => "Accounts",
            Self::Settings => "Settings",
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

    #[allow(dead_code)]
    config: Arc<SharedState<LauncherConfig>>,
    #[allow(dead_code)]
    accounts: Arc<SharedState<AccountStore>>,
    #[allow(dead_code)]
    instances: InstancesVec,
}

#[derive(Debug)]
pub enum AppMsg {
    Navigate(NavPage),
    LaunchGame,
    ShowToast(String),
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
                                set_stack: Some(&stack),
                            },
                        },
                    },

                    #[wrap(Some)]
                    set_content = &NavigationPage {

                        #[watch]
                        set_title: model.current_page.title(),

                        #[wrap(Some)]
                        set_child = &adw::ToolbarView {

                            add_top_bar = &adw::HeaderBar {},

                            #[name = "stack"]
                            #[wrap(Some)]
                            set_content = &adw::ViewStack {

                                add_titled_with_icon: (model.home_page.widget(), Some(NavPage::Home.as_str()), NavPage::Home.title(), "go-home-symbolic"),
                                add_titled_with_icon: (model.instances_page.widget(), Some(NavPage::Instances.as_str()), NavPage::Instances.title(), "view-grid-symbolic"),
                                add_titled_with_icon: (model.accounts_page.widget(), Some(NavPage::Accounts.as_str()), NavPage::Accounts.title(), "avatar-default-symbolic"),
                                add_titled_with_icon: (model.settings_page.widget(), Some(NavPage::Settings.as_str()), NavPage::Settings.title(), "emblem-system-symbolic"),

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

        let config = match LauncherConfig::load_or_default(&dirs.config_file()) {
            Ok(cfg) => cfg,
            Err(err) => {
                let err = format!("Failed to load config, using defaults: {err}");
                error!("{err}");
                sender.input(AppMsg::ShowToast(err));
                LauncherConfig::default()
            }
        };

        let config_state = Arc::new(SharedState::<LauncherConfig>::new());
        *config_state.write() = config;

        let accounts_path = dirs.accounts_file();
        let store = match AccountStore::load_or_default(&accounts_path) {
            Ok(s) => s,
            Err(err) => {
                let err_msg = format!("Failed to load accounts, using defaults: {err}");
                error!("{err_msg}");
                sender.input(AppMsg::ShowToast(err_msg));
                AccountStore::default()
            }
        };
        let accounts_state = Arc::new(SharedState::<AccountStore>::new());
        *accounts_state.write() = store;

        let instances = Instance::discover_all(&dirs.versions_dir()).unwrap_or_default();

        // Check whether the current instance exists.
        if let Some(active_id) = &config_state.read().active_instance {
            let alive = instances
                .iter()
                .find(|ins| ins.id() == active_id)
                .map(|ins| ins.exist())
                .unwrap_or(false);
            if !alive {
                config_state.write().active_instance =
                    instances.first().map(|ins| ins.id().to_string());
            }
        }
        let instances_state = Arc::new(SharedState::<Vec<Instance>>::new());
        *instances_state.write() = instances;

        // Initialize ui widgets
        let home_page = HomePage::builder().launch(()).detach();
        let instances_page = InstancesPage::builder().launch(()).detach();

        let accounts_page = AccountsPage::builder()
            .launch((Arc::clone(&accounts_state), accounts_path))
            .forward(sender.input_sender(), |msg| match msg {
                AccountsMsg::ShowToast(message) => AppMsg::ShowToast(message),
            });

        let settings_page = SettingsPage::builder()
            .launch((Arc::clone(&config_state), dirs.config_file()))
            .forward(sender.input_sender(), |msg| match msg {
                SettingsMsg::ShowToast(message) => AppMsg::ShowToast(message),
            });

        let toast_overlay = adw::ToastOverlay::new();
        let model = AppModel {
            current_page: NavPage::Home,
            toast_overlay,
            home_page,
            instances_page,
            accounts_page,
            settings_page,
            config: config_state,
            accounts: accounts_state,
            instances: instances_state,
        };

        let toast_overlay = &model.toast_overlay;
        let widgets = view_output!();

        widgets.stack.connect_visible_child_name_notify({
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
            AppMsg::LaunchGame => {}
            AppMsg::ShowToast(msg) => {
                let toast = adw::Toast::builder().title(msg).timeout(3).build();
                self.toast_overlay.add_toast(toast);
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
