use crate::error::{MinecraftError, Result};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha1::{Digest, Sha1};
use std::{
    fs::{self},
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
pub const DEFAULT_MANIFEST_TTL: Duration = Duration::from_secs(3600);

pub fn cache_key_for_url(url: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(url.as_bytes());
    format!("{}.json", hex::encode(hasher.finalize()))
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Cached<T> {
    /// The date the cache was created.
    pub cached_at: u64,
    pub ttl_secs: u64,
    pub data: T,
}

impl<T> Cached<T> {
    pub fn new(data: T, ttl: Duration) -> Result<Self> {
        let cached_at = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();

        Ok(Self {
            cached_at,
            ttl_secs: ttl.as_secs(),
            data,
        })
    }

    pub fn is_fresh(&self) -> Result<bool> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();

        Ok(now.saturating_sub(self.cached_at) < self.ttl_secs)
    }
}

impl<T: Serialize + DeserializeOwned> Cached<T> {
    pub fn load_from_file(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path).map_err(|source| MinecraftError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let cached = serde_json::from_str(&content)?;
        Ok(cached)
    }

    pub fn save_to_file(&self, path: &Path) -> Result<()> {
        let content = serde_json::to_string_pretty(self)?;

        acl_common::fs::write_atomic(path, content.as_bytes()).map_err(|source| {
            MinecraftError::Io {
                path: path.to_path_buf(),
                source,
            }
        })?;

        Ok(())
    }
}
