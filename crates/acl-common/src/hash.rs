// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use sha1::Sha1;
use sha2::{Digest, Sha256};

/// Returns the lowercase hex-encoded SHA-256 digest of the input bytes.
pub fn sha256_hex(input: impl AsRef<[u8]>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_ref());
    hex::encode(hasher.finalize())
}

/// Returns the lowercase hex-encoded SHA-1 digest of the input bytes.
pub fn sha1_hex(input: impl AsRef<[u8]>) -> String {
    let mut hasher = Sha1::new();
    hasher.update(input.as_ref());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_sha256_hex_deterministic_and_known_vector() {
        let key1 = sha256_hex("https://example.com/icon.png");
        let key2 = sha256_hex("https://example.com/icon.png");
        let key3 = sha256_hex("https://example.com/other.png");

        assert_eq!(key1, key2);
        assert_ne!(key1, key3);
        assert_eq!(key1.len(), 64);
        assert!(
            key1.chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
        );

        // Known SHA-256 test vector for b"hello"
        assert_eq!(
            sha256_hex(b"hello"),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn test_sha1_hex_deterministic_and_known_vector() {
        let key1 = sha1_hex("https://piston-meta.mojang.com/mc/game/version_manifest_v2.json");
        let key2 = sha1_hex("https://piston-meta.mojang.com/mc/game/version_manifest_v2.json");
        let key3 = sha1_hex("https://example.com/other.json");

        assert_eq!(key1, key2);
        assert_ne!(key1, key3);
        // SHA-1 is 40 lowercase hex chars
        assert_eq!(key1.len(), 40);
        assert!(
            key1.chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
        );

        // Known SHA-1 test vector for b"hello"
        assert_eq!(
            sha1_hex(b"hello"),
            "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
        );
    }
}
