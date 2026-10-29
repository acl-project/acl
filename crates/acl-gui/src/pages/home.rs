use gtk::prelude::{ButtonExt, OrientableExt, WidgetExt};
use relm4::adw;
use relm4::{ComponentParts, ComponentSender, RelmWidgetExt, SimpleComponent};
pub struct HomePage;

#[derive(Debug)]
pub enum HomeMsg {
    LaunchClicked,
}

#[relm4::component(pub)]
impl SimpleComponent for HomePage {
    type Init = ();
    type Input = HomeMsg;
    type Output = ();

    view! {
        gtk::Box {
            set_orientation: gtk::Orientation::Vertical,
            set_margin_all: 24,

            adw::StatusPage {
                set_vexpand: true,
                set_icon_name: Some("applications-games-symbolic"),
                set_title: "Adwaita Craft Launcher",
                set_description: Some("Select an instance to start playing Minecraft"),
            },

            gtk::Button {
                set_halign: gtk::Align::End,
                add_css_class: "suggested-action",
                add_css_class: "pill",
                connect_clicked => HomeMsg::LaunchClicked,

                adw::ButtonContent {
                    set_icon_name: "media-playback-start-symbolic",
                    set_label: "Start Game",
                }
            },

        }
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let model = HomePage;
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, _sender: ComponentSender<Self>) {
        match msg {
            HomeMsg::LaunchClicked => {}
        }
    }
}
