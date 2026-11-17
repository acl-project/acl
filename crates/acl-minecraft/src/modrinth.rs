// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{path::Path, time::Duration};

use serde::{Deserialize, Serialize};

use crate::{DownloadSource, Result, cache::fetch_cached_json};

pub const MODRINTH_API_BASE_URL: &str = "https://api.modrinth.com/v2";
pub const DEFAULT_SEARCH_TTL: Duration = Duration::from_secs(300);

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
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Mod => "mod",
            Self::Modpack => "modpack",
            Self::ResourcePack => "resourcepack",
            Self::Shader => "shader",
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
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Relevance => "relevance",
            Self::Downloads => "downloads",
            Self::Follows => "follows",
            Self::Newest => "newest",
            Self::Updated => "updated",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchParams {
    pub query: String,
    pub project_type: ProjectType,
    pub game_version: Option<String>,
    pub loader: Option<String>, // "fabric", "forge", "neoforge", "quilt"
    pub categories: Vec<String>,
    pub index: SearchIndex,
    pub offset: u32,
    pub limit: u32,
}

impl SearchParams {
    /// Construct a two-dimensional facets array of Modrinth, for example:
    /// [["project_type:mod"], ["versions:1.20.1"], ["categories:fabric"]]
    pub fn build_facets(&self) -> Vec<Vec<String>> {
        let mut facets = vec![vec![format!("project_type:{}", self.project_type.as_str())]];
        if let Some(version) = &self.game_version
            && !version.is_empty()
        {
            facets.push(vec![format!("versions:{version}")]);
        }

        if let Some(loader) = &self.loader
            && !loader.is_empty()
        {
            facets.push(vec![format!("categories:{loader}")]);
        }

        for category in &self.categories {
            if !category.is_empty() {
                facets.push(vec![format!("categories:{category}")]);
            }
        }

        facets
    }

    /// Build the complete `/v2/search` request URL
    pub fn build_url(&self, base_url: &str) -> String {
        let mut serializer = url::form_urlencoded::Serializer::new(String::new());
        if !self.query.is_empty() {
            serializer.append_pair("query", &self.query);
        }

        let facets = self.build_facets();
        if let Ok(facets_json) = serde_json::to_string(&facets) {
            serializer.append_pair("facets", &facets_json);
        }

        serializer.append_pair("index", self.index.as_str());
        serializer.append_pair("offset", &self.offset.to_string());
        serializer.append_pair("limit", &self.limit.to_string());

        let query_string = serializer.finish();
        format!("{}/search?{query_string}", base_url.trim_end_matches('/'))
    }

    pub fn url(&self) -> String {
        self.build_url(MODRINTH_API_BASE_URL)
    }
}

/// Client and server mods support enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SideSupport {
    Required,
    Optional,
    Unsupported,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchHit {
    pub project_id: String,
    pub project_type: ProjectType,
    pub slug: String,
    pub author: String,
    pub title: String,
    pub description: String,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub display_categories: Vec<String>,
    #[serde(default)]
    pub versions: Vec<String>,
    pub downloads: u64,
    pub follows: u64,
    #[serde(default)]
    pub icon_url: Option<String>,
    pub date_created: String,
    pub date_modified: String,
    #[serde(default)]
    pub latest_version: Option<String>,
    #[serde(default)]
    pub license: String,
    #[serde(default)]
    pub client_side: SideSupport,
    #[serde(default)]
    pub server_side: SideSupport,
    #[serde(default)]
    pub gallery: Vec<String>,
    #[serde(default)]
    pub featured_gallery: Option<String>,
    #[serde(default)]
    pub color: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchResponse {
    pub hits: Vec<SearchHit>,
    pub offset: u32,
    pub limit: u32,
    pub total_hits: u32,
}

impl SearchResponse {
    pub async fn fetch_with_cache(
        client: &reqwest::Client,
        params: &SearchParams,
        cache_dir: &Path,
        download_source: DownloadSource,
        force_refresh: bool,
    ) -> Result<Self> {
        let upstream_url = params.build_url(MODRINTH_API_BASE_URL);
        fetch_cached_json(
            client,
            &upstream_url,
            cache_dir,
            DEFAULT_SEARCH_TTL,
            download_source,
            force_refresh,
        )
        .await
    }
}

/// Version release type
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionDependency {
    #[serde(default)]
    pub version_id: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub file_name: Option<String>,
    pub dependency_type: DependencyType,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileHashes {
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub sha512: Option<String>,
}

/// Single downloadable file information (such as xxx.jar)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionFile {
    pub hashes: FileHashes,
    pub url: String,
    pub filename: String,
    /// The files that really need to be placed in the game
    #[serde(default)]
    pub primary: bool,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectVersion {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub version_number: String,
    #[serde(default)]
    pub changelog: Option<String>,
    #[serde(default)]
    pub dependencies: Vec<VersionDependency>,
    #[serde(default)]
    pub game_versions: Vec<String>,
    #[serde(default)]
    pub version_type: ReleaseType,
    #[serde(default)]
    pub loaders: Vec<String>,
    #[serde(default)]
    pub featured: bool,
    pub date_published: String,
    pub downloads: u64,
    #[serde(default)]
    pub files: Vec<VersionFile>,
}

impl ProjectVersion {
    /// Get the main download file of this version (first take primary == true, otherwise fall back to the first file)
    pub fn primary_file(&self) -> Option<&VersionFile> {
        self.files
            .iter()
            .find(|f| f.primary)
            .or_else(|| self.files.first())
    }
    /// Construct the request URL of GET /v2/project/{id|slug}/version
    /// When loaders and game_versions are empty slices, no corresponding parameters are appended.
    pub fn list_url(
        base_url: &str,
        project_id_or_slug: &str,
        loaders: &[&str],
        game_versions: &[&str],
    ) -> String {
        let base = format!(
            "{}/project/{project_id_or_slug}/version",
            base_url.trim_end_matches('/')
        );

        let mut serializer = url::form_urlencoded::Serializer::new(String::new());

        let mut has_query = false;

        if !loaders.is_empty()
            && let Ok(json) = serde_json::to_string(loaders)
        {
            serializer.append_pair("loaders", &json);
            has_query = true;
        }

        if !game_versions.is_empty()
            && let Ok(json) = serde_json::to_string(game_versions)
        {
            serializer.append_pair("game_versions", &json);
            has_query = true;
        }

        if has_query {
            format!("{base}?{}", serializer.finish())
        } else {
            base
        }
    }

    /// Pull the version list of the specified project (with cache)
    pub async fn fetch_list_with_cache(
        client: &reqwest::Client,
        project_id_or_slug: &str,
        loaders: &[&str],
        game_versions: &[&str],
        cache_dir: &Path,
        download_source: DownloadSource,
        force_refresh: bool,
    ) -> Result<Vec<Self>> {
        let upstream_url = Self::list_url(
            MODRINTH_API_BASE_URL,
            project_id_or_slug,
            loaders,
            game_versions,
        );
        fetch_cached_json(
            client,
            &upstream_url,
            cache_dir,
            DEFAULT_SEARCH_TTL,
            download_source,
            force_refresh,
        )
        .await
    }
}
