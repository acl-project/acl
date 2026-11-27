// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{path::Path, time::Duration};

use serde::{Deserialize, Serialize};

use crate::{
    DownloadSource, Result,
    cache::fetch_cached_json,
    project::{
        AssetDependency, AssetFile, AssetFilterState, AssetItemData, AssetVersion, DependencyType,
        ProjectHost, ProjectType, ReleaseType, SearchIndex,
    },
};

pub const MODRINTH_API_BASE_URL: &str = "https://api.modrinth.com/v2";
pub const DEFAULT_SEARCH_TTL: Duration = Duration::from_secs(900);

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
        let mut facets = vec![vec![format!(
            "project_type:{}",
            self.project_type.as_modrinth_str()
        )]];
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

        serializer.append_pair("index", self.index.as_modrinth_str());
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectDetail {
    pub id: String,
    pub project_type: ProjectType,
    pub slug: String,
    pub title: String,
    pub description: String,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub icon_url: Option<String>,
    pub downloads: u64,
}

impl ProjectDetail {
    pub fn batch_url(base_url: &str, ids: &[String]) -> String {
        let mut serializer = url::form_urlencoded::Serializer::new(String::new());
        if let Ok(json) = serde_json::to_string(ids) {
            serializer.append_pair("ids", &json);
        }
        format!(
            "{}/projects?{}",
            base_url.trim_end_matches('/'),
            serializer.finish()
        )
    }

