pub mod app;
pub mod pages;

use relm4::adw;

use tracing::info;
use tracing_subscriber::EnvFilter;

use crate::app::AppModel;

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
