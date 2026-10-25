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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionEntry {
    // Version number
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
                if let Err(err) = Cached::new(manifest.clone(), ttl)
                    .and_then(|entry| entry.save_to_file(&cache_path))
                {
                    warn!("Failed to save version manifest cache: {err}");
                }
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
