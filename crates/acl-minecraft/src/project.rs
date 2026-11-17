// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectType {
    #[default]
    Mod,
    Modpack,
    ResourcePack,
    Shader,
}

impl ProjectType {
    pub const fn as_modrinth_str(self) -> &'static str {
        match self {
            Self::Mod => "mod",
            Self::Modpack => "modpack",
            Self::ResourcePack => "resourcepack",
            Self::Shader => "shader",
        }
    }

    // Are you out of your mind, CurseForge? Why the fuck are you using these random magic numbers? Fuck you, CurseForge.
    pub const fn curseforge_class_id(self) -> u32 {
        match self {
            Self::Mod => 6,
            Self::Modpack => 4471,
            Self::ResourcePack => 12,
            Self::Shader => 6552,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchIndex {
    #[default]
    Relevance,
    Downloads,
    Follows,
    Newest,
    Updated,
}

impl SearchIndex {
    pub const fn as_modrinth_str(self) -> &'static str {
        match self {
            Self::Relevance => "relevance",
            Self::Downloads => "downloads",
            Self::Follows => "follows",
            Self::Newest => "newest",
            Self::Updated => "updated",
        }
    }

    pub const fn curseforge_sort_field(self) -> u32 {
        match self {
            Self::Relevance => 1, // Featured
            Self::Follows => 2,   // Popularity
            Self::Updated => 3,   // LastUpdated
            Self::Downloads => 6, // TotalDownloads
            Self::Newest => 11,   // ReleasedDate
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProjectHost {
    #[default]
    Modrinth,
    CurseForge,
}

/// Unified search filter conditions
#[derive(Debug, Clone)]
pub struct AssetFilterState {
    pub host: ProjectHost,
    pub project_type: ProjectType, // Mod, ResourcePack, Shader, Modpack
    pub query: String,
    pub game_version: Option<String>,
    pub loader: Option<String>, // Only Mod/Modpack is valid, or Shader corresponds to iris/optifine
    pub category: Option<String>,
    pub sort_by: SearchIndex, // Relevance, Downloads, Updated
    pub offset: u32,
    pub limit: u32,
}
