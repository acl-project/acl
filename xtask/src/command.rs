// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use clap::{Args, Parser, Subcommand};

#[derive(Parser)]
#[command(name = "xtask")]
#[command(about = "ACL workflow tool")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Localization tool.
    I18n(I18n),
}

#[derive(Args)]
pub struct I18n {
    /// Check whether the language file is fully translated.
    #[arg(long)]
    pub check: bool,
}
