use std::collections::HashMap;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

use rand::{RngCore, rngs::OsRng};
use reqwest::Url;
use sha2::Digest;
use sha2::Sha256;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

use crate::AuthError;
use crate::Result;

pub const MICROSOFT_OAUTH_AUTHORIZE_URL: &str = "https://login.live.com/oauth20_authorize.srf";

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

pub struct AuthSession {
    pub auth_url: Url,
    pub redirect_uri: String,
    pub state: String,
    pub pkce: PkcePair,
    pub listener: tokio::net::TcpListener,
}

impl AuthSession {
    pub fn build_authorize_url(
        client_id: &str,
        redirect_uri: &str,
        state: &str,
        code_challenge: &str,
    ) -> Result<Url> {
        let mut url = Url::parse(MICROSOFT_OAUTH_AUTHORIZE_URL)?;
        url.query_pairs_mut()
            .append_pair("client_id", client_id)
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("scope", "XboxLive.signin offline_access")
            .append_pair("state", state)
            .append_pair("code_challenge", code_challenge)
            .append_pair("code_challenge_method", "S256");

        Ok(url)
    }

    // Generate a 16-byte secure random hex string as the state.
    pub fn build_state() -> String {
        let mut state_bytes = [0u8; 16];
        rand::rngs::OsRng.fill_bytes(&mut state_bytes);
        state_bytes.iter().map(|b| format!("{:02x}", b)).collect()
    }

    pub async fn start(client_id: &str) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let redirect_uri = format!("http://localhost:{port}/callback");

        let pkce = PkcePair::generate();
        let state = Self::build_state();

        let auth_url =
            Self::build_authorize_url(client_id, &redirect_uri, &state, &pkce.challenge)?;

        Ok(Self {
            auth_url,
            redirect_uri,
            state,
            pkce,
            listener,
        })
    }

    pub async fn wait_for_callback(&self) -> Result<String> {
        let (mut stream, _) = self.listener.accept().await?;

        let mut buf = [0u8; 4096];
        let n = stream.read(&mut buf).await?;
        let request_str = String::from_utf8_lossy(&buf[..n]);

        let first_line = request_str.lines().next().unwrap_or_default();
        let mut parts = first_line.split_whitespace();
        let method = parts.next().unwrap_or_default();
        let uri = parts.next().unwrap_or("/");

        // Any non-GET method is directly invalid.
        if method != "GET" {
            return Err(AuthError::InvalidCallbackRequest {
                reason: format!("Expected HTTP GET, got {method}"),
            });
        }

        let parsed_url = Url::parse(&format!("http://localhost{uri}"))?;
        // Must be "/callback".
        if parsed_url.path() != "/callback" {
            return Err(AuthError::InvalidCallbackRequest {
                reason: format!("Unexpected path: {}", parsed_url.path()),
            });
        }

        let query: HashMap<String, String> = parsed_url.query_pairs().into_owned().collect();

        if let Some(err) = query.get("error") {
            let desc = query.get("error_description").cloned().unwrap_or_default();
            return Err(AuthError::OAuthCallbackError {
                error: err.clone(),
                description: desc,
            });
        }

        match query.get("state") {
            Some(state) if state == &self.state => {}
            Some(state) => {
                return Err(AuthError::StateMismatch {
                    expected: self.state.clone(),
                    actual: state.clone(),
                });
            }
            None => {
                return Err(AuthError::InvalidCallbackRequest {
                    reason: "Missing 'state' parameter in callback".into(),
                });
            }
        }

        let code = query
            .get("code")
            .cloned()
            .ok_or_else(|| AuthError::InvalidCallbackRequest {
                reason: "Missing 'code' parameter in callback".into(),
            })?;

        // Return a simple HTTP page to let the user return to the launcher.
        let body = "<html><body><h2>Login successful!</h2><p>You can close this tab now.</p></body></html>";
        let response = format!(
            "HTTP/1.1 200 OK\r\n\
         Content-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\r\n\
         {}",
            body.len(),
            body
        );

        let _ = stream.write_all(response.as_bytes()).await;
        let _ = stream.flush().await;

        Ok(code)
    }
}
