// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

use rand::{RngCore, rngs::OsRng};

use sha2::Digest;
use sha2::Sha256;

#[derive(Clone, PartialEq, Eq)]
pub struct PkcePair {
    /// A random string of 43–128 characters (Base64URL-encoded).
    pub verifier: String,
    /// BASE64URL_NO_PAD(SHA256(verifier))
    pub challenge: String,
}

impl std::fmt::Debug for PkcePair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PkcePair")
            .field("challenge", &self.challenge)
            .field("verifier", &"[REDACTED]")
            .finish()
    }
}

impl PkcePair {
    pub fn generate() -> Self {
        // Use the operating system's secure random source (OsRng) to generate 64 bytes of random entropy.
        let mut random_bytes = [0u8; 64];
        OsRng.fill_bytes(&mut random_bytes);

        let verifier = URL_SAFE_NO_PAD.encode(random_bytes);

        let mut hasher = Sha256::new();

        hasher.update(verifier.as_bytes());
        let hash = hasher.finalize();

        let challenge = URL_SAFE_NO_PAD.encode(hash);

        Self {
            verifier,
            challenge,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pkce_generation_properties() {
        let pkce = PkcePair::generate();

        // RFC 7636: verifier must be between 43 and 128 characters
        assert!((43..=128).contains(&pkce.verifier.len()));
        assert_eq!(pkce.verifier.len(), 86);

        // Verifier must only contain base64url unpadded characters
        assert!(
            pkce.verifier
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        );

        // Challenge must be SHA256 of verifier, Base64URL-encoded (32 bytes = 43 chars unpadded)
        assert_eq!(pkce.challenge.len(), 43);
        let mut hasher = Sha256::new();
        hasher.update(pkce.verifier.as_bytes());
        let expected_challenge = URL_SAFE_NO_PAD.encode(hasher.finalize());
        assert_eq!(pkce.challenge, expected_challenge);
    }

    #[test]
    fn test_pkce_uniqueness() {
        let pkce1 = PkcePair::generate();
        let pkce2 = PkcePair::generate();

        assert_ne!(pkce1.verifier, pkce2.verifier);
        assert_ne!(pkce1.challenge, pkce2.challenge);
    }
}
