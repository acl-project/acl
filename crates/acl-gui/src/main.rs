pub mod app;
pub mod pages;

use std::process::ExitCode;

use relm4::adw;

use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use crate::app::AppModel;

fn init_logging() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

fn main() -> ExitCode {
    init_logging();

    info!("Starting Adwaita Craft Launcher");

    if let Err(err) = adw::init() {
        error!("Failed to initialize libadwaita: {err}");
        return ExitCode::FAILURE;
    }

    let app = relm4::RelmApp::new("com.acl-project.adwaita-craft-launcher");
    app.run::<AppModel>(());

    ExitCode::SUCCESS
}
