use gtk::prelude::*;
use relm4::adw;
use relm4::prelude::*;
use tracing::info;
use tracing_subscriber::EnvFilter;
struct AppModel;

#[derive(Debug)]
enum AppMsg {
    Quit,
}

#[relm4::component]
impl SimpleComponent for AppModel {
    type Init = ();
    type Input = AppMsg;
    type Output = ();

    view! {
        adw::ApplicationWindow {
            set_title: Some("Adwaita Craft Launcher"),
            set_default_width: 480,
            set_default_height: 320,

            adw::ToolbarView {
                add_top_bar = &adw::HeaderBar {
                    pack_end = &gtk::Button {
                        set_icon_name: "window-close-symbolic",
                        connect_clicked => AppMsg::Quit,
                    },
                },

                #[wrap(Some)]
                set_content = &gtk::Label {
                    set_label: "Relm4 + GTK4 + libadwaita Ready",
                    set_vexpand: true,
                },
            },
        }
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let model = AppModel;
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, _sender: ComponentSender<Self>) {
        match msg {
            AppMsg::Quit => relm4::main_application().quit(),
        }
    }
}

fn init_logging() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

fn main() {
    init_logging();

    info!("Starting Adwaita Craft Launcher");
    adw::init().expect("Failed to initialize libadwaita");

    let app = relm4::RelmApp::new("com.acl-project.adwaita-craft-launcher");
    app.run::<AppModel>(());
}
