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
                "acl_test_fabric_{}_{}_{}",
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
    fn test_fabric_urls() {
        assert_eq!(
            FabricLoaderEntry::list_url(FABRIC_META_BASE_URL, "1.20.1"),
            "https://meta.fabricmc.net/v2/versions/loader/1.20.1"
        );
        assert_eq!(
            FabricLoaderEntry::list_url("https://meta.fabricmc.net/", "1.20.1"),
            "https://meta.fabricmc.net/v2/versions/loader/1.20.1"
        );

        assert_eq!(
            FabricLoaderEntry::profile_url(FABRIC_META_BASE_URL, "1.20.1", "0.16.10"),
            "https://meta.fabricmc.net/v2/versions/loader/1.20.1/0.16.10/profile/json"
        );
        assert_eq!(
            FabricLoaderEntry::profile_url("https://meta.fabricmc.net/", "1.20.1", "0.16.10"),
            "https://meta.fabricmc.net/v2/versions/loader/1.20.1/0.16.10/profile/json"
        );
    }

    #[test]
    fn test_fabric_loader_entry_deserialization() {
        let sample_json = r#"[
            {
                "loader": {
                    "separator": ".",
                    "build": 1610,
                    "maven": "net.fabricmc:fabric-loader:0.16.10",
                    "version": "0.16.10",
                    "stable": true
                },
                "intermediary": {
                    "maven": "net.fabricmc:intermediary:1.20.1",
                    "version": "1.20.1",
                    "stable": true
                }
            }
        ]"#;

        let entries: Vec<FabricLoaderEntry> =
            serde_json::from_str(sample_json).expect("failed to deserialize FabricLoaderEntry");
        assert_eq!(entries.len(), 1);

        let entry = &entries[0];
        assert_eq!(entry.loader.version, "0.16.10");
        assert_eq!(entry.loader.build, 1610);
        assert_eq!(entry.loader.separator, ".");
        assert_eq!(entry.loader.maven, "net.fabricmc:fabric-loader:0.16.10");
        assert!(entry.loader.stable);

        assert_eq!(entry.intermediary.version, "1.20.1");
        assert_eq!(entry.intermediary.maven, "net.fabricmc:intermediary:1.20.1");
        assert!(entry.intermediary.stable);
    }

    #[tokio::test]
    async fn test_fetch_profile_from_local_cache() {
        let temp = TempDirGuard::new("fabric_profile_cached");
        let loader_json_path = temp.path().join("loader.json");

        let patch_json = r#"{
            "id": "fabric-loader-0.16.10-1.20.1",
            "inheritsFrom": "1.20.1",
            "mainClass": "net.fabricmc.loader.impl.launch.knot.KnotClient",
            "arguments": {
                "game": [],
                "jvm": []
            },
            "libraries": [
                {
                    "name": "net.fabricmc:fabric-loader:0.16.10",
                    "url": "https://maven.fabricmc.net/"
                }
            ]
        }"#;

        fs::write(&loader_json_path, patch_json).expect("failed to write mock profile");

        let client = reqwest::Client::new();
        let patch = FabricLoaderEntry::fetch_profile(
            &client,
            "1.20.1",
            "0.16.10",
            &loader_json_path,
            DownloadSource::Official,
        )
        .await
        .expect("should load from local profile without hitting network");

        assert_eq!(patch.id, "fabric-loader-0.16.10-1.20.1");
        assert_eq!(patch.inherits_from.as_deref(), Some("1.20.1"));
        assert_eq!(
            patch.main_class.as_deref(),
            Some("net.fabricmc.loader.impl.launch.knot.KnotClient")
        );
        assert_eq!(patch.libraries.len(), 1);
        assert_eq!(
            patch.libraries[0].name,
            "net.fabricmc:fabric-loader:0.16.10"
        );
    }

    #[tokio::test]
    async fn test_fetch_list_with_cache_from_local_cache() {
        let temp = TempDirGuard::new("fabric_list_cached");
        let upstream_url = FabricLoaderEntry::list_url(FABRIC_META_BASE_URL, "1.20.1");
        let cache_key = crate::cache::cache_key_for_url(&upstream_url);
        let cache_file = temp.path().join(cache_key);

        let sample_entry = FabricLoaderEntry {
            loader: FabricLoaderInfo {
                separator: ".".to_string(),
                build: 1610,
                maven: "net.fabricmc:fabric-loader:0.16.10".to_string(),
                version: "0.16.10".to_string(),
                stable: true,
            },
            intermediary: FabricIntermediaryInfo {
                maven: "net.fabricmc:intermediary:1.20.1".to_string(),
                version: "1.20.1".to_string(),
                stable: true,
            },
        };

        let cached = crate::cache::Cached::new(
            vec![sample_entry.clone()],
            std::time::Duration::from_secs(3600),
        )
        .expect("failed to create cached");
        cached
            .save_to_file(&cache_file)
            .expect("failed to save cache");

        let client = reqwest::Client::new();
        let loaded = FabricLoaderEntry::fetch_list_with_cache(
            &client,
            "1.20.1",
            temp.path(),
            DownloadSource::Official,
        )
        .await
        .expect("should load from local cache without hitting network");

        assert_eq!(loaded, vec![sample_entry]);
    }
}
