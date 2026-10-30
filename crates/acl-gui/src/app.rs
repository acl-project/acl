use std::{str::FromStr, sync::Arc};

use crate::pages::{
    accounts::AccountsPage,
    home::HomePage,
    instances::InstancesPage,
    settings::{SettingsMsg, SettingsPage},
};
use adw::prelude::*;
use gtk::prelude::GtkWindowExt;
use libadwaita::NavigationPage;
use relm4::{SharedState, prelude::*};

use acl_core::{LauncherConfig, LauncherDirs};
use tracing::error;

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
    home: Controller<HomePage>,
    instances: Controller<InstancesPage>,
    accounts: Controller<AccountsPage>,
    settings: Controller<SettingsPage>,

    #[allow(dead_code)]
    config: Arc<SharedState<LauncherConfig>>,
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

                                add_titled_with_icon: (model.home.widget(), Some(NavPage::Home.as_str()), NavPage::Home.title(), "go-home-symbolic"),
                                add_titled_with_icon: (model.instances.widget(), Some(NavPage::Instances.as_str()), NavPage::Instances.title(), "view-grid-symbolic"),
                                add_titled_with_icon: (model.accounts.widget(), Some(NavPage::Accounts.as_str()), NavPage::Accounts.title(), "avatar-default-symbolic"),
                                add_titled_with_icon: (model.settings.widget(), Some(NavPage::Settings.as_str()), NavPage::Settings.title(), "emblem-system-symbolic"),

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

        // Initialize ui widgets
        let home = HomePage::builder().launch(()).detach();
        let instances = InstancesPage::builder().launch(()).detach();
        let accounts = AccountsPage::builder().launch(()).detach();

        let settings = SettingsPage::builder()
            .launch((Arc::clone(&config_state), dirs.config_file()))
            .forward(sender.input_sender(), |msg| match msg {
                SettingsMsg::ShowToast(message) => AppMsg::ShowToast(message),
            });

        let toast_overlay = adw::ToastOverlay::new();
        let model = AppModel {
            current_page: NavPage::Home,
            toast_overlay,
            home,
            instances,
            accounts,
            settings,
            config: config_state,
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
