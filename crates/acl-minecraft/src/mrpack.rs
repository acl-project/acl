// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    MinecraftError, ProjectHost, Result,
    modpack::{
        ModpackFile, ModpackFileSource, ModpackLoader, ModpackManifest, normalize_relative_path,
    },
    modrinth::{FileHashes, SideSupport},
};

pub const MRPACK_FORMAT_VERSION: u32 = 1;
pub const MRPACK_INDEX_FILE: &str = "modrinth.index.json";
pub const MRPACK_GAME: &str = "minecraft";

const fn default_required_side() -> SideSupport {
    SideSupport::Required
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MrpackFileEnv {
    #[serde(default = "default_required_side")]
    pub client: SideSupport,
    #[serde(default = "default_required_side")]
    pub server: SideSupport,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MrpackFile {
    /// Relative path inside the instance directory, e.g. "mods/sodium.jar"
    pub path: String,
    #[serde(default)]
    pub hashes: FileHashes,
    /// If omitted, the file is required on both client and server
    #[serde(default)]
    pub env: Option<MrpackFileEnv>,
    #[serde(default)]
    pub downloads: Vec<String>,
    pub file_size: u64,
}

impl MrpackFile {
    /// Returns `None` if unsupported on the client, `Some(true)` if required,
    /// and `Some(false)` if optional.
    pub fn client_requirement(&self) -> Option<bool> {
        match self.env.map(|env| env.client) {
            None | Some(SideSupport::Required | SideSupport::Unknown) => Some(true),
            Some(SideSupport::Optional) => Some(false),
            Some(SideSupport::Unsupported) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MrpackIndex {
    pub format_version: u32,
    pub game: String,
    pub version_id: String,
    pub name: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub files: Vec<MrpackFile>,
    #[serde(default)]
    pub dependencies: BTreeMap<String, String>,
}

fn parse_mrpack_dependencies(
    dependencies: &BTreeMap<String, String>,
) -> Result<(String, Option<ModpackLoader>)> {
    let mut game_version: Option<String> = None;
    let mut loader: Option<ModpackLoader> = None;

    for (key, value) in dependencies {
        let value = value.trim();
        if value.is_empty() {
            return Err(MinecraftError::ModpackEmptyDependencyVersion {
                dependency: key.clone(),
            });
        }

        if key == "minecraft" {
            game_version = Some(value.to_string());
            continue;
        }

        let parsed_loader = match key.as_str() {
            "fabric-loader" => ModpackLoader::Fabric(value.to_string()),
            "quilt-loader" => ModpackLoader::Quilt(value.to_string()),
            "forge" => ModpackLoader::Forge(value.to_string()),
            "neoforge" => ModpackLoader::NeoForge(value.to_string()),
            other => {
                return Err(MinecraftError::ModpackUnsupportedDependency {
                    dependency: other.to_string(),
                });
            }
        };

        if loader.is_some() {
            return Err(MinecraftError::ModpackMultipleModLoaders);
        }
        loader = Some(parsed_loader);
    }

    let game_version =
        game_version.ok_or_else(|| MinecraftError::ModpackMissingMinecraftVersion)?;

    Ok((game_version, loader))
}

impl TryFrom<MrpackIndex> for ModpackManifest {
    type Error = MinecraftError;

    fn try_from(index: MrpackIndex) -> Result<Self> {
        if index.format_version != MRPACK_FORMAT_VERSION {
            return Err(MinecraftError::ModpackUnsupportedFormatVersion {
                version: index.format_version,
            });
        }

        if index.game != MRPACK_GAME {
            return Err(MinecraftError::ModpackUnsupportedGame { game: index.game });
        }

        let (game_version, loader) = parse_mrpack_dependencies(&index.dependencies)?;

        let mut files = Vec::with_capacity(index.files.len());
        for file in index.files {
            let Some(required) = file.client_requirement() else {
                continue;
            };

            let normalized_path = normalize_relative_path(&file.path)?;
            if file.downloads.is_empty() {
                return Err(MinecraftError::ModpackMissingDownloads {
                    path: normalized_path,
                });
            }

            files.push(ModpackFile {
                source: ModpackFileSource::Direct {
                    path: normalized_path,
                    downloads: file.downloads,
                    size: file.file_size,
                    sha1: file.hashes.sha1,
                    sha512: file.hashes.sha512,
                },
                required,
            });
        }

        Ok(Self {
            host: ProjectHost::Modrinth,
            name: index.name,
            version: index.version_id,
            summary: index.summary,
            author: None,
            game_version,
            loader,
            recommended_ram_mb: None,
            override_dirs: vec!["overrides".to_string(), "client-overrides".to_string()],
            files,
        })
    }
}
