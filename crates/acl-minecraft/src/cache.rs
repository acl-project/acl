// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::error::{MinecraftError, Result};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha1::{Digest, Sha1};
use std::{
    fs::{self},
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tracing::{debug, warn};
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

pub fn verify_sha1(bytes: &[u8], expected_sha1: &str, target: &str) -> Result<()> {
    // Mod loaders such as Fabric do not provide a SHA-1 for profile JSON
    if expected_sha1.is_empty() {
        return Ok(());
    }
    let mut hasher = Sha1::new();
    hasher.update(bytes);
    let actual = hex::encode(hasher.finalize());

    if !actual.eq_ignore_ascii_case(expected_sha1) {
        return Err(MinecraftError::HashMismatch {
            target: target.to_string(),
            expected: expected_sha1.to_string(),
            actual,
        });
    }

    Ok(())
}
/// First load from the local cache; if the hash does not match, fetch from the URL and save it locally.
pub async fn fetch_verified_json<T: DeserializeOwned>(
    client: &reqwest::Client,
    url: &str,
    expected_sha1: &str,
    target_id: &str,
    cache_path: &Path,
) -> Result<T> {
    if cache_path.exists() {
        match fs::read(cache_path) {
            Ok(bytes) => {
                if verify_sha1(&bytes, expected_sha1, target_id).is_ok()
                    && let Ok(parsed) = serde_json::from_slice::<T>(&bytes)
                {
                    debug!(
                        "Loaded verified metadata for '{}' from {}",
                        target_id,
                        cache_path.display()
                    );
                    return Ok(parsed);
                }

                warn!(
                    "Local cached file at {} failed verification or parsing, re-fetching",
                    cache_path.display()
                );
            }
            Err(err) => {
                warn!(
                    "Failed to read cached file at {}: {err}",
                    cache_path.display()
                );
            }
        }
    }

    let response = client.get(url).send().await?.error_for_status()?;
    let bytes = response.bytes().await?;

    verify_sha1(&bytes, expected_sha1, target_id)?;
    let parsed: T = serde_json::from_slice(&bytes)?;

    if let Err(err) = acl_common::fs::write_atomic(cache_path, &bytes) {
        warn!("Failed to save file to {}: {err}", cache_path.display());
    }

    Ok(parsed)
}

/// Fetch JSON data with TTL caching per download source and offline fallback to expired cache.
pub async fn fetch_cached_json<T>(
    client: &reqwest::Client,
    url: &str,
    cache_dir: &Path,
    ttl: Duration,
    download_source: crate::mirror::DownloadSource,
    force_refresh: bool,
) -> Result<T>
where
    T: Serialize + DeserializeOwned + Clone,
{
    // Store the cache separately for each download source.
    // This ensures that when a download source has not synchronized the latest version, the version list will not show unsupported versions.
    let url = download_source.rewrite_url(url);
    let file_name = cache_key_for_url(&url);
    let cache_path = cache_dir.join(file_name);

    let cached = Cached::<T>::load_from_file(&cache_path).ok();

    // Only when it is not a forced refresh should a valid fresh cache be returned directly
    if !force_refresh {
        // If a clock anomaly occurs, unwrap_or(false) ensures that it does not abort early and instead continues to issue the network request below.
        if let Some(ref entry) = cached
            && entry.is_fresh().unwrap_or(false)
        {
            debug!("Using fresh cache from {}", cache_path.display());
            return Ok(entry.data.clone());
        }
    }

    let fetch_result: Result<T> = async {
        let response = client.get(&url).send().await?.error_for_status()?;
        Ok(response.json().await?)
    }
    .await;

    match fetch_result {
        Ok(data) => {
            if let Err(err) =
                Cached::new(data.clone(), ttl).and_then(|entry| entry.save_to_file(&cache_path))
            {
                warn!("Failed to save cache to {}: {err}", cache_path.display());
            }
            Ok(data)
        }
        Err(err) => {
            // Fallback to expired cache when offline or network fails
            if let Some(entry) = cached {
                warn!(
                    "Network fetch failed ({err}), falling back to expired cache from {}",
                    cache_path.display()
                );
                Ok(entry.data)
            } else {
                Err(err)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
                "acl_test_mc_cache_{}_{}_{}",
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
    fn test_cache_key_for_url() {
        let key1 =
            cache_key_for_url("https://piston-meta.mojang.com/mc/game/version_manifest_v2.json");
        let key2 =
            cache_key_for_url("https://piston-meta.mojang.com/mc/game/version_manifest_v2.json");
        let key3 = cache_key_for_url("https://example.com/other.json");

        assert_eq!(key1, key2);
        assert_ne!(key1, key3);
        assert!(key1.ends_with(".json"));
        // SHA1 is 40 hex chars + 5 for ".json"
        assert_eq!(key1.len(), 45);
    }

    #[test]
    fn test_cached_freshness() {
        let cached = Cached::new("manifest_data".to_string(), Duration::from_secs(3600))
            .expect("failed to create cached");
        assert!(cached.is_fresh().expect("failed to check freshness"));

        let expired = Cached {
            cached_at: cached.cached_at.saturating_sub(100),
            ttl_secs: 10,
            data: "manifest_data".to_string(),
        };
        assert!(!expired.is_fresh().expect("failed to check freshness"));
    }

    #[test]
    fn test_cached_save_and_load_roundtrip() {
        let temp = TempDirGuard::new("roundtrip");
        let cache_file = temp.path().join("cache.json");

        let cached = Cached::new("test_payload".to_string(), Duration::from_secs(120))
            .expect("failed to create cached");
        cached
            .save_to_file(&cache_file)
            .expect("failed to save cached to file");

        let loaded: Cached<String> =
            Cached::load_from_file(&cache_file).expect("failed to load cached from file");
        assert_eq!(loaded.data, "test_payload");
        assert_eq!(loaded.cached_at, cached.cached_at);
        assert_eq!(loaded.ttl_secs, cached.ttl_secs);
    }

    #[test]
    fn test_cached_load_from_nonexistent_file() {
        let temp = TempDirGuard::new("missing");
        let cache_file = temp.path().join("nonexistent.json");

        let result = Cached::<String>::load_from_file(&cache_file);
        assert!(matches!(result, Err(MinecraftError::Io { .. })));
    }

    #[test]
    fn test_cached_load_from_invalid_json() {
        let temp = TempDirGuard::new("invalid_json");
        let cache_file = temp.path().join("invalid.json");

        fs::write(&cache_file, "not valid json").expect("failed to write invalid json");
        let result = Cached::<String>::load_from_file(&cache_file);
        assert!(matches!(result, Err(MinecraftError::Json(_))));
    }

    #[test]
    fn test_verify_sha1() {
        let content = b"hello minecraft";
        let mut hasher = Sha1::new();
        hasher.update(content);
        let valid_sha1 = hex::encode(hasher.finalize());

        assert!(verify_sha1(content, &valid_sha1, "test-target").is_ok());
        // Verify case insensitivity
        assert!(verify_sha1(content, &valid_sha1.to_uppercase(), "test-target").is_ok());

        let result = verify_sha1(
            content,
            "0000000000000000000000000000000000000000",
            "test-target",
        );
        assert!(result.is_err());
        match result {
            Err(MinecraftError::HashMismatch {
                target,
                expected,
                actual,
            }) => {
                assert_eq!(target, "test-target");
                assert_eq!(expected, "0000000000000000000000000000000000000000");
                assert_eq!(actual, valid_sha1);
            }
            _ => panic!("Expected HashMismatch error"),
        }
    }

    #[tokio::test]
    async fn test_fetch_verified_json_from_valid_local_cache() {
        let temp = TempDirGuard::new("fetch_verified_valid");
        let cache_file = temp.path().join("verified.json");

        let payload = r#"{"name":"test_entry","count":42}"#;
        fs::write(&cache_file, payload).expect("failed to write payload");

        let mut hasher = Sha1::new();
        hasher.update(payload.as_bytes());
        let expected_sha1 = hex::encode(hasher.finalize());

        let client = reqwest::Client::new();
        let parsed: serde_json::Value = fetch_verified_json(
            &client,
            "http://127.0.0.1:9/unused",
            &expected_sha1,
            "test_target",
            &cache_file,
        )
        .await
        .expect("should load from local cache without hitting network");

        assert_eq!(parsed["name"], "test_entry");
        assert_eq!(parsed["count"], 42);
    }

    #[test]
    fn test_verify_sha1_empty_expected_skips_check() {
        let content = b"arbitrary mod loader profile json bytes";
        assert!(verify_sha1(content, "", "test-target").is_ok());
    }

    #[tokio::test]
    async fn test_fetch_cached_json_from_fresh_local_cache() {
        let temp = TempDirGuard::new("fetch_cached_valid");
        let dummy_url = "https://example.com/test_data.json";
        let cache_key = cache_key_for_url(dummy_url);
        let cache_path = temp.path().join(cache_key);

        let data = vec!["entry1".to_string(), "entry2".to_string()];
        let cached =
            Cached::new(data.clone(), Duration::from_secs(3600)).expect("failed to create cached");
        cached
            .save_to_file(&cache_path)
            .expect("failed to save cache");

        let client = reqwest::Client::new();
        let loaded: Vec<String> = fetch_cached_json(
            &client,
            dummy_url,
            temp.path(),
            Duration::from_secs(3600),
            crate::mirror::DownloadSource::Official,
            false,
        )
        .await
        .expect("should load from fresh cache without network");

        assert_eq!(loaded, data);
    }

    #[tokio::test]
    async fn test_fetch_cached_json_force_refresh_bypasses_cache() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let temp = TempDirGuard::new("fetch_cached_force_refresh");

        // 1. Start a local mock server returning new data
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock server");
        let local_addr = listener.local_addr().expect("get local addr");
        let url = format!("http://{local_addr}/new_data.json");
        let cache_key = cache_key_for_url(&url);
        let cache_path = temp.path().join(cache_key);

        // 2. Pre-seed local cache with old data (still fresh!)
        let old_data = vec!["cached_old_entry".to_string()];
        let cached =
            Cached::new(old_data.clone(), Duration::from_secs(3600)).expect("create cached");
        cached.save_to_file(&cache_path).expect("save cache");

        let server_handle = tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf).await;
                let payload = br#"["fresh_network_entry"]"#;
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    payload.len()
                );
                let _ = stream.write_all(header.as_bytes()).await;
                let _ = stream.write_all(payload).await;
                let _ = stream.flush().await;
            }
        });

        let client = reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("create test client");
        // 3. fetch with force_refresh = true: should bypass fresh cache and fetch from server
        let loaded: Vec<String> = fetch_cached_json(
            &client,
            &url,
            temp.path(),
            Duration::from_secs(3600),
            crate::mirror::DownloadSource::Official,
            true,
        )
        .await
        .expect("should fetch from mock server when force_refresh is true");

        let _ = server_handle.await;

        assert_eq!(loaded, vec!["fresh_network_entry".to_string()]);

        // 4. Verify that the cache file on disk was updated with the fresh data
        let reloaded: Cached<Vec<String>> =
            Cached::load_from_file(&cache_path).expect("load updated cache");
        assert_eq!(reloaded.data, vec!["fresh_network_entry".to_string()]);
    }
}
