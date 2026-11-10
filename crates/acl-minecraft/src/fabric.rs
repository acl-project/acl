// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::Path;

use crate::{
    DEFAULT_MANIFEST_TTL, DownloadSource, Result, VersionPatch,
    cache::{fetch_cached_json, fetch_verified_json},
};
use serde::{Deserialize, Serialize};
pub const FABRIC_META_BASE_URL: &str = "https://meta.fabricmc.net";

/// Information about a single Fabric Loader version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FabricLoaderInfo {
    pub separator: String,
    pub build: u32,
    pub maven: String,
    pub version: String,
    pub stable: bool,
}

/// Information about the intermediary mappings for a Minecraft version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FabricIntermediaryInfo {
    pub maven: String,
    pub version: String,
    pub stable: bool,
}

/// A single entry returned by `/v2/versions/loader/{game_version}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FabricLoaderEntry {
    pub loader: FabricLoaderInfo,
    pub intermediary: FabricIntermediaryInfo,
}

impl FabricLoaderEntry {
    /// Construct the upstream URL for listing Fabric loaders of a specific Minecraft version.
    pub fn list_url(base_url: &str, game_version: &str) -> String {
        format!(
            "{}/v2/versions/loader/{game_version}",
            base_url.trim_end_matches('/')
        )
    }

    /// Construct the upstream URL for fetching the launch profile (`VersionPatch`) JSON.
    pub fn profile_url(base_url: &str, game_version: &str, loader_version: &str) -> String {
        format!(
            "{}/v2/versions/loader/{game_version}/{loader_version}/profile/json",
            base_url.trim_end_matches('/')
        )
    }

    /// Fetch available Fabric loader entries for `game_version` with caching using default TTL.
    pub async fn fetch_list_with_cache(
        client: &reqwest::Client,
        game_version: &str,
        cache_dir: &Path,
        download_source: DownloadSource,
    ) -> Result<Vec<Self>> {
        let upstream_url = Self::list_url(FABRIC_META_BASE_URL, game_version);
        fetch_cached_json(
            client,
            &upstream_url,
            cache_dir,
            DEFAULT_MANIFEST_TTL,
            download_source,
        )
        .await
    }

    /// Fetch the Fabric profile (`VersionPatch`) for the given Minecraft and loader version.
    /// If `loader_json_path` is already present locally, loads it directly; otherwise fetches from network and saves it.
    pub async fn fetch_profile(
        client: &reqwest::Client,
        game_version: &str,
        loader_version: &str,
        loader_json_path: &Path,
        download_source: DownloadSource,
    ) -> Result<VersionPatch> {
        let upstream_url = Self::profile_url(FABRIC_META_BASE_URL, game_version, loader_version);
        fetch_verified_json(
            client,
            &download_source.rewrite_url(&upstream_url),
            "", // Fabric profile has no sha1; pass an empty string to skip validation directly.
            &format!("fabric-loader-{loader_version}-{game_version}"),
            loader_json_path,
        )
        .await
    }
}
