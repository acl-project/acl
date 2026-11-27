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

            let is_fresh = meta
                .modified()
                .ok()
                .and_then(|mtime| SystemTime::now().duration_since(mtime).ok())
                .is_some_and(|age| age < DISK_CACHE_TTL);

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

        // Crop uniformly to a 128x128 square and convert to RGBA8
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

#[cfg(test)]
mod tests {
    use super::*;
    use gtk::gdk::prelude::TextureExt;
    use relm4::tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    struct TempDirGuard {
        path: PathBuf,
    }

    impl TempDirGuard {
        fn new(name: &str) -> Self {
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let id = COUNTER.fetch_add(1, Ordering::SeqCst);
            let path = std::env::temp_dir().join(format!(
                "acl_test_icon_cache_{}_{}_{}",
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

    fn test_runtime() -> relm4::tokio::runtime::Runtime {
        relm4::tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build test tokio runtime")
    }

    fn test_client() -> reqwest::Client {
        reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("failed to build test HTTP client")
    }

    fn sample_png_bytes(width: u32, height: u32, rgba: [u8; 4]) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(width, height, image::Rgba(rgba));
        let mut out = Vec::new();
        let mut cursor = std::io::Cursor::new(&mut out);
        img.write_to(&mut cursor, image::ImageFormat::Png)
            .expect("failed to encode sample PNG");
        out
    }

    fn write_file_with_mtime(path: &Path, bytes: &[u8], mtime: SystemTime) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("failed to create parent dir");
        }
        let mut file = fs::File::create(path).expect("failed to create file");
        std::io::Write::write_all(&mut file, bytes).expect("failed to write file");
        file.set_modified(mtime).expect("failed to set file mtime");
    }

