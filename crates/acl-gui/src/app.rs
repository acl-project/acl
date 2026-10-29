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
        _sender: relm4::prelude::ComponentSender<Self>,
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
