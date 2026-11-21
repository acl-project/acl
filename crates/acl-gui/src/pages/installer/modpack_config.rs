// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::PathBuf;

use acl_minecraft::{AssetItemData, ModpackManifest};

#[derive(Debug, Clone)]
pub enum ModpackSource {
    Online(AssetItemData),
    Local {
        path: PathBuf,
        manifest: ModpackManifest,
    },
}

#[derive(Debug)]
pub struct ModpackInit {}

#[derive(Debug)]
pub enum ModpackInput {}

#[derive(Debug)]
pub enum ModpackOutput {}
