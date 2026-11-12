// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::Path;

use crate::{
    DEFAULT_MANIFEST_TTL, DownloadSource, Result, VersionPatch,
    cache::{fetch_cached_json, fetch_verified_json},
};
use serde::{Deserialize, Serialize};

pub const FABRIC_META_BASE_URL: &str = "https://meta.fabricmc.net/v2";

// Live by the QSL, die by the QSL.
pub const QUILT_META_BASE_URL: &str = "https://meta.quiltmc.org/v3";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetaLoader {
    Fabric,
    Quilt,
}

impl MetaLoader {
    pub const fn base_url(self) -> &'static str {
        match self {
            Self::Fabric => FABRIC_META_BASE_URL,
            Self::Quilt => QUILT_META_BASE_URL,
        }
    }

    pub const fn artifact_prefix(self) -> &'static str {
        match self {
            Self::Fabric => "fabric-loader",
            Self::Quilt => "quilt-loader",
        }
    }
}

/// Information about a single Fabric or Quilt Loader version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetaLoaderInfo {
    pub separator: String,
    pub build: u32,
    pub maven: String,
    pub version: String,
    #[serde(default)]
    pub stable: Option<bool>,
}

impl MetaLoaderInfo {
    /// Fabric explicitly returns the stable field;
    /// Quilt does not return this field and can be judged by whether the version number contains -(such as -beta.1).
    pub fn is_stable(&self) -> bool {
        self.stable.unwrap_or_else(|| !self.version.contains('-'))
    }
}

/// Information about the intermediary mappings for a Minecraft version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetaIntermediaryInfo {
    pub maven: String,
    pub version: String,
    #[serde(default)]
    pub stable: bool,
}

/// A single entry returned by `/v2/versions/loader/{game_version}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetaLoaderEntry {
    pub loader: MetaLoaderInfo,
    pub intermediary: MetaIntermediaryInfo,
}

impl MetaLoaderEntry {
    /// Construct the upstream URL for listing Fabric or Quilt loaders of a specific Minecraft version.
    pub fn list_url(base_url: &str, game_version: &str) -> String {
        format!(
            "{}/versions/loader/{game_version}",
            base_url.trim_end_matches('/')
        )
    }

    /// Construct the upstream URL for fetching the launch profile (`VersionPatch`) JSON.
    pub fn profile_url(base_url: &str, game_version: &str, loader_version: &str) -> String {
        format!(
            "{}/versions/loader/{game_version}/{loader_version}/profile/json",
            base_url.trim_end_matches('/')
        )
    }

    /// Fetch available Fabric or Quilt loader entries for `game_version` with caching using default TTL.
    pub async fn fetch_list_with_cache(
        meta_loader: MetaLoader,
        client: &reqwest::Client,
        game_version: &str,
        cache_dir: &Path,
        download_source: DownloadSource,
        force_refresh: bool,
    ) -> Result<Vec<Self>> {
        let upstream_url = Self::list_url(meta_loader.base_url(), game_version);
        fetch_cached_json(
            client,
            &upstream_url,
            cache_dir,
            DEFAULT_MANIFEST_TTL,
            download_source,
            force_refresh,
        )
        .await
    }

