// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{path::Path, time::Duration};

use serde::{Deserialize, Serialize};

use crate::{
    AssetDependency, AssetFile, AssetFilterState, AssetItemData, AssetVersion, DependencyType,
    MinecraftError, ProjectHost, ProjectType, ReleaseType, Result, SearchIndex,
    cache::fetch_cached_request,
};

pub const CURSEFORGE_API_BASE_URL: &str = "https://api.curseforge.com/v1";
pub const MINECRAFT_GAME_ID: u32 = 432;
pub const DEFAULT_SEARCH_TTL: Duration = Duration::from_secs(300);
pub const CURSEFORGE_API_KEY: Option<&'static str> = option_env!("CURSEFORGE_API_KEY");

/// Retrieves the value directly from the environment variable if set.
pub fn api_key() -> Option<String> {
    std::env::var("CURSEFORGE_API_KEY")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|k| !k.is_empty())
        .or_else(|| {
            CURSEFORGE_API_KEY
                .map(str::trim)
                .filter(|k| !k.is_empty())
                .map(str::to_string)
        })
}

fn require_api_key() -> Result<String> {
    api_key().ok_or(MinecraftError::CurseForgeApiKeyMissing)
}

// Oh look, a magic number. The world is saved.
pub fn mod_loader_type(loader: &str) -> Option<u32> {
    match loader.trim().to_ascii_lowercase().as_str() {
        "forge" => Some(1),
        "fabric" => Some(4),
        "quilt" => Some(5),
        "neoforge" => Some(6),
        _ => None,
    }
}

/// Search parameters for CurseForge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchParams {
    pub query: String,
    pub project_type: ProjectType,
    pub game_version: Option<String>,
    pub loader: Option<String>,
    pub category_id: Option<u32>,
    pub sort_by: SearchIndex,
    pub offset: u32,
    pub limit: u32,
}

impl SearchParams {
    pub fn build_url(&self, base_url: &str) -> String {
        let mut serializer = url::form_urlencoded::Serializer::new(String::new());

        // Fixed magic number, do not modify — this is a Minecraft launcher.
        serializer.append_pair("gameId", &MINECRAFT_GAME_ID.to_string());
        serializer.append_pair(
            "classId",
            &self.project_type.curseforge_class_id().to_string(),
        );

        let query = self.query.trim();
        if !query.is_empty() {
            serializer.append_pair("searchFilter", query);
        }

        if let Some(version) = self.game_version.as_deref().map(str::trim)
            && !version.is_empty()
        {
            serializer.append_pair("gameVersion", version);
        }

        if matches!(self.project_type, ProjectType::Mod | ProjectType::Modpack)
            && let Some(loader) = self.loader.as_deref()
            && let Some(loader_id) = mod_loader_type(loader)
        {
            serializer.append_pair("modLoaderType", &loader_id.to_string());
        }

        if let Some(category_id) = self.category_id {
            serializer.append_pair("categoryId", &category_id.to_string());
        }

        serializer.append_pair(
            "sortField",
            &self.sort_by.curseforge_sort_field().to_string(),
        );

        // Since you didn't bother setting it, I'll generously flip everything upside down for you.
        serializer.append_pair("sortOrder", "desc");

        let page_size = self.limit.clamp(1, 50);
        let index = self.offset.min(10_000_u32.saturating_sub(page_size));
        serializer.append_pair("index", &index.to_string());
        serializer.append_pair("pageSize", &page_size.to_string());

        format!(
            "{}/mods/search?{}",
            base_url.trim_end_matches('/'),
            serializer.finish()
        )
    }

    pub fn url(&self) -> String {
        self.build_url(CURSEFORGE_API_BASE_URL)
    }
}

