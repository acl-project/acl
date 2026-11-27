// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    fs,
    num::NonZeroUsize,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};

use gtk::{
    gdk,
    glib::{self, object::Cast},
};
use image::imageops::FilterType;
use lru::LruCache;
use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use tracing::debug;

/// Target decoded icon dimension (128x128 fits 64px widgets at 2x HiDPI).
const ICON_DECODE_SIZE: u32 = 128;

/// Duration to suppress retries for failed URLs in memory.
const FAILURE_TTL: Duration = Duration::from_secs(5 * 60);

/// Disk cache entry expiration (30 days).
const DISK_CACHE_TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// Maximum disk cache size (50 MiB).
const MAX_DISK_CACHE_BYTES: u64 = 50 * 1024 * 1024;

/// Maximum downloadable single icon size (5 MiB).
const MAX_DOWNLOAD_BYTES: usize = 5 * 1024 * 1024;

/// HTTP timeout for icon downloads.
const FETCH_TIMEOUT: Duration = Duration::from_secs(20);

/// Target disk cache size after pruning (40 MiB).
const DISK_CACHE_PRUNE_TARGET_BYTES: u64 = 40 * 1024 * 1024;

const MAX_MEMORY_ENTRIES: usize = 400;

fn cache_key(url: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(url.as_bytes());
    hex::encode(hasher.finalize())
}

#[derive(Debug)]
enum CacheEntry {
    Loaded(gdk::Texture),
    Failed(Instant), // Record failure time
}

pub enum CacheLookup {
    /// Cache hit; render the texture directly
    Hit(gdk::Texture),
    /// Recently failed; keep the placeholder image
    Failed,
    /// Cache miss; load asynchronously via `load()`
    Miss,
}

#[derive(Debug)]
pub struct IconCache {
    client: reqwest::Client,
    icon_dir: PathBuf,
    memory: Mutex<LruCache<String, CacheEntry>>,
}

impl IconCache {
    pub fn new(client: reqwest::Client, cache_dir: &Path) -> Self {
        let icon_dir = cache_dir.join("icons");

        let cleanup_dir = icon_dir.clone();
        // Prevent unit test failures
        if let Ok(handle) = relm4::tokio::runtime::Handle::try_current() {
            handle.spawn_blocking(move || Self::cleanup_disk_cache(&cleanup_dir));
        }

        let cap = NonZeroUsize::new(MAX_MEMORY_ENTRIES).unwrap_or(NonZeroUsize::MIN);
        let memory = Mutex::new(LruCache::new(cap));
        Self {
            client,
            icon_dir,
            memory,
        }
    }

    /// Read from in-memory cache only
    pub fn get(&self, url: &str) -> CacheLookup {
        let mut memory = self.memory.lock();

        if let Some(entry) = memory.get(url) {
            match entry {
                CacheEntry::Failed(at) => {
                    if at.elapsed() >= FAILURE_TTL {
                        memory.pop(url);
                        CacheLookup::Miss
                    } else {
                        CacheLookup::Failed
                    }
                }
                CacheEntry::Loaded(tex) => CacheLookup::Hit(tex.clone()),
            }
        } else {
            CacheLookup::Miss
        }
    }

    async fn read_disk_cache(icon_path: &Path) -> (Option<Vec<u8>>, bool) {
        let icon_path = icon_path.to_path_buf();
        relm4::tokio::task::spawn_blocking(move || {
            let Ok(meta) = fs::metadata(&icon_path) else {
                return (None, false);
            };

            if meta.len() > MAX_DOWNLOAD_BYTES as u64 {
                let _ = fs::remove_file(icon_path);
                return (None, false);
            }

            let Some(age) = meta
                .modified()
                .ok()
                .and_then(|mtime| SystemTime::now().duration_since(mtime).ok())
            else {
                return (None, false);
            };

            let is_fresh = age < DISK_CACHE_TTL;

            match fs::read(&icon_path) {
                Ok(bytes) => (Some(bytes), is_fresh),
                Err(_) => (None, false),
            }
        })
        .await
        .unwrap_or((None, false))
    }

    fn bytes_to_texture(bytes: &[u8]) -> Option<gdk::Texture> {
        let img = match image::load_from_memory(bytes) {
            Ok(img) => img,
            Err(err) => {
                debug!("Failed to decode icon image: {err}");
                return None;
            }
        };

        // Crop uniformly to a 96x96 square and convert to RGBA8
        let rgba = img
            .resize_to_fill(ICON_DECODE_SIZE, ICON_DECODE_SIZE, FilterType::Triangle)
            .to_rgba8();
        let (width, height) = rgba.dimensions();

        let texture = gdk::MemoryTexture::new(
            width as i32,
            height as i32,
            gdk::MemoryFormat::R8g8b8a8,
            &glib::Bytes::from_owned(rgba.into_raw()),
            width as usize * 4,
        );

        Some(texture.upcast())
    }

