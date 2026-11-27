// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use acl_common::hash::url_sha256;
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
        if h < 32 * scale {
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

        let hash = url_sha256(url);
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
            let c_url = url.to_string();
            if let Ok(result) = relm4::tokio::task::spawn_blocking(move || {
                let tex = Self::bytes_to_avatar_texture(&bytes)?;

                if let Err(e) = acl_common::fs::write_atomic(&skin_path, &bytes) {
                    debug!("Failed to save skin {c_url}: {e}");
                }

                Some(tex)
            })
            .await
                && let Some(tex) = result
            {
                let mut memory = self.memory.lock();
                memory.insert(url.to_string(), tex.clone());

                return Some(tex);
            }
        }

        None
    }
}
