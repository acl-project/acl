// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use sha1::Sha1;
use sha2::{Digest, Sha256};

pub fn url_sha256(url: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(url.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn url_sha1(url: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(url.as_bytes());
    format!("{}.json", hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_cache_key_deterministic_and_sha256_hex() {
        let key1 = url_sha256("https://example.com/icon.png");
        let key2 = url_sha256("https://example.com/icon.png");
        let key3 = url_sha256("https://example.com/other.png");

        assert_eq!(key1, key2);
        assert_ne!(key1, key3);
        assert_eq!(key1.len(), 64);
        assert!(
            key1.chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
        );
    }

    #[test]
    fn test_sha1_cache_key_for_url() {
        let key1 = url_sha1("https://piston-meta.mojang.com/mc/game/version_manifest_v2.json");
        let key2 = url_sha1("https://piston-meta.mojang.com/mc/game/version_manifest_v2.json");
        let key3 = url_sha1("https://example.com/other.json");

        assert_eq!(key1, key2);
        assert_ne!(key1, key3);
        assert!(key1.ends_with(".json"));
        // SHA1 is 40 hex chars + 5 for ".json"
        assert_eq!(key1.len(), 45);
    }
}
