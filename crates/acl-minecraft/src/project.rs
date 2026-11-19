// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{cmp::Ordering, collections::HashMap};

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

    // Truly inspired architecture by CurseForge—who needs named constants when you have mysterious, undocumented magic numbers?
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

/// Unified list item display data (converted from Modrinth or CurseForge)
#[derive(Debug, Clone)]
pub struct AssetItemData {
    pub host: ProjectHost,
    pub project_type: ProjectType,
    pub project_id: String,
    pub title: String,
    pub author: String,
    pub description: String,
    pub icon_url: Option<String>,
    pub downloads: u64,
    pub display_categories: Vec<String>,
    pub latest_version: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReleaseType {
    #[default]
    Release,
    Beta,
    Alpha,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DependencyType {
    /// Dependency must be installed (such as Fabric API)
    Required,
    /// Optional dependency
    Optional,
    /// Conflict and incompatibility
    Incompatible,
    /// Already built into the current Jar
    Embedded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetDependency {
    pub host: ProjectHost,
    pub project_id: String,
    pub version_id: Option<String>,
    pub dependency_type: DependencyType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetFile {
    pub url: String,
    pub filename: String,
    pub size: u64,
    pub sha1: Option<String>,
    pub sha512: Option<String>,
}

/// Platform-independent unified resource version
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetVersion {
    pub host: ProjectHost,
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub version_number: String,
    pub release_type: ReleaseType,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>, // Convert to lowercase uniformly, such as "fabric", "forge", "neoforge", "quilt"
    pub date_published: String,
    pub downloads: u64,
    pub primary_file: Option<AssetFile>,
    pub dependencies: Vec<AssetDependency>,
}

pub fn match_best_version(
    versions: &[AssetVersion],
    project_type: ProjectType,
    mc_version: Option<&str>,
    loader: Option<&str>,
) -> Option<AssetVersion> {
    let mut fallback = None;

    for v in versions {
        let mc_ok = mc_version.is_none_or(|mc| v.game_versions.iter().any(|gv| gv == mc));
        let loader_ok = match project_type {
            ProjectType::Mod => {
                loader.is_some_and(|l| v.loaders.iter().any(|vl| vl.eq_ignore_ascii_case(l)))
            }
            _ => true,
        };

        if mc_ok && loader_ok {
            if v.release_type == ReleaseType::Release {
                return Some(v.clone());
            }
            if fallback.is_none() {
                fallback = Some(v);
            }
        }
    }

    fallback.cloned()
}

pub fn group_versions_by_game_version(
    versions: &[AssetVersion],
) -> Vec<(String, Vec<AssetVersion>)> {
    let mut map: HashMap<String, Vec<AssetVersion>> = std::collections::HashMap::new();

    for ver in versions {
        for gv in &ver.game_versions {
            map.entry(gv.clone()).or_default().push(ver.clone());
        }
    }

    let mut groups: Vec<(String, Vec<AssetVersion>)> = map.into_iter().collect();
    groups.sort_by(|(a, _), (b, _)| {
        if a == b {
            Ordering::Equal
        } else if acl_common::version::is_newer_version(a, b) {
            Ordering::Less
        } else {
            Ordering::Greater
        }
    });

    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_project_type_curseforge_class_id() {
        assert_eq!(ProjectType::Mod.curseforge_class_id(), 6);
        assert_eq!(ProjectType::Modpack.curseforge_class_id(), 4471);
        assert_eq!(ProjectType::ResourcePack.curseforge_class_id(), 12);
        assert_eq!(ProjectType::Shader.curseforge_class_id(), 6552);
    }

    #[test]
    fn test_search_index_curseforge_sort_field() {
        assert_eq!(SearchIndex::Relevance.curseforge_sort_field(), 1);
        assert_eq!(SearchIndex::Follows.curseforge_sort_field(), 2);
        assert_eq!(SearchIndex::Updated.curseforge_sort_field(), 3);
        assert_eq!(SearchIndex::Downloads.curseforge_sort_field(), 6);
        assert_eq!(SearchIndex::Newest.curseforge_sort_field(), 11);
    }

    #[test]
    fn test_project_host_default() {
        assert_eq!(ProjectHost::default(), ProjectHost::Modrinth);
    }
}
