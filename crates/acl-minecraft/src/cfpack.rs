// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{collections::HashMap, path::Path};

use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::{
    MinecraftError, ModpackFile, ModpackFileSource, ModpackLoader, ModpackManifest, ProjectHost,
    ProjectType, ResolvedModpackFile, Result,
    curseforge::{self, ModFile, ModInfo},
    modpack::{
        ManualDownloadReason, ManualModpackFile, ModpackResolution, normalize_relative_path,
    },
};

pub const CFPACK_MANIFEST_FILE: &str = "manifest.json";
pub const CFPACK_MANIFEST_TYPE: &str = "minecraftModpack";
pub const CFPACK_MANIFEST_VERSION: u32 = 1;
pub const CFPACK_DEFAULT_OVERRIDES: &str = "overrides";

const fn default_true() -> bool {
    true
}

fn default_overrides() -> String {
    CFPACK_DEFAULT_OVERRIDES.to_string()
}

fn curseforge_target_dir(class_id: Option<u32>) -> &'static str {
    match ProjectType::from_curseforge_class_id(class_id) {
        ProjectType::ResourcePack => "resourcepacks",
        ProjectType::Shader => "shaderpacks",
        ProjectType::Mod | ProjectType::Modpack => "mods",
    }
}

fn curseforge_file_page_url(project_id: u64, file_id: u64) -> String {
    format!("https://www.curseforge.com/projects/{project_id}/files/{file_id}")
}

