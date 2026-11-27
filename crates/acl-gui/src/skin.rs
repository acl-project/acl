// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use acl_common::hash::sha256_hex;
use gtk::{
    gdk,
    glib::{self, object::Cast},
};
use image::{
    GenericImageView,
    imageops::{self, FilterType},
};
use parking_lot::Mutex;
use tracing::debug;

/// Maximum single skin size (1 MiB).
const MAX_SKIN_BYTES: usize = 1024 * 1024;
/// HTTP timeout for skin downloads.
const FETCH_TIMEOUT: Duration = Duration::from_secs(20);

const AVATAR_SIZE: u32 = 128;

#[derive(Debug)]
pub struct SkinCache {
    client: reqwest::Client,
    skin_dir: PathBuf,
    memory: Mutex<HashMap<String, gdk::Texture>>,
}

impl SkinCache {
    pub fn new(client: reqwest::Client, cache_dir: &Path) -> Self {
        let skin_dir = cache_dir.join("skins");
        let memory = Mutex::new(HashMap::new());
        Self {
            client,
            skin_dir,
            memory,
        }
    }

    /// Synchronous memory lookup
    pub fn get(&self, url: &str) -> Option<gdk::Texture> {
        self.memory.lock().get(url).cloned()
    }

    fn bytes_to_avatar_texture(bytes: &[u8]) -> Option<gdk::Texture> {
        let img = match image::load_from_memory(bytes) {
            Ok(img) => img,
            Err(err) => {
                debug!("Failed to decode skin image: {err}");
                return None;
            }
        };
        let (w, h) = img.dimensions();

        // Validate skin dimensions: width must be at least 64 and a multiple of 64, and height must match a 64x64 or 64x32 aspect ratio
        if w < 64 || w % 64 != 0 {
            return None;
        }
        let scale = w / 64;
        if h != 32 * scale && h != 64 * scale {
            return None;
        }

        let face_size = 8 * scale;
        let mut face = img
            .view(8 * scale, 8 * scale, face_size, face_size)
            .to_image();
        let hat = img
            .view(40 * scale, 8 * scale, face_size, face_size)
            .to_image();

        imageops::overlay(&mut face, &hat, 0, 0);

        let scaled = imageops::resize(&face, AVATAR_SIZE, AVATAR_SIZE, FilterType::Nearest);

        let texture = gdk::MemoryTexture::new(
            AVATAR_SIZE as i32,
            AVATAR_SIZE as i32,
            gdk::MemoryFormat::R8g8b8a8,
            &glib::Bytes::from_owned(scaled.into_raw()),
            AVATAR_SIZE as usize * 4,
        );

        Some(texture.upcast())
    }

    async fn download_bytes(client: &reqwest::Client, url: &str) -> Option<Vec<u8>> {
        crate::cache::download_bytes(client, url, MAX_SKIN_BYTES, FETCH_TIMEOUT).await
    }

    async fn read_disk_cache(skin_path: &Path) -> Option<Vec<u8>> {
        let skin_path = skin_path.to_path_buf();
        relm4::tokio::task::spawn_blocking(move || {
            let metadata = fs::metadata(&skin_path).ok()?;

            if !metadata.is_file() || metadata.len() > MAX_SKIN_BYTES as u64 {
                return None;
            }

            fs::read(&skin_path).ok()
        })
        .await
        .ok()
        .flatten()
    }

