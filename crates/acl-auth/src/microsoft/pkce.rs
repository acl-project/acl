use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

use rand::{RngCore, rngs::OsRng};

use sha2::Digest;
use sha2::Sha256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PkcePair {
    /// A random string of 43–128 characters (Base64URL-encoded).
    pub verifier: String,
    /// BASE64URL_NO_PAD(SHA256(verifier))
    pub challenge: String,
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