    fn create_sparse_file_with_mtime(path: &Path, len: u64, mtime: SystemTime) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("failed to create parent dir");
        }
        let file = fs::File::create(path).expect("failed to create sparse file");
        file.set_len(len).expect("failed to set sparse file length");
        file.set_modified(mtime).expect("failed to set file mtime");
    }

    async fn read_http_request(stream: &mut relm4::tokio::net::TcpStream) {
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf).await;
    }

    #[test]
    fn test_cache_key_deterministic_and_sha256_hex() {
        let key1 = cache_key("https://example.com/icon.png");
        let key2 = cache_key("https://example.com/icon.png");
        let key3 = cache_key("https://example.com/other.png");

        assert_eq!(key1, key2);
        assert_ne!(key1, key3);
        assert_eq!(key1.len(), 64);
        assert!(
            key1.chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
        );
    }

    #[test]
    fn test_bytes_to_texture_valid_and_invalid() {
        let png = sample_png_bytes(32, 16, [255, 0, 128, 255]);
        let texture = IconCache::bytes_to_texture(&png).expect("valid PNG should decode");
        assert_eq!(texture.width(), ICON_DECODE_SIZE as i32);
        assert_eq!(texture.height(), ICON_DECODE_SIZE as i32);

        assert!(IconCache::bytes_to_texture(&[]).is_none());
        assert!(IconCache::bytes_to_texture(b"not a valid image").is_none());
    }

    #[test]
    fn test_memory_cache_get_hit_miss_and_failure_ttl() {
        let temp = TempDirGuard::new("memory_get");
        let cache = IconCache::new(test_client(), temp.path());

        let url_loaded = "https://example.com/loaded.png";
        let url_failed = "https://example.com/failed.png";
        let url_expired = "https://example.com/expired-failed.png";

        assert!(matches!(cache.get(url_loaded), CacheLookup::Miss));

        let tex = IconCache::bytes_to_texture(&sample_png_bytes(16, 16, [0, 255, 0, 255]))
            .expect("decode sample PNG");
        cache
            .memory
            .lock()
            .put(url_loaded.to_string(), CacheEntry::Loaded(tex));
        cache
            .memory
            .lock()
            .put(url_failed.to_string(), CacheEntry::Failed(Instant::now()));

        assert!(matches!(cache.get(url_loaded), CacheLookup::Hit(_)));
        assert!(matches!(cache.get(url_failed), CacheLookup::Failed));

        if let Some(expired_at) = Instant::now().checked_sub(FAILURE_TTL + Duration::from_secs(1)) {
            cache
                .memory
                .lock()
                .put(url_expired.to_string(), CacheEntry::Failed(expired_at));

            assert!(matches!(cache.get(url_expired), CacheLookup::Miss));
            assert!(cache.memory.lock().peek(url_expired).is_none());
        }
    }

    #[test]
    fn test_read_disk_cache_states_and_oversized_removal() {
        let temp = TempDirGuard::new("read_disk_cache");
        let rt = test_runtime();

        let missing_path = temp.path().join("missing");
        let fresh_path = temp.path().join("fresh");
        let stale_path = temp.path().join("stale");
        let future_path = temp.path().join("future");
        let oversized_path = temp.path().join("oversized");

        let now = SystemTime::now();
        write_file_with_mtime(&fresh_path, b"fresh-bytes", now);
        write_file_with_mtime(
            &stale_path,
            b"stale-bytes",
            now - DISK_CACHE_TTL - Duration::from_secs(60),
        );
        write_file_with_mtime(
            &future_path,
            b"future-bytes",
            now + Duration::from_secs(3600),
        );
        create_sparse_file_with_mtime(&oversized_path, MAX_DOWNLOAD_BYTES as u64 + 1, now);

        rt.block_on(async {
            assert_eq!(
                IconCache::read_disk_cache(&missing_path).await,
                (None, false)
            );
            assert_eq!(
                IconCache::read_disk_cache(&fresh_path).await,
                (Some(b"fresh-bytes".to_vec()), true)
            );
            assert_eq!(
                IconCache::read_disk_cache(&stale_path).await,
                (Some(b"stale-bytes".to_vec()), false)
            );
            assert_eq!(
                IconCache::read_disk_cache(&future_path).await,
                (Some(b"future-bytes".to_vec()), false)
            );
            assert_eq!(
                IconCache::read_disk_cache(&oversized_path).await,
                (None, false)
            );
            assert!(
                !oversized_path.exists(),
                "oversized disk cache file should be removed"
            );
        });
    }

    #[test]
    fn test_load_empty_url_and_fresh_disk_cache_hit() {
        let temp = TempDirGuard::new("load_disk_hit");
        let cache = IconCache::new(test_client(), temp.path());
        let rt = test_runtime();

        let url = "http://127.0.0.1:9/offline-fresh-icon.png";
        let png = sample_png_bytes(24, 24, [10, 20, 30, 255]);
        let disk_path = cache.icon_dir.join(cache_key(url));
        write_file_with_mtime(&disk_path, &png, SystemTime::now());

        rt.block_on(async {
            assert!(cache.load("").await.is_none());
            assert!(matches!(cache.get(""), CacheLookup::Miss));

            let tex = cache
                .load(url)
                .await
                .expect("fresh disk cache should load without network");
            assert_eq!(tex.width(), ICON_DECODE_SIZE as i32);
            assert_eq!(tex.height(), ICON_DECODE_SIZE as i32);
            assert!(matches!(cache.get(url), CacheLookup::Hit(_)));
        });
    }

    #[test]
    fn test_load_downloads_persists_and_hits_memory() {
        let temp = TempDirGuard::new("load_download");
        let cache = IconCache::new(test_client(), temp.path());
        let rt = test_runtime();

        let png = sample_png_bytes(16, 32, [64, 128, 255, 255]);
        let request_count = Arc::new(AtomicUsize::new(0));

        rt.block_on(async {
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind mock server");
            let addr = listener.local_addr().expect("get local addr");
            let url = format!("http://{addr}/icon.png");

            let png_resp = png.clone();
            let count_clone = Arc::clone(&request_count);
            let server = relm4::tokio::spawn(async move {
                if let Ok((mut stream, _)) = listener.accept().await {
                    count_clone.fetch_add(1, Ordering::SeqCst);
                    read_http_request(&mut stream).await;
                    let header = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        png_resp.len()
                    );
                    let _ = stream.write_all(header.as_bytes()).await;
                    let _ = stream.write_all(&png_resp).await;
                    let _ = stream.flush().await;
                }
            });

            let first = cache.load(&url).await.expect("first load should download");
            assert_eq!(first.width(), ICON_DECODE_SIZE as i32);
            assert_eq!(first.height(), ICON_DECODE_SIZE as i32);

            let disk_path = cache.icon_dir.join(cache_key(&url));
            assert_eq!(fs::read(&disk_path).expect("read cached icon"), png);

            let second = cache
                .load(&url)
                .await
                .expect("second load should hit memory cache");
            assert_eq!(second.width(), ICON_DECODE_SIZE as i32);
            assert_eq!(request_count.load(Ordering::SeqCst), 1);

            let _ = server.await;
        });
    }

    #[test]
    fn test_load_network_errors_and_size_limits() {
        let temp = TempDirGuard::new("load_errors");
        let cache = IconCache::new(test_client(), temp.path());
        let rt = test_runtime();

        rt.block_on(async {
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind mock server");
            let addr = listener.local_addr().expect("get local addr");

            let hits_404 = Arc::new(AtomicUsize::new(0));
            let hits_404_srv = Arc::clone(&hits_404);

            let server = relm4::tokio::spawn(async move {
                for _ in 0..5 {
                    let Ok((mut stream, _)) = listener.accept().await else {
                        break;
                    };
                    let mut buf = [0u8; 1024];
                    let n = stream.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);

                    if req.starts_with("GET /404 ") {
                        hits_404_srv.fetch_add(1, Ordering::SeqCst);
                        let _ = stream
                            .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                            .await;
                    } else if req.starts_with("GET /empty ") {
                        let _ = stream
                            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                            .await;
                    } else if req.starts_with("GET /corrupt ") {
                        let body = b"definitely-not-an-image";
                        let header = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        );
                        let _ = stream.write_all(header.as_bytes()).await;
                        let _ = stream.write_all(body).await;
                    } else if req.starts_with("GET /too-large-header ") {
                        let header = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            MAX_DOWNLOAD_BYTES + 1
                        );
                        let _ = stream.write_all(header.as_bytes()).await;
                    } else if req.starts_with("GET /too-large-stream ") {
                        let _ = stream
                            .write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n")
                            .await;
                        let chunk = vec![0u8; 64 * 1024];
                        let mut sent = 0usize;
                        while sent <= MAX_DOWNLOAD_BYTES {
                            if stream.write_all(&chunk).await.is_err() {
                                break;
                            }
                            sent += chunk.len();
                        }
                    }
                    let _ = stream.flush().await;
                }
            });

            let url_404 = format!("http://{addr}/404");
            assert!(cache.load(&url_404).await.is_none());
            assert!(matches!(cache.get(&url_404), CacheLookup::Failed));
            // Second attempt must be suppressed by FAILURE_TTL without hitting the server.
            assert!(cache.load(&url_404).await.is_none());
            assert_eq!(hits_404.load(Ordering::SeqCst), 1);

            let url_empty = format!("http://{addr}/empty");
            assert!(cache.load(&url_empty).await.is_none());
            assert!(matches!(cache.get(&url_empty), CacheLookup::Failed));

            let url_corrupt = format!("http://{addr}/corrupt");
            assert!(cache.load(&url_corrupt).await.is_none());
            assert!(!cache.icon_dir.join(cache_key(&url_corrupt)).exists());

            let url_large_header = format!("http://{addr}/too-large-header");
            assert!(cache.load(&url_large_header).await.is_none());

            let url_large_stream = format!("http://{addr}/too-large-stream");
            assert!(cache.load(&url_large_stream).await.is_none());

            let _ = server.await;
        });
    }

    #[test]
    fn test_load_stale_disk_cache_refresh_and_fallback() {
        let temp = TempDirGuard::new("load_stale");
        let cache = IconCache::new(test_client(), temp.path());
        let rt = test_runtime();

        let old_png = sample_png_bytes(16, 16, [255, 0, 0, 255]);
        let new_png = sample_png_bytes(32, 32, [0, 0, 255, 255]);
        let stale_mtime = SystemTime::now() - DISK_CACHE_TTL - Duration::from_secs(60);

        rt.block_on(async {
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind mock server");
            let addr = listener.local_addr().expect("get local addr");

            let url_refresh = format!("http://{addr}/refresh.png");
            let url_fallback = format!("http://{addr}/fallback.png");

            let refresh_path = cache.icon_dir.join(cache_key(&url_refresh));
            let fallback_path = cache.icon_dir.join(cache_key(&url_fallback));
            write_file_with_mtime(&refresh_path, &old_png, stale_mtime);
            write_file_with_mtime(&fallback_path, &old_png, stale_mtime);

            let new_png_srv = new_png.clone();
            let server = relm4::tokio::spawn(async move {
                for _ in 0..2 {
                    let Ok((mut stream, _)) = listener.accept().await else {
                        break;
                    };
                    let mut buf = [0u8; 1024];
                    let n = stream.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);

                    if req.starts_with("GET /refresh.png ") {
                        let header = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            new_png_srv.len()
                        );
                        let _ = stream.write_all(header.as_bytes()).await;
                        let _ = stream.write_all(&new_png_srv).await;
                    } else {
                        let _ = stream
                            .write_all(b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                            .await;
                    }
                    let _ = stream.flush().await;
                }
            });

            let refreshed = cache
                .load(&url_refresh)
                .await
                .expect("stale cache should refresh from server");
            assert_eq!(refreshed.width(), ICON_DECODE_SIZE as i32);
            assert_eq!(
                fs::read(&refresh_path).expect("read refreshed disk cache"),
                new_png
            );

            let fallback = cache
                .load(&url_fallback)
                .await
                .expect("failed refresh should fall back to stale disk cache");
            assert_eq!(fallback.width(), ICON_DECODE_SIZE as i32);
            assert!(matches!(cache.get(&url_fallback), CacheLookup::Hit(_)));

            let _ = server.await;
        });
    }

    #[test]
    fn test_cleanup_disk_cache_expiry_tmp_skip_and_lru_eviction() {
        let temp = TempDirGuard::new("cleanup");
        let icon_dir = temp.path().join("icons");
        fs::create_dir_all(&icon_dir).expect("create icon dir");

        let now = SystemTime::now();
        let expired_mtime = now - DISK_CACHE_TTL - Duration::from_secs(3600);

        let expired_icon = icon_dir.join("expired_icon");
        let expired_tmp = icon_dir.join("expired.tmp.1234.0");
        let active_tmp = icon_dir.join("active.tmp.1234.1");
        let subdir = icon_dir.join("nested_dir");
        let oldest = icon_dir.join("oldest");
        let middle = icon_dir.join("middle");
        let newest = icon_dir.join("newest");

        write_file_with_mtime(&expired_icon, b"old", expired_mtime);
        write_file_with_mtime(&expired_tmp, b"old-tmp", expired_mtime);
        create_sparse_file_with_mtime(&active_tmp, 20 * 1024 * 1024, now);
        fs::create_dir_all(&subdir).expect("create subdir");

        // Three 20 MiB survivors total 60 MiB (> 50 MiB cap); evicting the oldest brings total to 40 MiB target.
        let chunk_size = 20 * 1024 * 1024;
        create_sparse_file_with_mtime(&oldest, chunk_size, now - Duration::from_secs(3 * 3600));
        create_sparse_file_with_mtime(&middle, chunk_size, now - Duration::from_secs(2 * 3600));
        create_sparse_file_with_mtime(&newest, chunk_size, now - Duration::from_secs(3600));

        IconCache::cleanup_disk_cache(&icon_dir);

        assert!(!expired_icon.exists(), "expired icon should be removed");
        assert!(!expired_tmp.exists(), "expired tmp file should be removed");
        assert!(active_tmp.exists(), "fresh tmp file should be skipped");
        assert!(subdir.exists(), "subdirectories should be ignored");
        assert!(!oldest.exists(), "oldest survivor should be evicted");
        assert!(middle.exists(), "middle survivor should remain");
        assert!(newest.exists(), "newest survivor should remain");
    }
}
