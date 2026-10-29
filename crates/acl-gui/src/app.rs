use crate::pages::home::HomePage;
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
    home_page: Controller<HomePage>,
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
                        set_content = &gtk::StackSidebar {
                            set_stack: &stack,
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
                        set_content = &gtk::Stack {

                            add_titled: (model.home_page.widget(), Some(NavPage::Home.as_str()), NavPage::Home.title()),

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
        let home_page = HomePage::builder().launch(()).detach();

        let model = AppModel {
            current_page: NavPage::Home,
            home_page,
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