impl From<&AssetFilterState> for SearchParams {
    fn from(state: &AssetFilterState) -> Self {
        Self {
            query: state.query.clone(),
            project_type: state.project_type,
            game_version: state.game_version.clone(),
            loader: state.loader.clone(),
            category_id: state
                .category
                .as_deref()
                .and_then(|c| c.trim().parse::<u32>().ok()),
            sort_by: state.sort_by,
            offset: state.offset,
            limit: state.limit,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pagination {
    pub index: u32,
    pub page_size: u32,
    pub result_count: u32,
    pub total_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModLogo {
    #[serde(default)]
    pub thumbnail_url: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModAuthor {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModCategory {
    pub id: u32,
    pub name: String,
    #[serde(default)]
    pub slug: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModFileIndex {
    #[serde(default)]
    pub game_version: Option<String>,
    pub file_id: u64,
    #[serde(default)]
    pub filename: String,
    #[serde(default)]
    pub release_type: u32,
    #[serde(default)]
    pub mod_loader: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModInfo {
    pub id: u64,
    #[serde(default)]
    pub class_id: Option<u32>,
    pub name: String,
    #[serde(default)]
    pub slug: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub download_count: u64,
    #[serde(default)]
    pub logo: Option<ModLogo>,
    #[serde(default)]
    pub authors: Vec<ModAuthor>,
    #[serde(default)]
    pub categories: Vec<ModCategory>,
    #[serde(default)]
    pub latest_files_indexes: Vec<ModFileIndex>,
    #[serde(default)]
    pub allow_mod_distribution: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchResponse {
    #[serde(default)]
    pub data: Vec<ModInfo>,
    pub pagination: Pagination,
}

impl SearchResponse {
    pub async fn fetch_with_cache(
        client: &reqwest::Client,
        params: &SearchParams,
        cache_dir: &Path,
        force_refresh: bool,
    ) -> Result<Self> {
        let api_key = require_api_key()?;
        let url = params.url();
        let req = client
            .get(&url)
            .header("x-api-key", &api_key)
            .header(reqwest::header::ACCEPT, "application/json");
        fetch_cached_request(req, &url, cache_dir, DEFAULT_SEARCH_TTL, force_refresh).await
    }
}

impl From<ModInfo> for AssetItemData {
    fn from(m: ModInfo) -> Self {
        let author = m
            .authors
            .into_iter()
            .next()
            .map(|a| a.name)
            .unwrap_or_default();
        let icon_url = m.logo.and_then(|l| {
            l.thumbnail_url
                .filter(|u| !u.trim().is_empty())
                .or_else(|| l.url.filter(|u| !u.trim().is_empty()))
        });

        let mut display_categories = Vec::new();

        for cat in m.categories {
            if !cat.name.is_empty() && !display_categories.contains(&cat.name) {
                display_categories.push(cat.name);
            }
        }

        let latest_version = m
            .latest_files_indexes
            .into_iter()
            .find_map(|f| f.game_version.filter(|v| !v.trim().is_empty()));

        Self {
            host: ProjectHost::CurseForge,
            project_type: ProjectType::from_curseforge_class_id(m.class_id),
            project_id: m.id.to_string(),
            title: m.name,
            author,
            description: m.summary,
            icon_url,
            downloads: m.download_count,
            display_categories,
            latest_version,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileHash {
    pub value: String,
    /// 1 = SHA-1, 2 = MD5
    pub algo: u32, // I'm speechless.
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDependency {
    pub mod_id: u64,
    /// 1 = EmbeddedLibrary, 2 = Optional, 3 = Required, 4 = Tool, 5 = Incompatible,  6 = Include
    pub relation_type: u32, // I'm speechless.
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModFile {
    pub id: u64,
    pub mod_id: u64,
    pub display_name: String,
    pub file_name: String,
    /// 1 = Release, 2 = Beta, 3 = Alpha
    #[serde(default)]
    pub release_type: u32, // I'm speechless.
    #[serde(default)]
    pub file_date: String,
    #[serde(default)]
    pub file_length: u64,
    #[serde(default)]
    pub download_count: u64,
    #[serde(default)]
    pub download_url: Option<String>,
    #[serde(default)]
    pub game_versions: Vec<String>,
    #[serde(default)]
    pub hashes: Vec<FileHash>,
    #[serde(default)]
    pub dependencies: Vec<FileDependency>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModFilesResponse {
    #[serde(default)]
    pub data: Vec<ModFile>,
    pub pagination: Pagination,
}

pub fn parse_project_id(id: &str) -> Result<u64> {
    id.trim()
        .parse::<u64>()
        .map_err(|_| MinecraftError::CurseForgeInvalidId { id: id.to_string() })
}

impl ModFile {
    pub fn sha1(&self) -> Option<String> {
        self.hashes
            .iter()
            .find(|h| h.algo == 1 /* I will eat my hat if anyone remembers what 1 is */ && !h.value.trim().is_empty())
            .map(|h| h.value.clone())
    }

    pub fn list_url(
        base_url: &str,
        mod_id: u64,
        game_version: Option<&str>,
        loader: Option<&str>,
        index: u32,
        page_size: u32,
    ) -> String {
        let mut serializer = url::form_urlencoded::Serializer::new(String::new());

        if let Some(version) = game_version.map(str::trim)
            && !version.is_empty()
        {
            serializer.append_pair("gameVersion", version);
        }

        if let Some(loader) = loader
            && let Some(loader_id) = mod_loader_type(loader)
        {
            serializer.append_pair("modLoaderType", &loader_id.to_string());
        }

        // Yes, we have to paginate a single mod's version list. No, I don't know why either.
        let page_size = page_size.clamp(1, 50);
        let index = index.min(10_000_u32.saturating_sub(page_size));
        serializer.append_pair("index", &index.to_string());
        serializer.append_pair("pageSize", &page_size.to_string());

        format!(
            "{}/mods/{mod_id}/files?{}",
            base_url.trim_end_matches('/'),
            serializer.finish()
        )
    }

    const FILES_PAGE_SIZE: u32 = 50;
    const MAX_FILE_PAGES: u32 = 20;

    pub async fn fetch_list_with_cache(
        client: &reqwest::Client,
        mod_id: u64,
        game_version: Option<&str>,
        loader: Option<&str>,
        cache_dir: &Path,
        force_refresh: bool,
    ) -> Result<Vec<Self>> {
        let api_key = require_api_key()?;
        let mut all_files = Vec::new();

        for index in (0..=10_000_u32.saturating_sub(Self::FILES_PAGE_SIZE))
            .step_by(Self::FILES_PAGE_SIZE as usize)
            .take(Self::MAX_FILE_PAGES as usize)
        {
            let url = Self::list_url(
                CURSEFORGE_API_BASE_URL,
                mod_id,
                game_version,
                loader,
                index,
                Self::FILES_PAGE_SIZE,
            );
            let req = client
                .get(&url)
                .header("x-api-key", &api_key)
                .header(reqwest::header::ACCEPT, "application/json");

            let resp: ModFilesResponse =
                fetch_cached_request(req, &url, cache_dir, DEFAULT_SEARCH_TTL, force_refresh)
                    .await?;

            let count = resp.data.len();
            if count == 0 {
                break;
            }

            all_files.extend(resp.data);

            if count < Self::FILES_PAGE_SIZE as usize
                || all_files.len() >= resp.pagination.total_count as usize
            {
                break;
            }
        }

        Ok(all_files)
    }
}

fn release_type_from_cf(code: u32) -> ReleaseType {
    match code {
        2 => ReleaseType::Beta,
        3 => ReleaseType::Alpha,
        _ => ReleaseType::Release,
    }
}
fn dependency_type_from_cf(code: u32) -> Option<DependencyType> {
    match code {
        1 | 6 => Some(DependencyType::Embedded),
        2 => Some(DependencyType::Optional),
        3 => Some(DependencyType::Required),
        5 => Some(DependencyType::Incompatible),
        _ => None,
    }
}

fn is_mc_version(s: &str) -> bool {
    let starts_with_digit = s.chars().next().is_some_and(|c| c.is_ascii_digit());
    if !starts_with_digit {
        return false;
    }

    if s.ends_with("-snapshot") {
        return false;
    }

    s.contains('.')
}

// Behold, peak database normalization: shoving MC versions, mod loaders, Java versions,
// and client/server flags into a single flat string array. Truly visionary.
fn split_game_versions_and_loaders(raw: Vec<String>) -> (Vec<String>, Vec<String>) {
    let mut game_versions = Vec::new();
    let mut loaders = Vec::new();

    for entry in raw {
        let trimmed = entry.trim();
        if trimmed.is_empty() {
            continue;
        }

        let lower = trimmed.to_ascii_lowercase();
        match lower.as_str() {
            "forge" | "fabric" | "quilt" | "neoforge" => {
                if !loaders.contains(&lower) {
                    loaders.push(lower);
                }
            }
            "client" | "server" => {}
            s if is_mc_version(s) => {
                let version = trimmed.to_string();
                if !game_versions.contains(&version) {
                    game_versions.push(version);
                }
            }
            _ => {}
        }
    }

    (game_versions, loaders)
}

impl From<ModFile> for AssetVersion {
    fn from(file: ModFile) -> Self {
        let sha1 = file.sha1();
        let primary_file = file
            .download_url
            .filter(|u| !u.trim().is_empty())
            .map(|url| AssetFile {
                url,
                filename: file.file_name.clone(),
                size: file.file_length,
                sha1,
                sha512: None,
            });

        let dependencies = file
            .dependencies
            .into_iter()
            .filter_map(|d| {
                Some(AssetDependency {
                    host: ProjectHost::CurseForge,
                    project_id: d.mod_id.to_string(),
                    version_id: None,
                    dependency_type: dependency_type_from_cf(d.relation_type)?,
                })
            })
            .collect();

        let (game_versions, loaders) = split_game_versions_and_loaders(file.game_versions);

        Self {
            host: ProjectHost::CurseForge,
            id: file.id.to_string(),
            project_id: file.mod_id.to_string(),
            name: file.display_name,
            version_number: file.file_name,
            release_type: release_type_from_cf(file.release_type),
            game_versions,
            loaders,
            date_published: file.file_date,
            downloads: file.download_count,
            primary_file,
            dependencies,
        }
    }
}

const BATCH_CHUNK_SIZE: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DataResponse<T> {
    #[serde(default)]
    data: T,
}

pub async fn fetch_projects_batch_with_cache(
    client: &reqwest::Client,
    mod_ids: &[u64],
    cache_dir: &Path,
    force_refresh: bool,
) -> Result<Vec<ModInfo>> {
    if mod_ids.is_empty() {
        return Ok(Vec::new());
    }

    let api_key = require_api_key()?;
    let mut unique_ids = mod_ids.to_vec();
    unique_ids.sort_unstable();
    unique_ids.dedup();

    let url = format!("{CURSEFORGE_API_BASE_URL}/mods");
    let mut results = Vec::with_capacity(unique_ids.len());

    for chunk in unique_ids.chunks(BATCH_CHUNK_SIZE) {
        let body = serde_json::json!({"modIds": chunk});
        let cache_seed = format!("POST:{url}:{body}");
        let req = client
            .post(&url)
            .header("x-api-key", &api_key)
            .header(reqwest::header::ACCEPT, "application/json")
            .json(&body);

        let resp: DataResponse<Vec<ModInfo>> = fetch_cached_request(
            req,
            &cache_seed,
            cache_dir,
            DEFAULT_SEARCH_TTL,
            force_refresh,
        )
        .await?;

        results.extend(resp.data);
    }

    Ok(results)
}

pub async fn fetch_files_batch_with_cache(
    client: &reqwest::Client,
    file_ids: &[u64],
    cache_dir: &Path,
    force_refresh: bool,
) -> Result<Vec<ModFile>> {
    if file_ids.is_empty() {
        return Ok(Vec::new());
    }

    let api_key = require_api_key()?;
    let mut unique_ids = file_ids.to_vec();
    unique_ids.sort_unstable();
    unique_ids.dedup();

    let url = format!("{CURSEFORGE_API_BASE_URL}/mods/files");
    let mut results = Vec::with_capacity(unique_ids.len());

    for chunk in unique_ids.chunks(BATCH_CHUNK_SIZE) {
        let body = serde_json::json!({ "fileIds": chunk });
        let cache_seed = format!("POST:{url}:{body}");
        let req = client
            .post(&url)
            .header("x-api-key", &api_key)
            .header(reqwest::header::ACCEPT, "application/json")
            .json(&body);

        let resp: DataResponse<Vec<ModFile>> = fetch_cached_request(
            req,
            &cache_seed,
            cache_dir,
            DEFAULT_SEARCH_TTL,
            force_refresh,
        )
        .await?;
        results.extend(resp.data);
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    struct EnvGuard {
        key: &'static str,
        original: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, val: &str) -> Self {
            let original = std::env::var(key).ok();
            // SAFETY: Tests serialize access via ENV_LOCK to avoid concurrent env modification.
            unsafe {
                std::env::set_var(key, val);
            }
            Self { key, original }
        }

        fn remove(key: &'static str) -> Self {
            let original = std::env::var(key).ok();
            // SAFETY: Tests serialize access via ENV_LOCK to avoid concurrent env modification.
            unsafe {
                std::env::remove_var(key);
            }
            Self { key, original }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            // SAFETY: Tests serialize access via ENV_LOCK to avoid concurrent env modification.
            unsafe {
                match &self.original {
                    Some(val) => std::env::set_var(self.key, val),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }

    struct TempDirGuard {
        path: PathBuf,
    }

    impl TempDirGuard {
        fn new(name: &str) -> Self {
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let id = COUNTER.fetch_add(1, Ordering::SeqCst);
            let path = std::env::temp_dir().join(format!(
                "acl_test_curseforge_{}_{}_{}",
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

    fn sample_mod_info(id: u64, name: &str) -> ModInfo {
        ModInfo {
            id,
            class_id: Some(6),
            name: name.to_string(),
            slug: name.to_ascii_lowercase().replace(' ', "-"),
            summary: format!("Summary for {name}"),
            download_count: 1000,
            logo: Some(ModLogo {
                thumbnail_url: Some("https://media.forgecdn.net/thumb.png".to_string()),
                url: Some("https://media.forgecdn.net/full.png".to_string()),
            }),
            authors: vec![ModAuthor {
                name: "author1".to_string(),
            }],
            categories: vec![ModCategory {
                id: 406,
                name: "World Gen".to_string(),
                slug: Some("world-gen".to_string()),
            }],
            latest_files_indexes: vec![ModFileIndex {
                game_version: Some("1.21.1".to_string()),
                file_id: 100,
                filename: "mod-1.0.jar".to_string(),
                release_type: 1,
                mod_loader: Some(4),
            }],
            allow_mod_distribution: Some(true),
        }
    }

    fn sample_mod_file(id: u64, mod_id: u64) -> ModFile {
        ModFile {
            id,
            mod_id,
            display_name: format!("Mod File {id}"),
            file_name: format!("mod-{id}.jar"),
            release_type: 1,
            file_date: "2026-01-01T00:00:00Z".to_string(),
            file_length: 2048,
            download_count: 500,
            download_url: Some(format!("https://edge.forgecdn.net/files/{id}/mod-{id}.jar")),
            game_versions: vec!["1.21.1".to_string(), "Fabric".to_string()],
            hashes: vec![FileHash {
                value: "abcdef0123456789".to_string(),
                algo: 1,
            }],
            dependencies: Vec::new(),
        }
    }

    #[tokio::test]
    async fn test_api_key_and_require_api_key() {
        let _lock = ENV_LOCK.lock().await;

        {
            let _guard = EnvGuard::set("CURSEFORGE_API_KEY", "  test-secret-key \n");
            assert_eq!(api_key().as_deref(), Some("test-secret-key"));
            assert_eq!(
                require_api_key().expect("api key should be present"),
                "test-secret-key"
            );
        }

        let compile_time_fallback = CURSEFORGE_API_KEY
            .filter(|k| !k.trim().is_empty())
            .map(str::to_string);

        {
            let _guard = EnvGuard::set("CURSEFORGE_API_KEY", "   \t\n ");
            assert_eq!(api_key(), compile_time_fallback);
            if compile_time_fallback.is_none() {
                assert!(matches!(
                    require_api_key(),
                    Err(MinecraftError::CurseForgeApiKeyMissing)
                ));
            }
        }

        {
            let _guard = EnvGuard::remove("CURSEFORGE_API_KEY");
            assert_eq!(api_key(), compile_time_fallback);
            if compile_time_fallback.is_none() {
                assert!(matches!(
                    require_api_key(),
                    Err(MinecraftError::CurseForgeApiKeyMissing)
                ));
            }
        }
    }

    #[test]
    fn test_mod_loader_type() {
        assert_eq!(mod_loader_type("forge"), Some(1));
        assert_eq!(mod_loader_type("Forge"), Some(1));
        assert_eq!(mod_loader_type("  FORGE  "), Some(1));
        assert_eq!(mod_loader_type("fabric"), Some(4));
        assert_eq!(mod_loader_type("Fabric"), Some(4));
        assert_eq!(mod_loader_type("quilt"), Some(5));
        assert_eq!(mod_loader_type("  Quilt\t"), Some(5));
        assert_eq!(mod_loader_type("neoforge"), Some(6));
        assert_eq!(mod_loader_type("NeoForge"), Some(6));
        assert_eq!(mod_loader_type("vanilla"), None);
        assert_eq!(mod_loader_type(""), None);
    }

    #[test]
    fn test_parse_project_id() {
        assert_eq!(parse_project_id("238222").expect("valid id"), 238_222);
        assert_eq!(
            parse_project_id("  394468 \n").expect("trimmed valid id"),
            394_468
        );

        for invalid in ["", "   ", "abc", "-1", "12.34", "99999999999999999999999"] {
            let err = parse_project_id(invalid).expect_err("invalid id should fail");
            assert!(matches!(
                err,
                MinecraftError::CurseForgeInvalidId { ref id } if id == invalid
            ));
        }
    }

    #[test]
    fn test_search_params_build_url_and_from_filter_state() {
        let minimal_state = AssetFilterState {
            host: ProjectHost::CurseForge,
            project_type: ProjectType::Mod,
            query: "   ".to_string(),
            game_version: Some("   ".to_string()),
            loader: None,
            category: Some("not-a-number".to_string()),
            sort_by: SearchIndex::Relevance,
            offset: 0,
            limit: 0,
        };
        let minimal = SearchParams::from(&minimal_state);
        assert_eq!(minimal.category_id, None);

        let minimal_url = minimal.build_url("https://api.curseforge.com/v1/");
        assert!(minimal_url.starts_with("https://api.curseforge.com/v1/mods/search?"));
        let parsed_minimal = url::Url::parse(&minimal_url).expect("parse minimal url");
        let minimal_pairs: HashMap<String, String> =
            parsed_minimal.query_pairs().into_owned().collect();
        assert_eq!(minimal_pairs.get("gameId").map(String::as_str), Some("432"));
        assert_eq!(minimal_pairs.get("classId").map(String::as_str), Some("6"));
        assert!(!minimal_pairs.contains_key("searchFilter"));
        assert!(!minimal_pairs.contains_key("gameVersion"));
        assert!(!minimal_pairs.contains_key("modLoaderType"));
        assert!(!minimal_pairs.contains_key("categoryId"));
        assert_eq!(
            minimal_pairs.get("sortField").map(String::as_str),
            Some("1")
        );
        assert_eq!(
            minimal_pairs.get("sortOrder").map(String::as_str),
            Some("desc")
        );
        assert_eq!(minimal_pairs.get("index").map(String::as_str), Some("0"));
        // limit == 0 is clamped to 1
        assert_eq!(minimal_pairs.get("pageSize").map(String::as_str), Some("1"));

        let full_state = AssetFilterState {
            host: ProjectHost::CurseForge,
            project_type: ProjectType::Modpack,
            query: "  Create & Magic  ".to_string(),
            game_version: Some(" 1.20.1 ".to_string()),
            loader: Some("NeoForge".to_string()),
            category: Some(" 4475 ".to_string()),
            sort_by: SearchIndex::Downloads,
            offset: 12_000,
            limit: 100,
        };
        let full = SearchParams::from(&full_state);
        assert_eq!(full.category_id, Some(4475));

        let full_url = full.url();
        assert!(full_url.starts_with("https://api.curseforge.com/v1/mods/search?"));
        let parsed_full = url::Url::parse(&full_url).expect("parse full url");
        let full_pairs: HashMap<String, String> = parsed_full.query_pairs().into_owned().collect();
        assert_eq!(full_pairs.get("classId").map(String::as_str), Some("4471"));
        assert_eq!(
            full_pairs.get("searchFilter").map(String::as_str),
            Some("Create & Magic")
        );
        assert_eq!(
            full_pairs.get("gameVersion").map(String::as_str),
            Some("1.20.1")
        );
        assert_eq!(
            full_pairs.get("modLoaderType").map(String::as_str),
            Some("6")
        );
        assert_eq!(
            full_pairs.get("categoryId").map(String::as_str),
            Some("4475")
        );
        assert_eq!(full_pairs.get("sortField").map(String::as_str), Some("6"));
        // limit clamped to 50 and offset clamped to 10_000 - 50 = 9950
        assert_eq!(full_pairs.get("pageSize").map(String::as_str), Some("50"));
        assert_eq!(full_pairs.get("index").map(String::as_str), Some("9950"));

        // ResourcePack and Shader must not append modLoaderType even when loader is provided
        for non_loader_type in [ProjectType::ResourcePack, ProjectType::Shader] {
            let params = SearchParams {
                query: String::new(),
                project_type: non_loader_type,
                game_version: None,
                loader: Some("fabric".to_string()),
                category_id: None,
                sort_by: SearchIndex::Updated,
                offset: 0,
                limit: 20,
            };
            let parsed = url::Url::parse(&params.url()).expect("parse non-loader url");
            let pairs: HashMap<String, String> = parsed.query_pairs().into_owned().collect();
            assert!(!pairs.contains_key("modLoaderType"));
            assert_eq!(
                pairs.get("classId").map(String::as_str),
                Some(non_loader_type.curseforge_class_id().to_string().as_str())
            );
        }
    }

    #[test]
    fn test_search_response_deserialization_and_mod_info_to_asset_item_data() {
        let json = r#"{
            "data": [
                {
                    "id": 238222,
                    "classId": 6,
                    "name": "Just Enough Items (JEI)",
                    "slug": "jei",
                    "summary": "View Items and Recipes",
                    "downloadCount": 350000000,
                    "logo": {
                        "thumbnailUrl": "  ",
                        "url": "https://media.forgecdn.net/jei.png"
                    },
                    "authors": [
                        { "name": "mezz" },
                        { "name": "coauthor" }
                    ],
                    "categories": [
                        { "id": 421, "name": "API and Library", "slug": "library-api" },
                        { "id": 422, "name": "" },
                        { "id": 423, "name": "API and Library" },
                        { "id": 424, "name": "Map and Information" }
                    ],
                    "latestFilesIndexes": [
                        { "gameVersion": "  ", "fileId": 1 },
                        { "gameVersion": "1.21.1", "fileId": 2, "filename": "jei.jar", "releaseType": 1, "modLoader": 6 }
                    ],
                    "allowModDistribution": true
                },
                {
                    "id": 999999,
                    "classId": 6552,
                    "name": "Minimal Shader"
                }
            ],
            "pagination": {
                "index": 0,
                "pageSize": 20,
                "resultCount": 2,
                "totalCount": 2
            }
        }"#;

        let resp: SearchResponse =
            serde_json::from_str(json).expect("deserialize CurseForge SearchResponse");
        assert_eq!(resp.pagination.total_count, 2);
        assert_eq!(resp.data.len(), 2);

        let first = AssetItemData::from(resp.data[0].clone());
        assert_eq!(first.host, ProjectHost::CurseForge);
        assert_eq!(first.project_type, ProjectType::Mod);
        assert_eq!(first.project_id, "238222");
        assert_eq!(first.title, "Just Enough Items (JEI)");
        assert_eq!(first.author, "mezz");
        assert_eq!(first.description, "View Items and Recipes");
        // Blank thumbnail_url falls back to logo.url
        assert_eq!(
            first.icon_url.as_deref(),
            Some("https://media.forgecdn.net/jei.png")
        );
        assert_eq!(first.downloads, 350_000_000);
        assert_eq!(
            first.display_categories,
            vec![
                "API and Library".to_string(),
                "Map and Information".to_string()
            ]
        );
        assert_eq!(first.latest_version.as_deref(), Some("1.21.1"));

        let minimal = AssetItemData::from(resp.data[1].clone());
        assert_eq!(minimal.host, ProjectHost::CurseForge);
        assert_eq!(minimal.project_type, ProjectType::Shader);
        assert_eq!(minimal.project_id, "999999");
        assert_eq!(minimal.author, "");
        assert_eq!(minimal.description, "");
        assert_eq!(minimal.icon_url, None);
        assert_eq!(minimal.downloads, 0);
        assert!(minimal.display_categories.is_empty());
        assert_eq!(minimal.latest_version, None);
    }

    #[test]
    fn test_mod_file_sha1_list_url_and_conversion_to_asset_version() {
        let json = r#"{
            "data": [
                {
                    "id": 555001,
                    "modId": 238222,
                    "displayName": "jei-1.21.1-neoforge-19.0.0.jar",
                    "fileName": "jei-19.0.0.jar",
                    "releaseType": 2,
                    "fileDate": "2026-02-01T12:00:00Z",
                    "fileLength": 123456,
                    "downloadCount": 4200,
                    "downloadUrl": "https://edge.forgecdn.net/files/555/1/jei-19.0.0.jar",
                    "gameVersions": [
                        "1.21.1",
                        "1.21.1",
                        "1.21.2-Snapshot",
                        "24w14a",
                        "Java 21",
                        "Client",
                        "Server",
                        "NeoForge",
                        "neoforge",
                        "Fabric",
                        "  "
                    ],
                    "hashes": [
                        { "value": "md5value", "algo": 2 },
                        { "value": "   ", "algo": 1 },
                        { "value": "deadbeef1234567890", "algo": 1 }
                    ],
                    "dependencies": [
                        { "modId": 10, "relationType": 1 },
                        { "modId": 20, "relationType": 2 },
                        { "modId": 30, "relationType": 3 },
                        { "modId": 40, "relationType": 4 },
                        { "modId": 50, "relationType": 5 },
                        { "modId": 60, "relationType": 6 },
                        { "modId": 70, "relationType": 99 }
                    ]
                },
                {
                    "id": 555002,
                    "modId": 238222,
                    "displayName": "Restricted Alpha",
                    "fileName": "restricted.jar",
                    "releaseType": 3,
                    "downloadUrl": "   ",
                    "hashes": [
                        { "value": "onlymd5", "algo": 2 }
                    ]
                }
            ],
            "pagination": {
                "index": 0,
                "pageSize": 50,
                "resultCount": 2,
                "totalCount": 2
            }
        }"#;

        let resp: ModFilesResponse =
            serde_json::from_str(json).expect("deserialize ModFilesResponse");
        assert_eq!(resp.data.len(), 2);

        let file1 = resp.data[0].clone();
        assert_eq!(file1.sha1().as_deref(), Some("deadbeef1234567890"));

        let version1 = AssetVersion::from(file1);
        assert_eq!(version1.host, ProjectHost::CurseForge);
        assert_eq!(version1.id, "555001");
        assert_eq!(version1.project_id, "238222");
        assert_eq!(version1.name, "jei-1.21.1-neoforge-19.0.0.jar");
        assert_eq!(version1.version_number, "jei-19.0.0.jar");
        assert_eq!(version1.release_type, ReleaseType::Beta);
        assert_eq!(version1.game_versions, vec!["1.21.1".to_string()]);
        assert_eq!(
            version1.loaders,
            vec!["neoforge".to_string(), "fabric".to_string()]
        );
        assert_eq!(version1.date_published, "2026-02-01T12:00:00Z");
        assert_eq!(version1.downloads, 4200);
        assert_eq!(
            version1.primary_file,
            Some(AssetFile {
                url: "https://edge.forgecdn.net/files/555/1/jei-19.0.0.jar".to_string(),
                filename: "jei-19.0.0.jar".to_string(),
                size: 123_456,
                sha1: Some("deadbeef1234567890".to_string()),
                sha512: None,
            })
        );
        assert_eq!(
            version1.dependencies,
            vec![
                AssetDependency {
                    host: ProjectHost::CurseForge,
                    project_id: "10".to_string(),
                    version_id: None,
                    dependency_type: DependencyType::Embedded,
                },
                AssetDependency {
                    host: ProjectHost::CurseForge,
                    project_id: "20".to_string(),
                    version_id: None,
                    dependency_type: DependencyType::Optional,
                },
                AssetDependency {
                    host: ProjectHost::CurseForge,
                    project_id: "30".to_string(),
                    version_id: None,
                    dependency_type: DependencyType::Required,
                },
                AssetDependency {
                    host: ProjectHost::CurseForge,
                    project_id: "50".to_string(),
                    version_id: None,
                    dependency_type: DependencyType::Incompatible,
                },
                AssetDependency {
                    host: ProjectHost::CurseForge,
                    project_id: "60".to_string(),
                    version_id: None,
                    dependency_type: DependencyType::Embedded,
                },
            ]
        );

        let file2 = resp.data[1].clone();
        assert_eq!(file2.sha1(), None);
        let version2 = AssetVersion::from(file2);
        assert_eq!(version2.release_type, ReleaseType::Alpha);
        assert_eq!(version2.primary_file, None);

        // Verify list_url query generation and pagination clamping
        let url = ModFile::list_url(
            "https://api.curseforge.com/v1/",
            238_222,
            Some(" 1.21.1 "),
            Some("Fabric"),
            15_000,
            200,
        );
        assert!(url.starts_with("https://api.curseforge.com/v1/mods/238222/files?"));
        let parsed = url::Url::parse(&url).expect("parse list_url");
        let pairs: HashMap<String, String> = parsed.query_pairs().into_owned().collect();
        assert_eq!(pairs.get("gameVersion").map(String::as_str), Some("1.21.1"));
        assert_eq!(pairs.get("modLoaderType").map(String::as_str), Some("4"));
        assert_eq!(pairs.get("pageSize").map(String::as_str), Some("50"));
        assert_eq!(pairs.get("index").map(String::as_str), Some("9950"));

        let bare_url = ModFile::list_url(
            "https://api.curseforge.com/v1",
            42,
            Some("  "),
            Some("unknown"),
            0,
            0,
        );
        let parsed_bare = url::Url::parse(&bare_url).expect("parse bare list_url");
        let bare_pairs: HashMap<String, String> = parsed_bare.query_pairs().into_owned().collect();
        assert!(!bare_pairs.contains_key("gameVersion"));
        assert!(!bare_pairs.contains_key("modLoaderType"));
        assert_eq!(bare_pairs.get("pageSize").map(String::as_str), Some("1"));
        assert_eq!(bare_pairs.get("index").map(String::as_str), Some("0"));
    }

    #[tokio::test]
    async fn test_search_response_fetch_with_cache() {
        let _lock = ENV_LOCK.lock().await;
        let temp = TempDirGuard::new("search_fetch");
        let client = reqwest::Client::new();

        let params = SearchParams {
            query: "sodium".to_string(),
            project_type: ProjectType::Mod,
            game_version: Some("1.21.1".to_string()),
            loader: Some("fabric".to_string()),
            category_id: None,
            sort_by: SearchIndex::Downloads,
            offset: 0,
            limit: 20,
        };

        if CURSEFORGE_API_KEY
            .filter(|k| !k.trim().is_empty())
            .is_none()
        {
            let _guard = EnvGuard::remove("CURSEFORGE_API_KEY");
            let err = SearchResponse::fetch_with_cache(&client, &params, temp.path(), false)
                .await
                .expect_err("should fail without api key");
            assert!(matches!(err, MinecraftError::CurseForgeApiKeyMissing));
        }

        let _guard = EnvGuard::set("CURSEFORGE_API_KEY", "test-key");
        let expected = SearchResponse {
            data: vec![sample_mod_info(394_468, "Sodium")],
            pagination: Pagination {
                index: 0,
                page_size: 20,
                result_count: 1,
                total_count: 1,
            },
        };

        let cache_file = temp
            .path()
            .join(crate::cache::cache_key_for_url(&params.url()));
        crate::cache::Cached::new(expected.clone(), DEFAULT_SEARCH_TTL)
            .expect("create search cache")
            .save_to_file(&cache_file)
            .expect("save search cache");

        let loaded = SearchResponse::fetch_with_cache(&client, &params, temp.path(), false)
            .await
            .expect("should load SearchResponse from local cache");
        assert_eq!(loaded, expected);
    }

    #[tokio::test]
    async fn test_mod_file_fetch_list_with_cache_pagination() {
        let _lock = ENV_LOCK.lock().await;
        let temp = TempDirGuard::new("files_pagination");
        let client = reqwest::Client::new();
        let mod_id = 238_222_u64;

        if CURSEFORGE_API_KEY
            .filter(|k| !k.trim().is_empty())
            .is_none()
        {
            let _guard = EnvGuard::remove("CURSEFORGE_API_KEY");
            let err = ModFile::fetch_list_with_cache(
                &client,
                mod_id,
                Some("1.21.1"),
                Some("fabric"),
                temp.path(),
                false,
            )
            .await
            .expect_err("should fail without api key");
            assert!(matches!(err, MinecraftError::CurseForgeApiKeyMissing));
        }

        let _guard = EnvGuard::set("CURSEFORGE_API_KEY", "test-key");

        // Pre-seed two pages: page 0 with 50 files, page 1 with 15 files (total 65)
        let page0_files: Vec<ModFile> = (1..=50).map(|id| sample_mod_file(id, mod_id)).collect();
        let page1_files: Vec<ModFile> = (51..=65).map(|id| sample_mod_file(id, mod_id)).collect();

        let page0 = ModFilesResponse {
            data: page0_files.clone(),
            pagination: Pagination {
                index: 0,
                page_size: 50,
                result_count: 50,
                total_count: 65,
            },
        };
        let page1 = ModFilesResponse {
            data: page1_files.clone(),
            pagination: Pagination {
                index: 50,
                page_size: 50,
                result_count: 15,
                total_count: 65,
            },
        };

        for (index, resp) in [(0_u32, &page0), (50_u32, &page1)] {
            let url = ModFile::list_url(
                CURSEFORGE_API_BASE_URL,
                mod_id,
                Some("1.21.1"),
                Some("fabric"),
                index,
                ModFile::FILES_PAGE_SIZE,
            );
            let cache_file = temp.path().join(crate::cache::cache_key_for_url(&url));
            crate::cache::Cached::new(resp.clone(), DEFAULT_SEARCH_TTL)
                .expect("create page cache")
                .save_to_file(&cache_file)
                .expect("save page cache");
        }

        let all = ModFile::fetch_list_with_cache(
            &client,
            mod_id,
            Some("1.21.1"),
            Some("fabric"),
            temp.path(),
            false,
        )
        .await
        .expect("should load paginated files from cache");
        assert_eq!(all.len(), 65);
        assert_eq!(all.first().map(|f| f.id), Some(1));
        assert_eq!(all.last().map(|f| f.id), Some(65));

        // Empty first page stops immediately without fetching further pages
        let empty_mod_id = 777_u64;
        let empty_url = ModFile::list_url(
            CURSEFORGE_API_BASE_URL,
            empty_mod_id,
            None,
            None,
            0,
            ModFile::FILES_PAGE_SIZE,
        );
        let empty_resp = ModFilesResponse {
            data: Vec::new(),
            pagination: Pagination {
                index: 0,
                page_size: 50,
                result_count: 0,
                total_count: 0,
            },
        };
        crate::cache::Cached::new(empty_resp, DEFAULT_SEARCH_TTL)
            .expect("create empty page cache")
            .save_to_file(
                &temp
                    .path()
                    .join(crate::cache::cache_key_for_url(&empty_url)),
            )
            .expect("save empty page cache");

        let empty_loaded =
            ModFile::fetch_list_with_cache(&client, empty_mod_id, None, None, temp.path(), false)
                .await
                .expect("empty page should return empty Vec");
        assert!(empty_loaded.is_empty());
    }

    #[tokio::test]
    async fn test_fetch_mods_batch_with_cache() {
        let _lock = ENV_LOCK.lock().await;
        let temp = TempDirGuard::new("mods_batch");
        let client = reqwest::Client::new();

        // Empty input returns empty Vec immediately even without an API key
        {
            let _guard = EnvGuard::remove("CURSEFORGE_API_KEY");
            let empty = fetch_projects_batch_with_cache(&client, &[], temp.path(), false)
                .await
                .expect("empty slice should succeed");
            assert!(empty.is_empty());

            if CURSEFORGE_API_KEY
                .filter(|k| !k.trim().is_empty())
                .is_none()
            {
                let err = fetch_projects_batch_with_cache(&client, &[1], temp.path(), false)
                    .await
                    .expect_err("non-empty batch without key should fail");
                assert!(matches!(err, MinecraftError::CurseForgeApiKeyMissing));
            }
        }

        let _guard = EnvGuard::set("CURSEFORGE_API_KEY", "test-key");

        // Build 205 unique IDs (plus duplicates and unsorted order) to verify sort, dedup, and 200-item chunking
        let mut input_ids: Vec<u64> = (1..=205).rev().collect();
        input_ids.push(1);
        input_ids.push(205);

        let url = format!("{CURSEFORGE_API_BASE_URL}/mods");
        let chunk1: Vec<u64> = (1..=200).collect();
        let chunk2: Vec<u64> = (201..=205).collect();

        for (chunk, sample_id, name) in [
            (&chunk1, 1_u64, "ChunkOneMod"),
            (&chunk2, 205_u64, "ChunkTwoMod"),
        ] {
            let body = serde_json::json!({ "modIds": chunk });
            let cache_seed = format!("POST:{url}:{body}");
            let cache_file = temp
                .path()
                .join(crate::cache::cache_key_for_url(&cache_seed));
            let resp = DataResponse {
                data: vec![sample_mod_info(sample_id, name)],
            };
            crate::cache::Cached::new(resp, DEFAULT_SEARCH_TTL)
                .expect("create mods batch cache")
                .save_to_file(&cache_file)
                .expect("save mods batch cache");
        }

        let mods = fetch_projects_batch_with_cache(&client, &input_ids, temp.path(), false)
            .await
            .expect("should load chunked mods batch from cache");
        let ids: Vec<u64> = mods.iter().map(|m| m.id).collect();
        assert_eq!(ids, vec![1, 205]);
    }

    #[tokio::test]
    async fn test_fetch_files_batch_with_cache() {
        let _lock = ENV_LOCK.lock().await;
        let temp = TempDirGuard::new("files_batch");
        let client = reqwest::Client::new();

        // Empty input returns empty Vec immediately even without an API key
        {
            let _guard = EnvGuard::remove("CURSEFORGE_API_KEY");
            let empty = fetch_files_batch_with_cache(&client, &[], temp.path(), false)
                .await
                .expect("empty slice should succeed");
            assert!(empty.is_empty());

            if CURSEFORGE_API_KEY
                .filter(|k| !k.trim().is_empty())
                .is_none()
            {
                let err = fetch_files_batch_with_cache(&client, &[10], temp.path(), false)
                    .await
                    .expect_err("non-empty batch without key should fail");
                assert!(matches!(err, MinecraftError::CurseForgeApiKeyMissing));
            }
        }

        let _guard = EnvGuard::set("CURSEFORGE_API_KEY", "test-key");

        let mut input_ids: Vec<u64> = (1001..=1203).rev().collect();
        input_ids.push(1001);
        input_ids.push(1203);

        let url = format!("{CURSEFORGE_API_BASE_URL}/mods/files");
        let chunk1: Vec<u64> = (1001..=1200).collect();
        let chunk2: Vec<u64> = (1201..=1203).collect();

        for (chunk, sample_id) in [(&chunk1, 1001_u64), (&chunk2, 1203_u64)] {
            let body = serde_json::json!({ "fileIds": chunk });
            let cache_seed = format!("POST:{url}:{body}");
            let cache_file = temp
                .path()
                .join(crate::cache::cache_key_for_url(&cache_seed));
            let resp = DataResponse {
                data: vec![sample_mod_file(sample_id, 238_222)],
            };
            crate::cache::Cached::new(resp, DEFAULT_SEARCH_TTL)
                .expect("create files batch cache")
                .save_to_file(&cache_file)
                .expect("save files batch cache");
        }

        let files = fetch_files_batch_with_cache(&client, &input_ids, temp.path(), false)
            .await
            .expect("should load chunked files batch from cache");
        let ids: Vec<u64> = files.iter().map(|f| f.id).collect();
        assert_eq!(ids, vec![1001, 1203]);
    }
}