    pub async fn fetch_batch_with_cache(
        client: &reqwest::Client,
        ids: &[String],
        cache_dir: &Path,
        download_source: DownloadSource,
        force_refresh: bool,
    ) -> Result<Vec<Self>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let upstream_url = Self::batch_url(MODRINTH_API_BASE_URL, ids);
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

fn modrinth_website_url(project_type: ProjectType, slug: &str, project_id: &str) -> Option<String> {
    let target = if !slug.trim().is_empty() {
        slug.trim()
    } else {
        project_id.trim()
    };

    if target.is_empty() {
        return None;
    }

    Some(format!(
        "https://modrinth.com/{}/{}",
        project_type.as_modrinth_str(),
        target
    ))
}

impl From<SearchHit> for AssetItemData {
    fn from(hit: SearchHit) -> Self {
        // If display_categories is empty, fallback to categories
        let display_categories = if hit.display_categories.is_empty() {
            hit.categories
        } else {
            hit.display_categories
        };

        let website_url = modrinth_website_url(hit.project_type, &hit.slug, &hit.project_id);

        Self {
            host: ProjectHost::Modrinth,
            project_type: hit.project_type,
            project_id: hit.project_id,
            title: hit.title,
            author: hit.author,
            description: hit.description,
            icon_url: hit.icon_url,
            downloads: hit.downloads,
            display_categories,
            latest_version: hit.latest_version,
            website_url,
        }
    }
}

/// Convert the UI's unified filtering status reference to Modrinth's SearchParams
impl From<&AssetFilterState> for SearchParams {
    fn from(state: &AssetFilterState) -> Self {
        Self {
            query: state.query.clone(),
            project_type: state.project_type,
            game_version: state.game_version.clone(),
            loader: state.loader.clone(),
            categories: state.category.iter().cloned().collect(),
            index: state.sort_by,
            offset: state.offset,
            limit: state.limit,
        }
    }
}

impl From<ProjectVersion> for AssetVersion {
    fn from(v: ProjectVersion) -> Self {
        let primary_file = v.primary_file().map(|f| AssetFile {
            url: f.url.clone(),
            filename: f.filename.clone(),
            size: f.size,
            sha1: f.hashes.sha1.clone(),
            sha512: f.hashes.sha512.clone(),
        });

        let dependencies = v
            .dependencies
            .into_iter()
            .filter_map(|d| {
                Some(AssetDependency {
                    host: ProjectHost::Modrinth,
                    project_id: d.project_id?,
                    version_id: d.version_id,
                    dependency_type: d.dependency_type,
                })
            })
            .collect();

        Self {
            host: ProjectHost::Modrinth,
            id: v.id,
            project_id: v.project_id,
            name: v.name,
            version_number: v.version_number,
            release_type: v.version_type,
            game_versions: v.game_versions,
            loaders: v.loaders,
            date_published: v.date_published,
            downloads: v.downloads,
            primary_file,
            dependencies,
        }
    }
}

impl From<ProjectDetail> for AssetItemData {
    fn from(p: ProjectDetail) -> Self {
        let website_url = modrinth_website_url(p.project_type, &p.slug, &p.id);

        Self {
            host: ProjectHost::Modrinth,
            project_type: p.project_type,
            project_id: p.id,
            title: p.title,
            author: String::new(),
            description: p.description,
            icon_url: p.icon_url,
            downloads: p.downloads,
            display_categories: p.categories,
            latest_version: None,
            website_url,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct TempDirGuard {
        path: PathBuf,
    }

    impl TempDirGuard {
        fn new(name: &str) -> Self {
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let id = COUNTER.fetch_add(1, Ordering::SeqCst);
            let path = std::env::temp_dir().join(format!(
                "acl_test_modrinth_{}_{}_{}",
                name,
                std::process::id(),
                id
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("failed to create temp test dir");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn test_project_type_and_search_index_properties_and_serde() {
        assert_eq!(ProjectType::default(), ProjectType::Mod);
        let project_cases = [
            (ProjectType::Mod, "mod", "\"mod\""),
            (ProjectType::Modpack, "modpack", "\"modpack\""),
            (
                ProjectType::ResourcePack,
                "resourcepack",
                "\"resourcepack\"",
            ),
            (ProjectType::Shader, "shader", "\"shader\""),
        ];
        for (variant, expected_str, expected_json) in project_cases {
            assert_eq!(variant.as_modrinth_str(), expected_str);
            let serialized = serde_json::to_string(&variant).expect("serialize ProjectType");
            assert_eq!(serialized, expected_json);
            let deserialized: ProjectType =
                serde_json::from_str(&serialized).expect("deserialize ProjectType");
            assert_eq!(deserialized, variant);
        }

        assert_eq!(SearchIndex::default(), SearchIndex::Relevance);
        let index_cases = [
            (SearchIndex::Relevance, "relevance", "\"relevance\""),
            (SearchIndex::Downloads, "downloads", "\"downloads\""),
            (SearchIndex::Follows, "follows", "\"follows\""),
            (SearchIndex::Newest, "newest", "\"newest\""),
            (SearchIndex::Updated, "updated", "\"updated\""),
        ];
        for (variant, expected_str, expected_json) in index_cases {
            assert_eq!(variant.as_modrinth_str(), expected_str);
            let serialized = serde_json::to_string(&variant).expect("serialize SearchIndex");
            assert_eq!(serialized, expected_json);
            let deserialized: SearchIndex =
                serde_json::from_str(&serialized).expect("deserialize SearchIndex");
            assert_eq!(deserialized, variant);
        }
    }

    #[test]
    fn test_enums_serde_and_defaults() {
        assert_eq!(SideSupport::default(), SideSupport::Unknown);
        for (variant, expected_json) in [
            (SideSupport::Required, "\"required\""),
            (SideSupport::Optional, "\"optional\""),
            (SideSupport::Unsupported, "\"unsupported\""),
            (SideSupport::Unknown, "\"unknown\""),
        ] {
            let serialized = serde_json::to_string(&variant).expect("serialize SideSupport");
            assert_eq!(serialized, expected_json);
            let deserialized: SideSupport =
                serde_json::from_str(&serialized).expect("deserialize SideSupport");
            assert_eq!(deserialized, variant);
        }

        assert_eq!(ReleaseType::default(), ReleaseType::Release);
        for (variant, expected_json) in [
            (ReleaseType::Release, "\"release\""),
            (ReleaseType::Beta, "\"beta\""),
            (ReleaseType::Alpha, "\"alpha\""),
        ] {
            let serialized = serde_json::to_string(&variant).expect("serialize ReleaseType");
            assert_eq!(serialized, expected_json);
            let deserialized: ReleaseType =
                serde_json::from_str(&serialized).expect("deserialize ReleaseType");
            assert_eq!(deserialized, variant);
        }

        for (variant, expected_json) in [
            (DependencyType::Required, "\"required\""),
            (DependencyType::Optional, "\"optional\""),
            (DependencyType::Incompatible, "\"incompatible\""),
            (DependencyType::Embedded, "\"embedded\""),
        ] {
            let serialized = serde_json::to_string(&variant).expect("serialize DependencyType");
            assert_eq!(serialized, expected_json);
            let deserialized: DependencyType =
                serde_json::from_str(&serialized).expect("deserialize DependencyType");
            assert_eq!(deserialized, variant);
        }
    }

    #[test]
    fn test_search_params_build_facets_and_url() {
        let minimal = SearchParams {
            query: String::new(),
            project_type: ProjectType::Mod,
            game_version: Some(String::new()),
            loader: None,
            categories: vec![String::new()],
            index: SearchIndex::Downloads,
            offset: 0,
            limit: 20,
        };
        assert_eq!(
            minimal.build_facets(),
            vec![vec!["project_type:mod".to_string()]]
        );

        let minimal_url = minimal.build_url("https://api.modrinth.com/v2/");
        assert!(minimal_url.starts_with("https://api.modrinth.com/v2/search?"));
        let parsed_minimal = url::Url::parse(&minimal_url).expect("parse minimal url");
        let minimal_pairs: std::collections::HashMap<String, String> =
            parsed_minimal.query_pairs().into_owned().collect();
        assert!(!minimal_pairs.contains_key("query"));
        assert_eq!(
            minimal_pairs.get("facets").map(String::as_str),
            Some(r#"[["project_type:mod"]]"#)
        );
        assert_eq!(
            minimal_pairs.get("index").map(String::as_str),
            Some("downloads")
        );
        assert_eq!(minimal_pairs.get("offset").map(String::as_str), Some("0"));
        assert_eq!(minimal_pairs.get("limit").map(String::as_str), Some("20"));

        let full = SearchParams {
            query: "sodium extra & more".to_string(),
            project_type: ProjectType::Shader,
            game_version: Some("1.21.1".to_string()),
            loader: Some("iris".to_string()),
            categories: vec![
                "optimization".to_string(),
                String::new(),
                "realistic".to_string(),
            ],
            index: SearchIndex::Updated,
            offset: 40,
            limit: 10,
        };
        assert_eq!(
            full.build_facets(),
            vec![
                vec!["project_type:shader".to_string()],
                vec!["versions:1.21.1".to_string()],
                vec!["categories:iris".to_string()],
                vec!["categories:optimization".to_string()],
                vec!["categories:realistic".to_string()],
            ]
        );

        let full_url = full.url();
        assert!(full_url.starts_with("https://api.modrinth.com/v2/search?"));
        assert!(full_url.contains("query=sodium+extra+%26+more"));
        let parsed_full = url::Url::parse(&full_url).expect("parse full url");
        let full_pairs: std::collections::HashMap<String, String> =
            parsed_full.query_pairs().into_owned().collect();
        assert_eq!(
            full_pairs.get("query").map(String::as_str),
            Some("sodium extra & more")
        );
        assert_eq!(
            full_pairs.get("facets").map(String::as_str),
            Some(
                r#"[["project_type:shader"],["versions:1.21.1"],["categories:iris"],["categories:optimization"],["categories:realistic"]]"#
            )
        );
        assert_eq!(full_pairs.get("index").map(String::as_str), Some("updated"));
        assert_eq!(full_pairs.get("offset").map(String::as_str), Some("40"));
        assert_eq!(full_pairs.get("limit").map(String::as_str), Some("10"));
    }

    #[tokio::test]
    async fn test_search_response_deserialization_and_fetch_with_cache() {
        let sample_json = r#"{
            "hits": [
                {
                    "project_id": "AANobbMI",
                    "project_type": "mod",
                    "slug": "sodium",
                    "author": "jellysquid3",
                    "title": "Sodium",
                    "description": "Fastest rendering optimization mod",
                    "categories": ["fabric", "optimization"],
                    "display_categories": ["optimization"],
                    "versions": ["1.20.1", "1.21.1"],
                    "downloads": 45000000,
                    "follows": 120000,
                    "icon_url": "https://cdn.modrinth.com/data/AANobbMI/icon.png",
                    "date_created": "2020-07-09T03:02:55Z",
                    "date_modified": "2024-08-15T12:00:00Z",
                    "latest_version": "mc1.21.1-0.6.0",
                    "license": "LGPL-3.0-only",
                    "client_side": "required",
                    "server_side": "unsupported",
                    "gallery": ["https://cdn.modrinth.com/data/AANobbMI/gallery.png"],
                    "featured_gallery": null,
                    "color": 3845695
                },
                {
                    "project_id": "MINIMAL1",
                    "project_type": "resourcepack",
                    "slug": "minimal-pack",
                    "author": "tester",
                    "title": "Minimal Pack",
                    "description": "Minimal fields to verify serde defaults",
                    "downloads": 12,
                    "follows": 3,
                    "date_created": "2025-01-01T00:00:00Z",
                    "date_modified": "2025-01-02T00:00:00Z"
                }
            ],
            "offset": 0,
            "limit": 20,
            "total_hits": 2
        }"#;

        let response: SearchResponse =
            serde_json::from_str(sample_json).expect("deserialize SearchResponse");
        assert_eq!(response.offset, 0);
        assert_eq!(response.limit, 20);
        assert_eq!(response.total_hits, 2);
        assert_eq!(response.hits.len(), 2);

        let first = &response.hits[0];
        assert_eq!(first.project_id, "AANobbMI");
        assert_eq!(first.project_type, ProjectType::Mod);
        assert_eq!(first.slug, "sodium");
        assert_eq!(first.downloads, 45_000_000);
        assert_eq!(first.client_side, SideSupport::Required);
        assert_eq!(first.server_side, SideSupport::Unsupported);
        assert_eq!(first.color, Some(3_845_695));
        assert_eq!(first.featured_gallery, None);

        let minimal = &response.hits[1];
        assert_eq!(minimal.project_id, "MINIMAL1");
        assert_eq!(minimal.project_type, ProjectType::ResourcePack);
        assert!(minimal.categories.is_empty());
        assert!(minimal.versions.is_empty());
        assert_eq!(minimal.icon_url, None);
        assert_eq!(minimal.latest_version, None);
        assert_eq!(minimal.license, "");
        assert_eq!(minimal.client_side, SideSupport::Unknown);
        assert_eq!(minimal.server_side, SideSupport::Unknown);
        assert!(minimal.gallery.is_empty());
        assert_eq!(minimal.color, None);

        let temp = TempDirGuard::new("search_cache");
        let params = SearchParams {
            query: "sodium".to_string(),
            project_type: ProjectType::Mod,
            game_version: Some("1.20.1".to_string()),
            loader: Some("fabric".to_string()),
            categories: Vec::new(),
            index: SearchIndex::Relevance,
            offset: 0,
            limit: 20,
        };
        let upstream_url = params.build_url(MODRINTH_API_BASE_URL);
        let cache_file = temp
            .path()
            .join(crate::cache::cache_key_for_url(&upstream_url));
        let cached =
            crate::cache::Cached::new(response.clone(), DEFAULT_SEARCH_TTL).expect("create cache");
        cached.save_to_file(&cache_file).expect("save search cache");

        let client = reqwest::Client::new();
        let loaded = SearchResponse::fetch_with_cache(
            &client,
            &params,
            temp.path(),
            DownloadSource::Official,
            false,
        )
        .await
        .expect("should load SearchResponse from local cache");
        assert_eq!(loaded, response);
    }

    #[test]
    fn test_project_version_primary_file() {
        let base_version = |files: Vec<VersionFile>| ProjectVersion {
            id: "ver1".to_string(),
            project_id: "proj1".to_string(),
            name: "1.0.0".to_string(),
            version_number: "1.0.0".to_string(),
            changelog: None,
            dependencies: Vec::new(),
            game_versions: vec!["1.20.1".to_string()],
            version_type: ReleaseType::Release,
            loaders: vec!["fabric".to_string()],
            featured: true,
            date_published: "2024-01-01T00:00:00Z".to_string(),
            downloads: 100,
            files,
        };

        let empty = base_version(Vec::new());
        assert_eq!(empty.primary_file(), None);

        let sources_file = VersionFile {
            hashes: FileHashes::default(),
            url: "https://cdn.modrinth.com/mod-sources.jar".to_string(),
            filename: "mod-sources.jar".to_string(),
            primary: false,
            size: 1024,
        };
        let main_file = VersionFile {
            hashes: FileHashes {
                sha1: Some("abc".to_string()),
                sha512: None,
            },
            url: "https://cdn.modrinth.com/mod.jar".to_string(),
            filename: "mod.jar".to_string(),
            primary: true,
            size: 2048,
        };
        let secondary_marked = base_version(vec![sources_file.clone(), main_file.clone()]);
        assert_eq!(secondary_marked.primary_file(), Some(&main_file));

        let fallback_file = VersionFile {
            hashes: FileHashes::default(),
            url: "https://cdn.modrinth.com/other.jar".to_string(),
            filename: "other.jar".to_string(),
            primary: false,
            size: 512,
        };
        let none_primary = base_version(vec![sources_file.clone(), fallback_file]);
        assert_eq!(none_primary.primary_file(), Some(&sources_file));
    }

    #[test]
    fn test_project_version_list_url() {
        let no_params =
            ProjectVersion::list_url("https://api.modrinth.com/v2/", "sodium", &[], &[]);
        assert_eq!(
            no_params,
            "https://api.modrinth.com/v2/project/sodium/version"
        );

        let loaders_only =
            ProjectVersion::list_url(MODRINTH_API_BASE_URL, "AANobbMI", &["fabric", "quilt"], &[]);
        let parsed_loaders = url::Url::parse(&loaders_only).expect("parse loaders url");
        let loaders_pairs: std::collections::HashMap<String, String> =
            parsed_loaders.query_pairs().into_owned().collect();
        assert_eq!(
            loaders_pairs.get("loaders").map(String::as_str),
            Some(r#"["fabric","quilt"]"#)
        );
        assert!(!loaders_pairs.contains_key("game_versions"));

        let versions_only =
            ProjectVersion::list_url(MODRINTH_API_BASE_URL, "sodium", &[], &["1.20.1"]);
        let parsed_versions = url::Url::parse(&versions_only).expect("parse versions url");
        let versions_pairs: std::collections::HashMap<String, String> =
            parsed_versions.query_pairs().into_owned().collect();
        assert!(!versions_pairs.contains_key("loaders"));
        assert_eq!(
            versions_pairs.get("game_versions").map(String::as_str),
            Some(r#"["1.20.1"]"#)
        );

        let both = ProjectVersion::list_url(
            "https://api.modrinth.com/v2/",
            "sodium",
            &["fabric"],
            &["1.20.1", "1.21.1"],
        );
        assert!(both.starts_with("https://api.modrinth.com/v2/project/sodium/version?"));
        let parsed_both = url::Url::parse(&both).expect("parse both url");
        let both_pairs: std::collections::HashMap<String, String> =
            parsed_both.query_pairs().into_owned().collect();
        assert_eq!(
            both_pairs.get("loaders").map(String::as_str),
            Some(r#"["fabric"]"#)
        );
        assert_eq!(
            both_pairs.get("game_versions").map(String::as_str),
            Some(r#"["1.20.1","1.21.1"]"#)
        );
    }

    #[tokio::test]
    async fn test_project_version_deserialization_and_fetch_list_with_cache() {
        let sample_json = r#"[
            {
                "id": "b4h3K9xL",
                "project_id": "AANobbMI",
                "name": "Sodium 0.5.11 for Fabric 1.20.1",
                "version_number": "mc1.20.1-0.5.11",
                "changelog": "Fixed rendering bug",
                "dependencies": [
                    {
                        "version_id": null,
                        "project_id": "P7dR8mSH",
                        "file_name": null,
                        "dependency_type": "required"
                    },
                    {
                        "version_id": "depVer1",
                        "project_id": "optProj",
                        "file_name": "optional.jar",
                        "dependency_type": "optional"
                    }
                ],
                "game_versions": ["1.20.1"],
                "version_type": "beta",
                "loaders": ["fabric", "quilt"],
                "featured": true,
                "date_published": "2024-07-02T18:10:00Z",
                "downloads": 5120300,
                "files": [
                    {
                        "hashes": {
                            "sha512": "sha512hex",
                            "sha1": "9f8e7d6c5b4a32109f8e7d6c5b4a32109f8e7d6c"
                        },
                        "url": "https://cdn.modrinth.com/data/AANobbMI/versions/b4h3K9xL/sodium.jar",
                        "filename": "sodium.jar",
                        "primary": true,
                        "size": 945120
                    }
                ]
            }
        ]"#;

        let versions: Vec<ProjectVersion> =
            serde_json::from_str(sample_json).expect("deserialize ProjectVersion list");
        assert_eq!(versions.len(), 1);
        let ver = &versions[0];
        assert_eq!(ver.id, "b4h3K9xL");
        assert_eq!(ver.project_id, "AANobbMI");
        assert_eq!(ver.version_number, "mc1.20.1-0.5.11");
        assert_eq!(ver.version_type, ReleaseType::Beta);
        assert_eq!(ver.changelog.as_deref(), Some("Fixed rendering bug"));
        assert!(ver.featured);
        assert_eq!(ver.dependencies.len(), 2);
        assert_eq!(
            ver.dependencies[0].dependency_type,
            DependencyType::Required
        );
        assert_eq!(ver.dependencies[0].project_id.as_deref(), Some("P7dR8mSH"));
        assert_eq!(ver.dependencies[0].version_id, None);
        assert_eq!(
            ver.dependencies[1].dependency_type,
            DependencyType::Optional
        );
        assert_eq!(
            ver.dependencies[1].file_name.as_deref(),
            Some("optional.jar")
        );

        let primary = ver.primary_file().expect("primary file present");
        assert_eq!(primary.filename, "sodium.jar");
        assert_eq!(
            primary.hashes.sha1.as_deref(),
            Some("9f8e7d6c5b4a32109f8e7d6c5b4a32109f8e7d6c")
        );
        assert_eq!(primary.hashes.sha512.as_deref(), Some("sha512hex"));

        let temp = TempDirGuard::new("version_list_cache");
        let upstream_url =
            ProjectVersion::list_url(MODRINTH_API_BASE_URL, "AANobbMI", &["fabric"], &["1.20.1"]);
        let cache_file = temp
            .path()
            .join(crate::cache::cache_key_for_url(&upstream_url));
        let cached =
            crate::cache::Cached::new(versions.clone(), DEFAULT_SEARCH_TTL).expect("create cache");
        cached
            .save_to_file(&cache_file)
            .expect("save version list cache");

        let client = reqwest::Client::new();
        let loaded = ProjectVersion::fetch_list_with_cache(
            &client,
            "AANobbMI",
            &["fabric"],
            &["1.20.1"],
            temp.path(),
            DownloadSource::Official,
            false,
        )
        .await
        .expect("should load ProjectVersion list from local cache");
        assert_eq!(loaded, versions);
    }

    #[test]
    fn test_asset_item_data_from_search_hit() {
        let make_hit = |categories: Vec<&str>, display_categories: Vec<&str>| SearchHit {
            project_id: "AANobbMI".to_string(),
            project_type: ProjectType::Mod,
            slug: "sodium".to_string(),
            author: "jellysquid3".to_string(),
            title: "Sodium".to_string(),
            description: "Fastest rendering optimization mod".to_string(),
            categories: categories.into_iter().map(str::to_string).collect(),
            display_categories: display_categories.into_iter().map(str::to_string).collect(),
            versions: vec!["1.21.1".to_string()],
            downloads: 45_000_000,
            follows: 120_000,
            icon_url: Some("https://cdn.modrinth.com/data/AANobbMI/icon.png".to_string()),
            date_created: "2020-07-09T03:02:55Z".to_string(),
            date_modified: "2024-08-15T12:00:00Z".to_string(),
            latest_version: Some("mc1.21.1-0.6.0".to_string()),
            license: "LGPL-3.0-only".to_string(),
            client_side: SideSupport::Required,
            server_side: SideSupport::Unsupported,
            gallery: Vec::new(),
            featured_gallery: None,
            color: None,
        };

        let with_display = AssetItemData::from(make_hit(
            vec!["fabric", "optimization"],
            vec!["optimization"],
        ));
        assert_eq!(with_display.host, ProjectHost::Modrinth);
        assert_eq!(with_display.project_type, ProjectType::Mod);
        assert_eq!(with_display.project_id, "AANobbMI");
        assert_eq!(with_display.title, "Sodium");
        assert_eq!(with_display.author, "jellysquid3");
        assert_eq!(
            with_display.description,
            "Fastest rendering optimization mod"
        );
        assert_eq!(
            with_display.icon_url.as_deref(),
            Some("https://cdn.modrinth.com/data/AANobbMI/icon.png")
        );
        assert_eq!(with_display.downloads, 45_000_000);
        assert_eq!(with_display.display_categories, vec!["optimization"]);
        assert_eq!(
            with_display.latest_version.as_deref(),
            Some("mc1.21.1-0.6.0")
        );

        let fallback = AssetItemData::from(make_hit(vec!["fabric", "optimization"], Vec::new()));
        assert_eq!(fallback.display_categories, vec!["fabric", "optimization"]);
    }

    #[test]
    fn test_search_params_from_asset_filter_state() {
        let with_category = AssetFilterState {
            host: ProjectHost::Modrinth,
            project_type: ProjectType::Modpack,
            query: "create".to_string(),
            game_version: Some("1.21.1".to_string()),
            loader: Some("fabric".to_string()),
            category: Some("adventure".to_string()),
            sort_by: SearchIndex::Downloads,
            offset: 20,
            limit: 20,
        };
        let params_with_category = SearchParams::from(&with_category);
        assert_eq!(
            params_with_category,
            SearchParams {
                query: "create".to_string(),
                project_type: ProjectType::Modpack,
                game_version: Some("1.21.1".to_string()),
                loader: Some("fabric".to_string()),
                categories: vec!["adventure".to_string()],
                index: SearchIndex::Downloads,
                offset: 20,
                limit: 20,
            }
        );

        let without_category = AssetFilterState {
            category: None,
            ..with_category
        };
        let params_without_category = SearchParams::from(&without_category);
        assert!(params_without_category.categories.is_empty());
    }

    #[test]
    fn test_asset_version_from_project_version() {
        let secondary = VersionFile {
            hashes: FileHashes::default(),
            url: "https://cdn.modrinth.com/mod-sources.jar".to_string(),
            filename: "mod-sources.jar".to_string(),
            primary: false,
            size: 512,
        };
        let main = VersionFile {
            hashes: FileHashes {
                sha1: Some("deadbeef".to_string()),
                sha512: Some("cafebabe".to_string()),
            },
            url: "https://cdn.modrinth.com/mod.jar".to_string(),
            filename: "mod.jar".to_string(),
            primary: true,
            size: 2048,
        };

        let pv = ProjectVersion {
            id: "ver123".to_string(),
            project_id: "AANobbMI".to_string(),
            name: "Sodium 0.5.8".to_string(),
            version_number: "mc1.20.1-0.5.8".to_string(),
            changelog: Some("Fix bug".to_string()),
            dependencies: vec![
                VersionDependency {
                    version_id: Some("depVer".to_string()),
                    project_id: Some("P7dR8mSH".to_string()),
                    file_name: None,
                    dependency_type: DependencyType::Required,
                },
                VersionDependency {
                    version_id: None,
                    project_id: None,
                    file_name: Some("orphan.jar".to_string()),
                    dependency_type: DependencyType::Optional,
                },
            ],
            game_versions: vec!["1.20.1".to_string()],
            version_type: ReleaseType::Beta,
            loaders: vec!["fabric".to_string()],
            featured: true,
            date_published: "2024-05-01T00:00:00Z".to_string(),
            downloads: 999,
            files: vec![secondary, main],
        };

        let av = AssetVersion::from(pv);
        assert_eq!(av.host, ProjectHost::Modrinth);
        assert_eq!(av.id, "ver123");
        assert_eq!(av.project_id, "AANobbMI");
        assert_eq!(av.name, "Sodium 0.5.8");
        assert_eq!(av.version_number, "mc1.20.1-0.5.8");
        assert_eq!(av.release_type, ReleaseType::Beta);
        assert_eq!(av.game_versions, vec!["1.20.1"]);
        assert_eq!(av.loaders, vec!["fabric"]);
        assert_eq!(av.date_published, "2024-05-01T00:00:00Z");
        assert_eq!(av.downloads, 999);
        assert_eq!(
            av.primary_file,
            Some(AssetFile {
                url: "https://cdn.modrinth.com/mod.jar".to_string(),
                filename: "mod.jar".to_string(),
                size: 2048,
                sha1: Some("deadbeef".to_string()),
                sha512: Some("cafebabe".to_string()),
            })
        );
        assert_eq!(
            av.dependencies,
            vec![AssetDependency {
                host: ProjectHost::Modrinth,
                project_id: "P7dR8mSH".to_string(),
                version_id: Some("depVer".to_string()),
                dependency_type: DependencyType::Required,
            }]
        );
    }

    #[tokio::test]
    async fn test_project_detail_batch_url_from_and_fetch_cache() {
        let ids = vec!["AANobbMI".to_string(), "P7dR8mSH".to_string()];
        let batch_url = ProjectDetail::batch_url("https://api.modrinth.com/v2/", &ids);
        assert!(batch_url.starts_with("https://api.modrinth.com/v2/projects?"));
        let parsed = url::Url::parse(&batch_url).expect("parse batch url");
        let pairs: std::collections::HashMap<String, String> =
            parsed.query_pairs().into_owned().collect();
        assert_eq!(
            pairs.get("ids").map(String::as_str),
            Some(r#"["AANobbMI","P7dR8mSH"]"#)
        );

        let details = vec![ProjectDetail {
            id: "P7dR8mSH".to_string(),
            project_type: ProjectType::Mod,
            slug: "fabric-api".to_string(),
            title: "Fabric API".to_string(),
            description: "Core hooks".to_string(),
            categories: vec!["library".to_string()],
            icon_url: Some("https://cdn.modrinth.com/data/P7dR8mSH/icon.png".to_string()),
            downloads: 80_000_000,
        }];

        let item = AssetItemData::from(details[0].clone());
        assert_eq!(item.host, ProjectHost::Modrinth);
        assert_eq!(item.project_type, ProjectType::Mod);
        assert_eq!(item.project_id, "P7dR8mSH");
        assert_eq!(item.title, "Fabric API");
        assert_eq!(item.author, "");
        assert_eq!(item.description, "Core hooks");
        assert_eq!(
            item.icon_url.as_deref(),
            Some("https://cdn.modrinth.com/data/P7dR8mSH/icon.png")
        );
        assert_eq!(item.downloads, 80_000_000);
        assert_eq!(item.display_categories, vec!["library"]);
        assert_eq!(item.latest_version, None);
        assert_eq!(
            item.website_url.as_deref(),
            Some("https://modrinth.com/mod/fabric-api")
        );

        let temp = TempDirGuard::new("project_detail_cache");
        let client = reqwest::Client::new();

        // Empty slice returns Ok(vec![]) immediately without hitting network or cache.
        let empty = ProjectDetail::fetch_batch_with_cache(
            &client,
            &[],
            temp.path(),
            DownloadSource::Official,
            false,
        )
        .await
        .expect("empty ids should succeed");
        assert!(empty.is_empty());

        let upstream_url = ProjectDetail::batch_url(MODRINTH_API_BASE_URL, &ids);
        let cache_file = temp
            .path()
            .join(crate::cache::cache_key_for_url(&upstream_url));
        let cached =
            crate::cache::Cached::new(details.clone(), DEFAULT_SEARCH_TTL).expect("create cache");
        cached
            .save_to_file(&cache_file)
            .expect("save project detail cache");

        let loaded = ProjectDetail::fetch_batch_with_cache(
            &client,
            &ids,
            temp.path(),
            DownloadSource::Official,
            false,
        )
        .await
        .expect("should load ProjectDetail list from local cache");
        assert_eq!(loaded, details);
    }

    #[test]
    fn test_modrinth_website_url() {
        assert_eq!(
            modrinth_website_url(ProjectType::Mod, "sodium", "AANobbMI").as_deref(),
            Some("https://modrinth.com/mod/sodium")
        );
        assert_eq!(
            modrinth_website_url(ProjectType::Modpack, "fabulously-optimized", "1KVo5zza")
                .as_deref(),
            Some("https://modrinth.com/modpack/fabulously-optimized")
        );
        assert_eq!(
            modrinth_website_url(ProjectType::ResourcePack, "  ", "pack123").as_deref(),
            Some("https://modrinth.com/resourcepack/pack123")
        );
        assert_eq!(
            modrinth_website_url(ProjectType::Shader, "bsl-shaders", "shader1").as_deref(),
            Some("https://modrinth.com/shader/bsl-shaders")
        );
        assert_eq!(modrinth_website_url(ProjectType::Mod, "  ", ""), None);
    }

    #[test]
    fn test_default_search_ttl() {
        assert_eq!(DEFAULT_SEARCH_TTL, Duration::from_secs(900));
    }
}