    async fn download_bytes(client: &reqwest::Client, url: &str) -> Option<Vec<u8>> {
        let mut response = match client.get(url).timeout(FETCH_TIMEOUT).send().await {
            Ok(res) => match res.error_for_status() {
                Ok(res) => res,
                Err(e) => {
                    debug!("Failed to get icon {}: {}", url, e);
                    return None;
                }
            },
            Err(e) => {
                debug!("Failed to get icon {}: {}", url, e);
                return None;
            }
        };

        if let Some(len) = response.content_length()
            && len > MAX_DOWNLOAD_BYTES as u64
        {
            debug!(
                "Failed to get icon {}: download bytes {} greater than {}",
                url, len, MAX_DOWNLOAD_BYTES
            );
            return None;
        }

        let mut bytes_read: usize = 0;
        let mut buf = Vec::new();

        while let Some(chunk) = response.chunk().await.ok()? {
            bytes_read = bytes_read.saturating_add(chunk.len());

            if bytes_read > MAX_DOWNLOAD_BYTES {
                debug!("Failed to get icon {}: download size exceeds limit", url,);
                return None;
            }

            buf.extend_from_slice(&chunk);
        }

        if buf.is_empty() {
            return None;
        }

        Some(buf)
    }

    pub async fn load(&self, url: &str) -> Option<gdk::Texture> {
        if url.is_empty() {
            return None;
        }

        match self.get(url) {
            CacheLookup::Hit(tex) => {
                return Some(tex);
            }
            CacheLookup::Failed => {
                return None;
            }
            CacheLookup::Miss => {}
        }

        let key = cache_key(url);
        let icon_dir = self.icon_dir.clone();
        let icon_path = icon_dir.join(key);

        let (mut cache_bytes, is_fresh) = Self::read_disk_cache(&icon_path).await;

        if is_fresh
            && let Some(bytes) = cache_bytes.take()
            && !bytes.is_empty()
            && let Ok(tex) =
                relm4::tokio::task::spawn_blocking(move || Self::bytes_to_texture(&bytes)).await
            && let Some(tex) = tex
        {
            let mut memory = self.memory.lock();
            memory.put(url.to_string(), CacheEntry::Loaded(tex.clone()));

            return Some(tex);
        }

        if let Some(bytes) = Self::download_bytes(&self.client, url).await {
            let c_url = url.to_string();
            if let Ok(result) = relm4::tokio::task::spawn_blocking(move || {
                let tex = Self::bytes_to_texture(&bytes)?;

                if let Err(e) = acl_common::fs::write_atomic(&icon_path, &bytes) {
                    debug!("Failed to save icon {c_url}: {e}");
                }

                Some(tex)
            })
            .await
                && let Some(tex) = result
            {
                let mut memory = self.memory.lock();
                memory.put(url.to_string(), CacheEntry::Loaded(tex.clone()));

                return Some(tex);
            }
        }

        if let Some(bytes) = cache_bytes
            && let Ok(result) =
                relm4::tokio::task::spawn_blocking(move || Self::bytes_to_texture(&bytes)).await
            && let Some(tex) = result
        {
            debug!("Failed to get fresh icon {url}, use cached icon");
            let mut memory = self.memory.lock();
            memory.put(url.to_string(), CacheEntry::Loaded(tex.clone()));

            return Some(tex);
        }

        self.memory
            .lock()
            .put(url.to_string(), CacheEntry::Failed(Instant::now()));

        None
    }

    fn cleanup_disk_cache(icon_dir: &Path) {
        let Ok(entries) = fs::read_dir(icon_dir) else {
            return;
        };

        let now = SystemTime::now();

        let mut survivors: Vec<(PathBuf, u64, SystemTime)> = Vec::new();
        let mut total_bytes: u64 = 0;
        let mut removed_count: usize = 0;

        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if !meta.is_file() {
                continue;
            }

            let Ok(mtime) = meta.modified() else {
                continue;
            };

            let path = entry.path();

            let is_expired = now
                .duration_since(mtime)
                .is_ok_and(|age| age >= DISK_CACHE_TTL);

            if is_expired {
                if fs::remove_file(&path).is_ok() {
                    removed_count += 1;
                }
                continue;
            }

            // Skip in-progress temporary files from `write_atomic`.
            if entry.file_name().to_string_lossy().contains(".tmp.") {
                continue;
            }

            let len = meta.len();
            total_bytes = total_bytes.saturating_add(len);
            survivors.push((path, len, mtime));
        }

        if total_bytes > MAX_DISK_CACHE_BYTES {
            // Evict oldest files first until total size drops to the low watermark.
            survivors.sort_by_key(|(_, _, mtime)| *mtime);

            for (path, len, _) in survivors {
                if total_bytes <= DISK_CACHE_PRUNE_TARGET_BYTES {
                    break;
                }

                if fs::remove_file(&path).is_ok() {
                    total_bytes = total_bytes.saturating_sub(len);
                    removed_count += 1;
                }
            }
        }

        if removed_count > 0 {
            debug!("Cleaned up {removed_count} cached icons, remaining size: {total_bytes} bytes");
        }
    }
}