    /// Fetch the Fabric or Quilt profile (`VersionPatch`) for the given Minecraft and loader version.
    /// If `loader_json_path` is already present locally, loads it directly; otherwise fetches from network and saves it.
    pub async fn fetch_profile(
        meta_loader: MetaLoader,
        client: &reqwest::Client,
        game_version: &str,
        loader_version: &str,
        loader_json_path: &Path,
        download_source: DownloadSource,
    ) -> Result<VersionPatch> {
        let upstream_url = Self::profile_url(meta_loader.base_url(), game_version, loader_version);
        fetch_verified_json(
            client,
            &download_source.rewrite_url(&upstream_url),
            "", // Fabric or Quilt profile has no sha1; pass an empty string to skip validation directly.
            &format!(
                "{}-{loader_version}-{game_version}",
                meta_loader.artifact_prefix()
            ),
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
    fn test_meta_loader_properties_and_urls() {
        assert_eq!(MetaLoader::Fabric.base_url(), FABRIC_META_BASE_URL);
        assert_eq!(MetaLoader::Fabric.artifact_prefix(), "fabric-loader");
        assert_eq!(MetaLoader::Quilt.base_url(), QUILT_META_BASE_URL);
        assert_eq!(MetaLoader::Quilt.artifact_prefix(), "quilt-loader");

        assert_eq!(
            MetaLoaderEntry::list_url(FABRIC_META_BASE_URL, "1.20.1"),
            "https://meta.fabricmc.net/v2/versions/loader/1.20.1"
        );
        assert_eq!(
            MetaLoaderEntry::list_url("https://meta.fabricmc.net/v2/", "1.20.1"),
            "https://meta.fabricmc.net/v2/versions/loader/1.20.1"
        );

        assert_eq!(
            MetaLoaderEntry::profile_url(FABRIC_META_BASE_URL, "1.20.1", "0.16.10"),
            "https://meta.fabricmc.net/v2/versions/loader/1.20.1/0.16.10/profile/json"
        );
        assert_eq!(
            MetaLoaderEntry::profile_url("https://meta.fabricmc.net/v2/", "1.20.1", "0.16.10"),
            "https://meta.fabricmc.net/v2/versions/loader/1.20.1/0.16.10/profile/json"
        );

        assert_eq!(
            MetaLoaderEntry::list_url(QUILT_META_BASE_URL, "1.21.1"),
            "https://meta.quiltmc.org/v3/versions/loader/1.21.1"
        );
        assert_eq!(
            MetaLoaderEntry::list_url("https://meta.quiltmc.org/v3/", "1.21.1"),
            "https://meta.quiltmc.org/v3/versions/loader/1.21.1"
        );
        assert_eq!(
            MetaLoaderEntry::profile_url(QUILT_META_BASE_URL, "1.21.1", "0.26.4"),
            "https://meta.quiltmc.org/v3/versions/loader/1.21.1/0.26.4/profile/json"
        );
        assert_eq!(
            MetaLoaderEntry::profile_url("https://meta.quiltmc.org/v3/", "1.21.1", "0.26.4"),
            "https://meta.quiltmc.org/v3/versions/loader/1.21.1/0.26.4/profile/json"
        );
    }

    #[test]
    fn test_meta_loader_info_is_stable() {
        let explicit_stable = MetaLoaderInfo {
            separator: ".".to_string(),
            build: 1,
            maven: "net.fabricmc:fabric-loader:0.16.10".to_string(),
            version: "0.16.10-beta.1".to_string(),
            stable: Some(true),
        };
        assert!(explicit_stable.is_stable());

        let explicit_unstable = MetaLoaderInfo {
            separator: ".".to_string(),
            build: 1,
            maven: "net.fabricmc:fabric-loader:0.16.10".to_string(),
            version: "0.16.10".to_string(),
            stable: Some(false),
        };
        assert!(!explicit_unstable.is_stable());

        let quilt_stable = MetaLoaderInfo {
            separator: ".".to_string(),
            build: 1,
            maven: "org.quiltmc:quilt-loader:0.26.4".to_string(),
            version: "0.26.4".to_string(),
            stable: None,
        };
        assert!(quilt_stable.is_stable());

        let quilt_beta = MetaLoaderInfo {
            separator: ".".to_string(),
            build: 1,
            maven: "org.quiltmc:quilt-loader:0.26.4-beta.1".to_string(),
            version: "0.26.4-beta.1".to_string(),
            stable: None,
        };
        assert!(!quilt_beta.is_stable());
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

        let entries: Vec<MetaLoaderEntry> =
            serde_json::from_str(sample_json).expect("failed to deserialize FabricLoaderEntry");
        assert_eq!(entries.len(), 1);

        let entry = &entries[0];
        assert_eq!(entry.loader.version, "0.16.10");
        assert_eq!(entry.loader.build, 1610);
        assert_eq!(entry.loader.separator, ".");
        assert_eq!(entry.loader.maven, "net.fabricmc:fabric-loader:0.16.10");
        assert_eq!(entry.loader.stable, Some(true));
        assert!(entry.loader.is_stable());

        assert_eq!(entry.intermediary.version, "1.20.1");
        assert_eq!(entry.intermediary.maven, "net.fabricmc:intermediary:1.20.1");
        assert!(entry.intermediary.stable);
    }

    #[test]
    fn test_quilt_loader_entry_deserialization() {
        let sample_json = r#"[
            {
                "loader": {
                    "separator": ".",
                    "build": 1,
                    "maven": "org.quiltmc:quilt-loader:0.26.4",
                    "version": "0.26.4"
                },
                "intermediary": {
                    "maven": "net.fabricmc:intermediary:1.21.1",
                    "version": "1.21.1"
                }
            },
            {
                "loader": {
                    "separator": ".",
                    "build": 2,
                    "maven": "org.quiltmc:quilt-loader:0.26.4-beta.1",
                    "version": "0.26.4-beta.1"
                },
                "intermediary": {
                    "maven": "net.fabricmc:intermediary:1.21.1",
                    "version": "1.21.1"
                }
            }
        ]"#;

        let entries: Vec<MetaLoaderEntry> =
            serde_json::from_str(sample_json).expect("failed to deserialize Quilt MetaLoaderEntry");
        assert_eq!(entries.len(), 2);

        assert_eq!(entries[0].loader.version, "0.26.4");
        assert_eq!(entries[0].loader.stable, None);
        assert!(entries[0].loader.is_stable());
        assert_eq!(entries[0].intermediary.version, "1.21.1");
        assert!(!entries[0].intermediary.stable);

        assert_eq!(entries[1].loader.version, "0.26.4-beta.1");
        assert_eq!(entries[1].loader.stable, None);
        assert!(!entries[1].loader.is_stable());
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
        let patch = MetaLoaderEntry::fetch_profile(
            MetaLoader::Fabric,
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
    async fn test_fetch_quilt_profile_and_list_from_local_cache() {
        let temp = TempDirGuard::new("quilt_cached");
        let loader_json_path = temp.path().join("loader.json");

        let patch_json = r#"{
            "id": "quilt-loader-0.26.4-1.21.1",
            "inheritsFrom": "1.21.1",
            "mainClass": "org.quiltmc.loader.impl.launch.knot.KnotClient",
            "arguments": {
                "game": []
            },
            "libraries": [
                {
                    "name": "org.quiltmc:quilt-loader:0.26.4",
                    "url": "https://maven.quiltmc.org/repository/release/"
                }
            ]
        }"#;

        fs::write(&loader_json_path, patch_json).expect("failed to write mock quilt profile");

        let client = reqwest::Client::new();
        let patch = MetaLoaderEntry::fetch_profile(
            MetaLoader::Quilt,
            &client,
            "1.21.1",
            "0.26.4",
            &loader_json_path,
            DownloadSource::Official,
        )
        .await
        .expect("should load quilt profile from local file without hitting network");

        assert_eq!(patch.id, "quilt-loader-0.26.4-1.21.1");
        assert_eq!(
            patch.main_class.as_deref(),
            Some("org.quiltmc.loader.impl.launch.knot.KnotClient")
        );

        let upstream_url = MetaLoaderEntry::list_url(QUILT_META_BASE_URL, "1.21.1");
        let cache_key = crate::cache::cache_key_for_url(&upstream_url);
        let cache_file = temp.path().join(cache_key);

        let sample_entry = MetaLoaderEntry {
            loader: MetaLoaderInfo {
                separator: ".".to_string(),
                build: 1,
                maven: "org.quiltmc:quilt-loader:0.26.4".to_string(),
                version: "0.26.4".to_string(),
                stable: None,
            },
            intermediary: MetaIntermediaryInfo {
                maven: "net.fabricmc:intermediary:1.21.1".to_string(),
                version: "1.21.1".to_string(),
                stable: false,
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

        let loaded = MetaLoaderEntry::fetch_list_with_cache(
            MetaLoader::Quilt,
            &client,
            "1.21.1",
            temp.path(),
            DownloadSource::Official,
            false,
        )
        .await
        .expect("should load quilt list from local cache without hitting network");

        assert_eq!(loaded, vec![sample_entry]);
    }

    #[tokio::test]
    async fn test_fetch_list_with_cache_from_local_cache() {
        let temp = TempDirGuard::new("fabric_list_cached");
        let upstream_url = MetaLoaderEntry::list_url(FABRIC_META_BASE_URL, "1.20.1");
        let cache_key = crate::cache::cache_key_for_url(&upstream_url);
        let cache_file = temp.path().join(cache_key);

        let sample_entry = MetaLoaderEntry {
            loader: MetaLoaderInfo {
                separator: ".".to_string(),
                build: 1610,
                maven: "net.fabricmc:fabric-loader:0.16.10".to_string(),
                version: "0.16.10".to_string(),
                stable: Some(true),
            },
            intermediary: MetaIntermediaryInfo {
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
        let loaded = MetaLoaderEntry::fetch_list_with_cache(
            MetaLoader::Fabric,
            &client,
            "1.20.1",
            temp.path(),
            DownloadSource::Official,
            false,
        )
        .await
        .expect("should load from local cache without hitting network");

        assert_eq!(loaded, vec![sample_entry]);
    }
}