fn sanitize_curseforge_file_name(raw: &str) -> Result<String> {
    let normalized = normalize_relative_path(raw)?;
    if normalized.contains('/') {
        return Err(MinecraftError::ModpackUnsafePath {
            path: raw.to_string(),
        });
    }
    Ok(normalized)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfpackModLoader {
    pub id: String,
    #[serde(default)]
    pub primary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfpackMinecraft {
    pub version: String,
    #[serde(default)]
    pub mod_loaders: Vec<CfpackModLoader>,
    #[serde(default)]
    pub recommended_ram: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CfpackFile {
    // Uppercase ID is not Id
    #[serde(rename = "projectID")]
    pub project_id: u64,
    #[serde(rename = "fileID")]
    pub file_id: u64,
    #[serde(default = "default_true")]
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfpackManifest {
    pub minecraft: CfpackMinecraft,
    pub manifest_type: String,
    pub manifest_version: u32,
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub files: Vec<CfpackFile>,
    #[serde(default = "default_overrides")]
    pub overrides: String,
}

// Apparently having separate "name" and "version" fields in JSON was asking for too much.
fn parse_cfpack_loader_id(id: &str) -> Result<ModpackLoader> {
    let trimmed = id.trim();
    let Some((raw_kind, raw_version)) = trimmed.split_once('-') else {
        return Err(MinecraftError::ModpackUnsupportedDependency {
            dependency: trimmed.to_string(),
        });
    };

    let kind = raw_kind.trim().to_ascii_lowercase();
    let version = raw_version.trim();

    if !matches!(kind.as_str(), "forge" | "neoforge" | "fabric" | "quilt") {
        return Err(MinecraftError::ModpackUnsupportedDependency {
            dependency: trimmed.to_string(),
        });
    }

    if version.is_empty() {
        return Err(MinecraftError::ModpackEmptyDependencyVersion { dependency: kind });
    }

    let version = version.to_string();

    match kind.as_str() {
        "forge" => Ok(ModpackLoader::Forge(version)),
        "neoforge" => Ok(ModpackLoader::NeoForge(version)),
        "fabric" => Ok(ModpackLoader::Fabric(version)),
        "quilt" => Ok(ModpackLoader::Quilt(version)),
        _ => Err(MinecraftError::ModpackUnsupportedDependency {
            dependency: trimmed.to_string(),
        }),
    }
}

fn parse_cfpack_mod_loaders(mod_loaders: &[CfpackModLoader]) -> Result<Option<ModpackLoader>> {
    if mod_loaders.is_empty() {
        return Ok(None);
    }

    let mut primaries = mod_loaders.iter().filter(|loader| loader.primary);

    let selected = match (primaries.next(), primaries.next()) {
        // Exactly one primary loader
        (Some(primary), None) => primary,
        // Multiple primary loaders
        (Some(_), Some(_)) => return Err(MinecraftError::ModpackMultipleModLoaders),
        // No primary flag set: fall back only if there is a single loader entry
        (None, _) => match mod_loaders {
            [only] => only,
            _ => return Err(MinecraftError::ModpackMultipleModLoaders),
        },
    };

    let loader = parse_cfpack_loader_id(&selected.id)?;
    Ok(Some(loader))
}

impl TryFrom<CfpackManifest> for ModpackManifest {
    type Error = MinecraftError;

    fn try_from(manifest: CfpackManifest) -> Result<Self> {
        if manifest.manifest_version != CFPACK_MANIFEST_VERSION {
            return Err(MinecraftError::ModpackUnsupportedFormatVersion {
                version: manifest.manifest_version,
            });
        }

        if manifest.manifest_type != CFPACK_MANIFEST_TYPE {
            return Err(MinecraftError::ModpackUnsupportedGame {
                game: manifest.manifest_type,
            });
        }

        let game_version = manifest.minecraft.version.trim();
        if game_version.is_empty() {
            return Err(MinecraftError::ModpackMissingMinecraftVersion);
        }

        let loader = parse_cfpack_mod_loaders(&manifest.minecraft.mod_loaders)?;

        let override_dirs = {
            let trimmed = manifest.overrides.trim();
            if trimmed.is_empty() {
                Vec::new()
            } else {
                vec![normalize_relative_path(trimmed)?]
            }
        };

        let author = manifest
            .author
            .map(|a| a.trim().to_string())
            .filter(|a| !a.is_empty());

        let recommended_ram_mb = manifest.minecraft.recommended_ram.filter(|&ram| ram > 0);

        let files = manifest
            .files
            .into_iter()
            .map(|file| ModpackFile {
                source: ModpackFileSource::CurseForge {
                    project_id: file.project_id,
                    file_id: file.file_id,
                },
                required: file.required,
            })
            .collect();

        Ok(Self {
            host: ProjectHost::CurseForge,
            name: manifest.name,
            version: manifest.version,
            summary: None,
            author,
            game_version: game_version.to_string(),
            loader,
            recommended_ram_mb,
            override_dirs,
            files,
        })
    }
}

pub async fn resolve_files(
    entries: &[CfpackFile],
    client: &reqwest::Client,
    cache_dir: &Path,
) -> Result<ModpackResolution> {
    if entries.is_empty() {
        return Ok(ModpackResolution::default());
    }

    let file_ids: Vec<u64> = entries.iter().map(|e| e.file_id).collect();
    let project_ids: Vec<u64> = entries.iter().map(|e| e.project_id).collect();

    let (files, mods) = tokio::try_join!(
        curseforge::fetch_files_batch_with_cache(client, &file_ids, cache_dir, false),
        curseforge::fetch_projects_batch_with_cache(client, &project_ids, cache_dir, false),
    )?;

    let files_map: HashMap<u64, ModFile> = files.into_iter().map(|f| (f.id, f)).collect();
    let mods_map: HashMap<u64, ModInfo> = mods.into_iter().map(|m| (m.id, m)).collect();

    let mut resolution = ModpackResolution {
        files: Vec::with_capacity(entries.len()),
        manual_files: Vec::new(),
    };

    for entry in entries {
        let mod_info = mods_map.get(&entry.project_id);
        let project_name = mod_info.map(|m| m.name.clone());
        let target_dir = curseforge_target_dir(mod_info.and_then(|m| m.class_id));
        let page_url = curseforge_file_page_url(entry.project_id, entry.file_id);

        let Some(mod_file) = files_map.get(&entry.file_id) else {
            warn!(
                "CurseForge file metadata not found for project {}, file {}",
                entry.project_id, entry.file_id
            );

            resolution.manual_files.push(ManualModpackFile {
                project_id: entry.project_id,
                file_id: entry.file_id,
                project_name,
                file_name: None,
                target_dir: target_dir.to_string(),
                page_url,
                required: entry.required,
                reason: ManualDownloadReason::MetadataNotFound,
            });
            continue;
        };

        let Some(download_url) = mod_file
            .download_url
            .as_deref()
            .map(str::trim)
            .filter(|u| !u.is_empty())
        else {
            warn!(
                "CurseForge file '{}' (project {}, file {}) requires manual download",
                mod_file.file_name, entry.project_id, entry.file_id
            );
            resolution.manual_files.push(ManualModpackFile {
                project_id: entry.project_id,
                file_id: entry.file_id,
                project_name,
                file_name: Some(mod_file.file_name.clone()),
                target_dir: target_dir.to_string(),
                page_url,
                required: entry.required,
                reason: ManualDownloadReason::DistributionDisabled,
            });
            continue;
        };

        let clean_name = sanitize_curseforge_file_name(&mod_file.file_name)?;
        let path = format!("{target_dir}/{clean_name}");

        resolution.files.push(ResolvedModpackFile {
            path,
            urls: vec![download_url.to_string()],
            size: mod_file.file_length,
            sha1: mod_file.sha1(),
            sha512: None,
        });
    }

    Ok(resolution)
}
