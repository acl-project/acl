// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

pub mod app;
pub mod i18n;
pub mod pages;
pub mod widgets;

mod system;

use std::process::ExitCode;

use acl_core::LauncherDirs;
use relm4::adw;

use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use crate::app::AppModel;

fn init_logging() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"))
        .add_directive("relm4=warn".parse().unwrap()) // Avoid printing framework logs for update_cmd_with_view.
        .add_directive("reqwest=warn".parse().unwrap()) // Avoid having reqwest print the full request body.
        .add_directive("hyper=warn".parse().unwrap());
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

fn main() -> ExitCode {
    init_logging();

    info!("Starting Adwaita Craft Launcher");

    if let Err(err) = adw::init() {
        error!("Failed to initialize libadwaita: {err}");
        return ExitCode::FAILURE;
    }

    let dirs = match LauncherDirs::new() {
        Ok(dirs) => dirs,
        Err(err) => {
            error!("Failed to resolve launcher directories: {err}");
            return ExitCode::FAILURE;
        }
    };

    let app = relm4::RelmApp::new("com.acl-project.adwaita-craft-launcher");
    app.run::<AppModel>(dirs);

    ExitCode::SUCCESS
}
