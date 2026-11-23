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

pub async fn fetch_mods_batch_with_cache(
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
