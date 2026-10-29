use std::str::FromStr;

use crate::pages::{
    accounts::AccountsPage, home::HomePage, instances::InstancesPage, settings::SettingsPage,
};
use adw::prelude::*;
use gtk::prelude::GtkWindowExt;
use libadwaita::NavigationPage;
use relm4::prelude::*;

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
    home: Controller<HomePage>,
    instances: Controller<InstancesPage>,
    accounts: Controller<AccountsPage>,
    settings: Controller<SettingsPage>,
}

#[derive(Debug)]
pub enum AppMsg {
    Navigate(NavPage),
    LaunchGame,
}

#[relm4::component(pub)]
impl SimpleComponent for AppModel {
    type Input = AppMsg;

    type Output = ();

    type Init = ();

    view! {

        adw::ApplicationWindow {
            set_title: Some("Adwaita Craft Launcher"),
            set_default_width: 860,
            set_default_height: 540,

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

        }
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        sender: relm4::prelude::ComponentSender<Self>,
    ) -> relm4::prelude::ComponentParts<Self> {
        let home = HomePage::builder().launch(()).detach();
        let instances = InstancesPage::builder().launch(()).detach();
        let accounts = AccountsPage::builder().launch(()).detach();
        let settings = SettingsPage::builder().launch(()).detach();
        let model = AppModel {
            current_page: NavPage::Home,
            home,
            instances,
            accounts,
            settings,
        };

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
