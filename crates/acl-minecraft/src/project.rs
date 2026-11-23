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

    pub const fn from_curseforge_class_id(class_id: Option<u32>) -> ProjectType {
        match class_id {
            Some(4471) => ProjectType::Modpack,
            Some(12) => ProjectType::ResourcePack,
            Some(6552) => ProjectType::Shader,
            _ => ProjectType::Mod,
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
    pub website_url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReleaseType {
    #[default]
    Release,
    Beta,
    Alpha,
}

impl ReleaseType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Release => "Release",
            Self::Alpha => "Alpha",
            Self::Beta => "Beta",
        }
    }
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

        for variant in [
            ProjectType::Mod,
            ProjectType::Modpack,
            ProjectType::ResourcePack,
            ProjectType::Shader,
        ] {
            assert_eq!(
                ProjectType::from_curseforge_class_id(Some(variant.curseforge_class_id())),
                variant
            );
        }

        // None and unknown class IDs fall back to ProjectType::Mod
        assert_eq!(
            ProjectType::from_curseforge_class_id(None),
            ProjectType::Mod
        );
        assert_eq!(
            ProjectType::from_curseforge_class_id(Some(99999)),
            ProjectType::Mod
        );
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

    fn sample_version(
        id: &str,
        release_type: ReleaseType,
        game_versions: &[&str],
        loaders: &[&str],
    ) -> AssetVersion {
        AssetVersion {
            host: ProjectHost::Modrinth,
            id: id.to_string(),
            project_id: "proj1".to_string(),
            name: format!("Version {id}"),
            version_number: id.to_string(),
            release_type,
            game_versions: game_versions.iter().map(|s| (*s).to_string()).collect(),
            loaders: loaders.iter().map(|s| (*s).to_string()).collect(),
            date_published: "2026-01-01T00:00:00Z".to_string(),
            downloads: 100,
            primary_file: None,
            dependencies: Vec::new(),
        }
    }

    #[test]
    fn test_release_type_label() {
        assert_eq!(ReleaseType::default(), ReleaseType::Release);
        assert_eq!(ReleaseType::Release.label(), "Release");
        assert_eq!(ReleaseType::Beta.label(), "Beta");
        assert_eq!(ReleaseType::Alpha.label(), "Alpha");
    }

    #[test]
    fn test_match_best_version_for_mod_and_other_types() {
        let beta_first = sample_version("v-beta", ReleaseType::Beta, &["1.20.1"], &["fabric"]);
        let release_second =
            sample_version("v-release", ReleaseType::Release, &["1.20.1"], &["fabric"]);
        let forge_release =
            sample_version("v-forge", ReleaseType::Release, &["1.20.1"], &["forge"]);
        let alpha_only = sample_version("v-alpha", ReleaseType::Alpha, &["1.21.1"], &["neoforge"]);
        let versions = vec![beta_first, release_second, forge_release, alpha_only];

        // Prefers Release over an earlier Beta for matching MC version + loader
        let matched =
            match_best_version(&versions, ProjectType::Mod, Some("1.20.1"), Some("fabric"))
                .expect("should match fabric release");
        assert_eq!(matched.id, "v-release");

        // Loader comparison is case-insensitive
        let matched_ci =
            match_best_version(&versions, ProjectType::Mod, Some("1.20.1"), Some("Fabric"))
                .expect("should match Fabric case-insensitively");
        assert_eq!(matched_ci.id, "v-release");

        // Falls back to Beta/Alpha when no Release exists
        let fallback = match_best_version(
            &versions,
            ProjectType::Mod,
            Some("1.21.1"),
            Some("neoforge"),
        )
        .expect("should fall back to alpha");
        assert_eq!(fallback.id, "v-alpha");

        // Vanilla (loader = None) cannot install mods
        assert_eq!(
            match_best_version(&versions, ProjectType::Mod, Some("1.20.1"), None),
            None
        );

        // Unmatched game version or loader returns None
        assert_eq!(
            match_best_version(&versions, ProjectType::Mod, Some("1.19.2"), Some("fabric")),
            None
        );
        assert_eq!(
            match_best_version(&versions, ProjectType::Mod, Some("1.20.1"), Some("quilt")),
            None
        );

        // ResourcePack, Shader, and Modpack ignore loader restrictions and prefer Release
        for asset_type in [
            ProjectType::ResourcePack,
            ProjectType::Shader,
            ProjectType::Modpack,
        ] {
            let with_mc = match_best_version(&versions, asset_type, Some("1.20.1"), None)
                .expect("should match release for 1.20.1 without loader");
            assert_eq!(with_mc.id, "v-release");

            let any_mc = match_best_version(&versions, asset_type, None, Some("vanilla"))
                .expect("should match release when mc_version is None");
            assert_eq!(any_mc.id, "v-release");
        }
    }

    #[test]
    fn test_group_versions_by_game_version() {
        assert!(group_versions_by_game_version(&[]).is_empty());

        let v_old = sample_version("v-old", ReleaseType::Release, &["1.19.2"], &["fabric"]);
        let v_multi = sample_version(
            "v-multi",
            ReleaseType::Release,
            &["1.20.1", "1.21.1"],
            &["fabric"],
        );
        let v_20_second = sample_version("v-20-2", ReleaseType::Beta, &["1.20.1"], &["fabric"]);

        let groups = group_versions_by_game_version(&[v_old, v_multi, v_20_second]);
        let group_versions: Vec<&str> = groups.iter().map(|(gv, _)| gv.as_str()).collect();
        assert_eq!(group_versions, vec!["1.21.1", "1.20.1", "1.19.2"]);

        let ids_1_21_1: Vec<&str> = groups[0].1.iter().map(|v| v.id.as_str()).collect();
        assert_eq!(ids_1_21_1, vec!["v-multi"]);

        let ids_1_20_1: Vec<&str> = groups[1].1.iter().map(|v| v.id.as_str()).collect();
        assert_eq!(ids_1_20_1, vec!["v-multi", "v-20-2"]);

        let ids_1_19_2: Vec<&str> = groups[2].1.iter().map(|v| v.id.as_str()).collect();
        assert_eq!(ids_1_19_2, vec!["v-old"]);
    }
}
