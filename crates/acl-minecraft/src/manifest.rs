use std::{path::Path, time::Duration};

use crate::{
    cache::{Cached, DEFAULT_MANIFEST_TTL, cache_key_for_url},
    error::Result,
};
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};
pub const VERSION_MANIFEST_URL: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LatestVersions {
    pub release: String,
    pub snapshot: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VersionType {
    Release,
    Snapshot,
    OldBeta,
    OldAlpha,
}

impl VersionType {
    pub fn label(&self) -> &'static str {
        match self {
            VersionType::Release => "Release",
            VersionType::Snapshot => "Snapshot",
            VersionType::OldBeta => "Old Beta",
            VersionType::OldAlpha => "Old Alpha",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionEntry {
    // Minecraft version number, not instance id
    pub id: String,

    #[serde(rename = "type")]
    pub version_type: VersionType,

    pub url: String,
    pub time: String,

    #[serde(rename = "releaseTime")]
    pub release_time: String,

    pub sha1: String,
    #[serde(rename = "complianceLevel")]
    pub compliance_level: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionManifest {
    pub latest: LatestVersions,
    pub versions: Vec<VersionEntry>,
}

impl VersionManifest {
    pub async fn fetch_from_url(client: &reqwest::Client, url: &str) -> Result<Self> {
        let response = client.get(url).send().await?.error_for_status()?;
        let manifest: VersionManifest = response.json().await?;
        Ok(manifest)
    }

    /// Fetch version manifest with caching using default TTL.
    pub async fn fetch_with_cache(client: &reqwest::Client, cache_dir: &Path) -> Result<Self> {
        Self::fetch_with_cache_from_url(
            client,
            VERSION_MANIFEST_URL,
            cache_dir,
            DEFAULT_MANIFEST_TTL,
        )
        .await
    }

    pub async fn fetch_with_cache_from_url(
        client: &reqwest::Client,
        url: &str,
        cache_dir: &Path,
        ttl: Duration,
    ) -> Result<Self> {
        let file_name = cache_key_for_url(url);
        let cache_path = cache_dir.join(file_name);

        let cached = Cached::<Self>::load_from_file(&cache_path).ok();

        // If a clock anomaly occurs, unwrap_or(false) ensures that it does not abort early and instead continues to issue the network request below.
        if let Some(ref entry) = cached
            && entry.is_fresh().unwrap_or(false)
        {
            debug!(
                "Using fresh version manifest cache from {}",
                cache_path.display()
            );
            return Ok(entry.data.clone());
        }

        match Self::fetch_from_url(client, url).await {
            Ok(manifest) => {
                manifest.save_cache(cache_dir, url, ttl);
                Ok(manifest)
            }

            Err(err) => {
                // Fallback to expired cache when offline or network fails
                if let Some(entry) = cached {
                    warn!(
                        "Network fetch failed ({err}), falling back to expired manifest cache from {}",
                        cache_path.display()
                    );
                    Ok(entry.data)
                } else {
                    Err(err)
                }
            }
        }
    }

    /// Cache the manifest at the default address.
    pub fn cache(&self, cache_dir: &Path) {
        self.save_cache(cache_dir, VERSION_MANIFEST_URL, DEFAULT_MANIFEST_TTL);
    }

    /// Manually cache the manifest.
    pub fn save_cache(&self, cache_dir: &Path, url: &str, ttl: Duration) {
        let file_name = cache_key_for_url(url);
        let cache_path = cache_dir.join(file_name);
        if let Err(err) =
            Cached::new(self.clone(), ttl).and_then(|entry| entry.save_to_file(&cache_path))
        {
            warn!("Failed to save version manifest cache: {err}");
        }
    }

    pub fn find_version(&self, id: &str) -> Option<&VersionEntry> {
        self.versions.iter().find(|version| version.id == id)
    }

    pub fn filter_by_type(&self, version_type: VersionType) -> Vec<&VersionEntry> {
        self.versions
            .iter()
            .filter(|version| version.version_type == version_type)
            .collect()
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
                "acl_test_mc_manifest_{}_{}_{}",
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

    fn sample_manifest() -> VersionManifest {
        let json_str = r#"{
            "latest": {
                "release": "1.20.4",
                "snapshot": "24w03a"
            },
            "versions": [
                {
                    "id": "1.20.4",
                    "type": "release",
                    "url": "https://example.com/1.20.4.json",
                    "time": "2023-12-07T12:00:00+00:00",
                    "releaseTime": "2023-12-07T12:00:00+00:00",
                    "sha1": "abcdef1234567890",
                    "complianceLevel": 1
                },
                {
                    "id": "24w03a",
                    "type": "snapshot",
                    "url": "https://example.com/24w03a.json",
                    "time": "2024-01-17T12:00:00+00:00",
                    "releaseTime": "2024-01-17T12:00:00+00:00",
                    "sha1": "1234567890abcdef",
                    "complianceLevel": 1
                },
                {
                    "id": "b1.8.1",
                    "type": "old_beta",
                    "url": "https://example.com/b1.8.1.json",
                    "time": "2011-09-19T12:00:00+00:00",
                    "releaseTime": "2011-09-19T12:00:00+00:00",
                    "sha1": "fedcba0987654321",
                    "complianceLevel": 0
                },
                {
                    "id": "a1.2.6",
                    "type": "old_alpha",
                    "url": "https://example.com/a1.2.6.json",
                    "time": "2010-12-03T12:00:00+00:00",
                    "releaseTime": "2010-12-03T12:00:00+00:00",
                    "sha1": "0987654321fedcba",
                    "complianceLevel": 0
                }
            ]
        }"#;
        serde_json::from_str(json_str).expect("failed to deserialize sample manifest")
    }

    #[test]
    fn test_manifest_deserialization() {
        let manifest = sample_manifest();
        assert_eq!(manifest.latest.release, "1.20.4");
        assert_eq!(manifest.latest.snapshot, "24w03a");
        assert_eq!(manifest.versions.len(), 4);

        let v1 = &manifest.versions[0];
        assert_eq!(v1.id, "1.20.4");
        assert_eq!(v1.version_type, VersionType::Release);
        assert_eq!(v1.compliance_level, 1);
        assert_eq!(v1.sha1, "abcdef1234567890");

        let beta = &manifest.versions[2];
        assert_eq!(beta.version_type, VersionType::OldBeta);

        let alpha = &manifest.versions[3];
        assert_eq!(alpha.version_type, VersionType::OldAlpha);
    }

    #[test]
    fn test_manifest_find_version() {
        let manifest = sample_manifest();

        let found = manifest.find_version("1.20.4");
        assert!(found.is_some());
        assert_eq!(
            found.map(|v| v.url.as_str()),
            Some("https://example.com/1.20.4.json")
        );

        let not_found = manifest.find_version("9.99.9");
        assert!(not_found.is_none());
    }

    #[test]
    fn test_manifest_filter_by_type() {
        let manifest = sample_manifest();

        let releases = manifest.filter_by_type(VersionType::Release);
        assert_eq!(releases.len(), 1);
        assert_eq!(releases[0].id, "1.20.4");

        let snapshots = manifest.filter_by_type(VersionType::Snapshot);
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].id, "24w03a");

        let old_betas = manifest.filter_by_type(VersionType::OldBeta);
        assert_eq!(old_betas.len(), 1);
        assert_eq!(old_betas[0].id, "b1.8.1");
    }

    #[test]
    fn test_version_type_serde_roundtrip() {
        let release_json = serde_json::to_string(&VersionType::Release).expect("serialize");
        assert_eq!(release_json, "\"release\"");
        let parsed: VersionType = serde_json::from_str(&release_json).expect("deserialize");
        assert_eq!(parsed, VersionType::Release);

        let beta_json = serde_json::to_string(&VersionType::OldBeta).expect("serialize");
        assert_eq!(beta_json, "\"old_beta\"");
        let parsed_beta: VersionType = serde_json::from_str(&beta_json).expect("deserialize");
        assert_eq!(parsed_beta, VersionType::OldBeta);
    }

    #[tokio::test]
    async fn test_fetch_with_cache_from_url_uses_cache() {
        let temp = TempDirGuard::new("cached_fetch");
        let manifest = sample_manifest();
        let dummy_url = "https://example.com/mock_manifest.json";

        let cache_key = cache_key_for_url(dummy_url);
        let cache_path = temp.path().join(cache_key);

        let cached = Cached::new(manifest.clone(), Duration::from_secs(3600))
            .expect("failed to create cached");
        cached
            .save_to_file(&cache_path)
            .expect("failed to save cache");

        let client = reqwest::Client::new();
        let loaded = VersionManifest::fetch_with_cache_from_url(
            &client,
            dummy_url,
            temp.path(),
            Duration::from_secs(3600),
        )
        .await
        .expect("should load from fresh cache without network");

        assert_eq!(loaded, manifest);
    }
}
