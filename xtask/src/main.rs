// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::process::ExitCode;

use clap::Parser;

use crate::command::Cli;

mod command;
mod i18n;

fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        command::Commands::I18n(args) => i18n::run(args.check)?,
    }

    Ok(())
}

fn main() -> ExitCode {
    if let Err(err) = run() {
        eprintln!("{err}");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}