    pub async fn load(&self, url: &str) -> Option<gdk::Texture> {
        if url.is_empty() {
            return None;
        }

        if let Some(texture) = self.get(url) {
            return Some(texture);
        }

        let hash = sha256_hex(url);
        let skin_path = self.skin_dir.join(hash);

        if let Some(bytes) = Self::read_disk_cache(&skin_path).await
            && let Some(texture) =
                relm4::tokio::task::spawn_blocking(move || Self::bytes_to_avatar_texture(&bytes))
                    .await
                    .ok()
                    .flatten()
        {
            self.memory.lock().insert(url.to_owned(), texture.clone());
            return Some(texture);
        }

        if let Some(bytes) = Self::download_bytes(&self.client, url).await {
            let skin_url = url.to_string();
            if let Ok(result) = relm4::tokio::task::spawn_blocking(move || {
                let texture = Self::bytes_to_avatar_texture(&bytes)?;

                if let Err(e) = acl_common::fs::write_atomic(&skin_path, &bytes) {
                    debug!("Failed to save skin {skin_url}: {e}");
                }

                Some(texture)
            })
            .await
                && let Some(texture) = result
            {
                let mut memory = self.memory.lock();
                memory.insert(url.to_string(), texture.clone());

                return Some(texture);
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gtk::gdk::prelude::{TextureExt, TextureExtManual};
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
                "acl_test_skin_cache_{}_{}_{}",
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

    fn encode_png(img: &image::RgbaImage) -> Vec<u8> {
        let mut out = Vec::new();
        let mut cursor = std::io::Cursor::new(&mut out);
        img.write_to(&mut cursor, image::ImageFormat::Png)
            .expect("failed to encode sample PNG");
        out
    }

    /// Creates a synthetic Minecraft skin PNG with distinct face and hat pixels.
    /// - Face region `(8*scale, 8*scale, 8*scale, 8*scale)` is filled with `face_rgba`.
    /// - Hat region `(40*scale, 8*scale, 8*scale, 8*scale)` is transparent except the
    ///   right half, which is filled with `hat_rgba`.
    fn sample_skin_png(width: u32, height: u32, face_rgba: [u8; 4], hat_rgba: [u8; 4]) -> Vec<u8> {
        let mut img = image::RgbaImage::from_pixel(width, height, image::Rgba([0, 0, 0, 0]));
        let scale = width / 64;
        let face_size = 8 * scale;

        for y in 0..face_size {
            for x in 0..face_size {
                img.put_pixel(8 * scale + x, 8 * scale + y, image::Rgba(face_rgba));
                if x >= face_size / 2 {
                    img.put_pixel(40 * scale + x, 8 * scale + y, image::Rgba(hat_rgba));
                }
            }
        }

        encode_png(&img)
    }

    /// Reads an ARGB32 pixel `(a, r, g, b)` from a `gdk::Texture` at `(x, y)`.
    fn texture_pixel_argb(texture: &gdk::Texture, x: usize, y: usize) -> [u8; 4] {
        let width = texture.width() as usize;
        let height = texture.height() as usize;
        let stride = width * 4;
        let mut buf = vec![0u8; stride * height];
        texture.download(&mut buf, stride);

        let offset = y * stride + x * 4;
        let argb = u32::from_ne_bytes([
            buf[offset],
            buf[offset + 1],
            buf[offset + 2],
            buf[offset + 3],
        ]);
        [
            ((argb >> 24) & 0xff) as u8,
            ((argb >> 16) & 0xff) as u8,
            ((argb >> 8) & 0xff) as u8,
            (argb & 0xff) as u8,
        ]
    }

    async fn read_http_request(stream: &mut relm4::tokio::net::TcpStream) -> String {
        let mut buf = [0u8; 1024];
        let n = stream.read(&mut buf).await.unwrap_or(0);
        String::from_utf8_lossy(&buf[..n]).into_owned()
    }

    #[test]
    fn test_bytes_to_avatar_texture_valid_sizes_and_hat_overlay() {
        let face = [200, 100, 50, 255];
        let hat = [10, 220, 40, 255];

        // 64x64 modern skin
        let png_64x64 = sample_skin_png(64, 64, face, hat);
        let tex_64x64 =
            SkinCache::bytes_to_avatar_texture(&png_64x64).expect("64x64 skin should decode");
        assert_eq!(tex_64x64.width(), AVATAR_SIZE as i32);
        assert_eq!(tex_64x64.height(), AVATAR_SIZE as i32);
        // Left half of the face has transparent hat -> shows face color
        assert_eq!(
            texture_pixel_argb(&tex_64x64, 0, 0),
            [face[3], face[0], face[1], face[2]]
        );
        // Right half of the face has opaque hat -> overlaid with hat color
        assert_eq!(
            texture_pixel_argb(&tex_64x64, (AVATAR_SIZE / 2) as usize, 0),
            [hat[3], hat[0], hat[1], hat[2]]
        );

        // 64x32 legacy skin
        let png_64x32 = sample_skin_png(64, 32, face, hat);
        let tex_64x32 =
            SkinCache::bytes_to_avatar_texture(&png_64x32).expect("64x32 skin should decode");
        assert_eq!(tex_64x32.width(), AVATAR_SIZE as i32);
        assert_eq!(tex_64x32.height(), AVATAR_SIZE as i32);
        assert_eq!(
            texture_pixel_argb(&tex_64x32, 0, 0),
            [face[3], face[0], face[1], face[2]]
        );
        assert_eq!(
            texture_pixel_argb(&tex_64x32, (AVATAR_SIZE / 2) as usize, 0),
            [hat[3], hat[0], hat[1], hat[2]]
        );

        // 128x128 HD skin (2x scale)
        let png_128x128 = sample_skin_png(128, 128, face, hat);
        let tex_128x128 =
            SkinCache::bytes_to_avatar_texture(&png_128x128).expect("128x128 skin should decode");
        assert_eq!(tex_128x128.width(), AVATAR_SIZE as i32);
        assert_eq!(tex_128x128.height(), AVATAR_SIZE as i32);
        assert_eq!(
            texture_pixel_argb(&tex_128x128, 0, 0),
            [face[3], face[0], face[1], face[2]]
        );
        assert_eq!(
            texture_pixel_argb(&tex_128x128, (AVATAR_SIZE / 2) as usize, 0),
            [hat[3], hat[0], hat[1], hat[2]]
        );
    }

    #[test]
    fn test_bytes_to_avatar_texture_invalid_inputs() {
        assert!(SkinCache::bytes_to_avatar_texture(&[]).is_none());
        assert!(SkinCache::bytes_to_avatar_texture(b"not a valid image").is_none());

        for (w, h) in [
            (32, 32),   // width < 64
            (96, 96),   // width not a multiple of 64
            (64, 31),   // height below 32 * scale
            (64, 48),   // height neither 32 * scale nor 64 * scale
            (64, 100),  // height exceeds 64 * scale
            (128, 32),  // height does not match 2x scale (expects 64 or 128)
            (128, 100), // height does not match 2x scale
        ] {
            let img = image::RgbaImage::from_pixel(w, h, image::Rgba([255, 0, 0, 255]));
            let png = encode_png(&img);
            assert!(
                SkinCache::bytes_to_avatar_texture(&png).is_none(),
                "expected {w}x{h} image to be rejected"
            );
        }
    }

    #[test]
    fn test_read_disk_cache_states() {
        let temp = TempDirGuard::new("read_disk_cache");
        let rt = test_runtime();

        let missing_path = temp.path().join("missing");
        let dir_path = temp.path().join("subdir");
        let oversized_path = temp.path().join("oversized");
        let valid_path = temp.path().join("valid");

        fs::create_dir_all(&dir_path).expect("create subdir");
        let oversized = fs::File::create(&oversized_path).expect("create oversized file");
        oversized
            .set_len(MAX_SKIN_BYTES as u64 + 1)
            .expect("set oversized length");
        fs::write(&valid_path, b"skin-bytes").expect("write valid file");

        rt.block_on(async {
            assert_eq!(SkinCache::read_disk_cache(&missing_path).await, None);
            assert_eq!(SkinCache::read_disk_cache(&dir_path).await, None);
            assert_eq!(SkinCache::read_disk_cache(&oversized_path).await, None);
            assert_eq!(
                SkinCache::read_disk_cache(&valid_path).await,
                Some(b"skin-bytes".to_vec())
            );
        });
    }

    #[test]
    fn test_load_empty_url_and_disk_cache_hit() {
        let temp = TempDirGuard::new("load_disk_hit");
        let cache = SkinCache::new(test_client(), temp.path());
        let rt = test_runtime();

        let url = "http://127.0.0.1:9/offline-cached-skin.png";
        let png = sample_skin_png(64, 64, [100, 150, 200, 255], [0, 0, 0, 0]);
        let disk_path = cache.skin_dir.join(sha256_hex(url));
        fs::create_dir_all(&cache.skin_dir).expect("create skin dir");
        fs::write(&disk_path, &png).expect("write cached skin");

        rt.block_on(async {
            assert!(cache.load("").await.is_none());
            assert!(cache.get("").is_none());

            let texture = cache
                .load(url)
                .await
                .expect("disk cache should load without network");
            assert_eq!(texture.width(), AVATAR_SIZE as i32);
            assert_eq!(texture.height(), AVATAR_SIZE as i32);
            assert!(cache.get(url).is_some());
        });
    }

    #[test]
    fn test_load_downloads_persists_and_hits_memory() {
        let temp = TempDirGuard::new("load_download");
        let cache = SkinCache::new(test_client(), temp.path());
        let rt = test_runtime();

        let png = sample_skin_png(64, 64, [50, 100, 150, 255], [200, 200, 0, 255]);
        let request_count = Arc::new(AtomicUsize::new(0));

        rt.block_on(async {
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind mock server");
            let addr = listener.local_addr().expect("get local addr");
            let url = format!("http://{addr}/skin.png");

            let png_resp = png.clone();
            let count_clone = Arc::clone(&request_count);
            let server = relm4::tokio::spawn(async move {
                if let Ok((mut stream, _)) = listener.accept().await {
                    count_clone.fetch_add(1, Ordering::SeqCst);
                    let _ = read_http_request(&mut stream).await;
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
            assert_eq!(first.width(), AVATAR_SIZE as i32);
            assert_eq!(first.height(), AVATAR_SIZE as i32);

            let disk_path = cache.skin_dir.join(sha256_hex(&url));
            assert_eq!(fs::read(&disk_path).expect("read cached skin"), png);

            let second = cache
                .load(&url)
                .await
                .expect("second load should hit memory cache");
            assert_eq!(second.width(), AVATAR_SIZE as i32);
            assert_eq!(request_count.load(Ordering::SeqCst), 1);

            let _ = server.await;
        });
    }

    #[test]
    fn test_load_corrupt_disk_fallback_and_network_errors() {
        let temp = TempDirGuard::new("load_errors");
        let cache = SkinCache::new(test_client(), temp.path());
        let rt = test_runtime();

        let valid_png = sample_skin_png(64, 64, [20, 40, 60, 255], [0, 0, 0, 0]);
        let bad_dim_png = encode_png(&image::RgbaImage::from_pixel(
            32,
            32,
            image::Rgba([255, 0, 0, 255]),
        ));

        rt.block_on(async {
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind mock server");
            let addr = listener.local_addr().expect("get local addr");

            let url_recover = format!("http://{addr}/recover.png");
            let recover_path = cache.skin_dir.join(sha256_hex(&url_recover));
            fs::create_dir_all(&cache.skin_dir).expect("create skin dir");
            fs::write(&recover_path, b"corrupt-cached-bytes").expect("write corrupt cache");

            let valid_srv = valid_png.clone();
            let bad_dim_srv = bad_dim_png.clone();
            let server = relm4::tokio::spawn(async move {
                for _ in 0..5 {
                    let Ok((mut stream, _)) = listener.accept().await else {
                        break;
                    };
                    let req = read_http_request(&mut stream).await;

                    if req.starts_with("GET /recover.png ") {
                        let header = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            valid_srv.len()
                        );
                        let _ = stream.write_all(header.as_bytes()).await;
                        let _ = stream.write_all(&valid_srv).await;
                    } else if req.starts_with("GET /404 ") {
                        let _ = stream
                            .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                            .await;
                    } else if req.starts_with("GET /empty ") {
                        let _ = stream
                            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                            .await;
                    } else if req.starts_with("GET /corrupt ") {
                        let body = b"not-a-png";
                        let header = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        );
                        let _ = stream.write_all(header.as_bytes()).await;
                        let _ = stream.write_all(body).await;
                    } else if req.starts_with("GET /bad-dim ") {
                        let header = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            bad_dim_srv.len()
                        );
                        let _ = stream.write_all(header.as_bytes()).await;
                        let _ = stream.write_all(&bad_dim_srv).await;
                    }
                    let _ = stream.flush().await;
                }
            });

            // Corrupt disk cache falls back to downloading and overwrites disk cache
            let recovered = cache
                .load(&url_recover)
                .await
                .expect("corrupt disk cache should recover via download");
            assert_eq!(recovered.width(), AVATAR_SIZE as i32);
            assert_eq!(fs::read(&recover_path).expect("read recovered skin"), valid_png);

            for path in ["/404", "/empty", "/corrupt", "/bad-dim"] {
                let url = format!("http://{addr}{path}");
                assert!(
                    cache.load(&url).await.is_none(),
                    "expected {path} to fail loading"
                );
                assert!(cache.get(&url).is_none());
                assert!(!cache.skin_dir.join(sha256_hex(&url)).exists());
            }

            let _ = server.await;
        });
    }
}
