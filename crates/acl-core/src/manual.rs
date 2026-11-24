// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Persistence for CurseForge modpack files that graciously require manual human intervention.
//!
//! Because nothing elevates the modded Minecraft experience quite like alt-tabbing out of a
//! native launcher, opening a web browser, waiting through countdown timers, and admiring
//! high-value banner advertisements for every single third-party-restricted mod.
//!
//! Since CurseForge's API thoughtfully omits download URLs to safeguard this vital user
//! journey, we serialize these files here so users can savor the manual download process
//! at their own leisure.

use std::{
    fs::{self, File},
    path::{Path, PathBuf},
};

use crate::{CoreError, Result};
use acl_minecraft::ManualModpackFile;
use serde::{Deserialize, Serialize};
use tracing::info;
pub const MANUAL_DOWNLOADS_FILE: &str = "manual_downloads.json";
const MANUAL_DOWNLOADS_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingManualFile {
    #[serde(flatten)]
    pub file: ManualModpackFile,
    #[serde(default)]
    pub skipped: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingManualDownloads {
    pub version: u32,
    #[serde(default)]
    pub files: Vec<PendingManualFile>,
}

impl PendingManualDownloads {
    pub fn path(instance_dir: &Path) -> PathBuf {
        instance_dir.join(MANUAL_DOWNLOADS_FILE)
    }

    pub fn exists(instance_dir: &Path) -> bool {
        Self::path(instance_dir).exists()
    }

    pub fn from_manual_files(files: Vec<ManualModpackFile>) -> Self {
        let pending_files = files
            .into_iter()
            .map(|file| PendingManualFile {
                file,
                skipped: false,
            })
            .collect();

        Self {
            version: MANUAL_DOWNLOADS_SCHEMA_VERSION,
            files: pending_files,
        }
    }

    pub fn load(instance_dir: &Path) -> Result<Option<Self>> {
        let path = Self::path(instance_dir);
        if !path.exists() {
            return Ok(None);
        }

        let fils = File::open(&path).map_err(|source| CoreError::Io { path, source })?;

        let list: PendingManualDownloads = serde_json::from_reader(fils)?;

        Ok(Some(list))
    }

    pub fn save(&self, instance_dir: &Path) -> Result<()> {
        let path = Self::path(instance_dir);

        let content = serde_json::to_string_pretty(self)?;

        acl_common::fs::write_atomic(&path, content.as_bytes()).map_err(|source| {
            CoreError::Io {
                path: path.clone(),
                source,
            }
        })?;

        info!("Save {} success", path.display());

        Ok(())
    }

    pub fn clear(instance_dir: &Path) -> Result<()> {
        let path = Self::path(instance_dir);
        if !path.exists() {
            return Ok(());
        }

        fs::remove_file(&path).map_err(|source| CoreError::Io {
            path: path.clone(),
            source,
        })?;

        info!("Remove {} success", path.display());

        Ok(())
    }
}
