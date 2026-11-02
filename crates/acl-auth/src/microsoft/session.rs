// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashMap;
use std::time::Duration;

use rand::RngCore;
use reqwest::Url;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

use crate::AuthError;
use crate::Result;
use crate::microsoft::pkce::PkcePair;
use crate::microsoft::types::*;

pub struct MicrosoftLoginSession {
    pub auth_url: Url,
    pub redirect_uri: String,
    pub state: String,
    pub pkce: PkcePair,
    pub client_id: String,
    pub listener: tokio::net::TcpListener,
}

impl MicrosoftLoginSession {
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
            client_id: client_id.to_string(),
            listener,
        })
    }

    async fn accept_once(&self) -> Result<Option<String>> {
        let (mut stream, _) = self.listener.accept().await?;

        let mut buf = [0u8; 4096];
        let n = stream.read(&mut buf).await?;
        let request_str =
            String::from_utf8_lossy(buf.get(..n).ok_or(AuthError::InvalidCallbackRequest {
                reason: "Stream size is not correct".to_owned(),
            })?);

        let first_line = request_str.lines().next().unwrap_or_default();
        let mut parts = first_line.split_whitespace();
        let method = parts.next().unwrap_or_default();
        let uri = parts.next().unwrap_or("/");

        // Directly return 405 for non-GET requests and ignore them.
        if method != "GET" {
            let _ = stream
                .write_all(b"HTTP/1.1 405 Method Not Allowed\r\nConnection: close\r\n\r\n")
                .await;
            return Ok(None);
        }

        let parsed_url = Url::parse(&format!("http://localhost{uri}"))?;
        // Return 404 for non-/callback requests and ignore them, returning Ok(None).
        if parsed_url.path() != "/callback" {
            let _ = stream
                .write_all(b"HTTP/1.1 404 Not Found\r\nConnection: close\r\n\r\n")
                .await;
            return Ok(None);
        }

        let query: HashMap<String, String> = parsed_url.query_pairs().into_owned().collect();

        if let Some(err) = query.get("error") {
            let desc = query.get("error_description").cloned().unwrap_or_default();
            let _ = stream
                .write_all(b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\nLogin failed")
                .await;
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

        Ok(Some(code))
    }

    /// Wait for the callback; if none arrives after 180 seconds, treat it as a timeout.
    pub async fn wait_for_callback(&self) -> Result<String> {
        let timeout = tokio::time::sleep(Duration::from_secs(180));
        tokio::pin!(timeout);

        loop {
            tokio::select! {
                _ = &mut timeout => {
                    return Err(AuthError::OAuthTimeout);
                }
                res = self.accept_once() => {
                    if let Some(code) = res? {
                        return Ok(code);
                    }
                }

            }
        }
    }

    pub async fn exchange_code_for_ms_token(
        &self,
        client: &reqwest::Client,
        token_url: &str,
        code: &str,
    ) -> Result<MsTokenResponse> {
        let params = [
            ("client_id", self.client_id.clone()),
            ("grant_type", "authorization_code".to_owned()),
            ("code", code.to_owned()),
            ("redirect_uri", self.redirect_uri.clone()),
            ("code_verifier", self.pkce.verifier.clone()),
        ];

        let res = client.post(token_url).form(&params).send().await?;

        let status = res.status();
        if !status.is_success() {
            let status_code = status.as_u16();
            let body_text = res.text().await.unwrap_or_default();

            let message = if let Ok(err) = serde_json::from_str::<MsTokenError>(&body_text) {
                match err.error_description {
                    Some(desc) => format!("{}: {desc}", err.error),
                    None => err.error,
                }
            } else if !body_text.trim().is_empty() {
                body_text
            } else {
                status
                    .canonical_reason()
                    .unwrap_or("Unknown error")
                    .to_string()
            };

            return Err(AuthError::TokenRequestFailed {
                status: status_code,
                message,
            });
        }

        let token: MsTokenResponse = res.json().await?;

        Ok(token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    #[test]
    fn test_build_authorize_url() {
        let url = MicrosoftLoginSession::build_authorize_url(
            "test_client_id",
            "http://localhost:1234/callback",
            "test_state",
            "test_challenge",
        )
        .unwrap();

        assert_eq!(url.scheme(), "https");
        assert_eq!(url.host_str(), Some("login.live.com"));
        assert_eq!(url.path(), "/oauth20_authorize.srf");

        let pairs: HashMap<String, String> = url.query_pairs().into_owned().collect();
        assert_eq!(
            pairs.get("client_id").map(|s| s.as_str()),
            Some("test_client_id")
        );
        assert_eq!(pairs.get("response_type").map(|s| s.as_str()), Some("code"));
        assert_eq!(
            pairs.get("redirect_uri").map(|s| s.as_str()),
            Some("http://localhost:1234/callback")
        );
        assert_eq!(
            pairs.get("scope").map(|s| s.as_str()),
            Some("XboxLive.signin offline_access")
        );
        assert_eq!(pairs.get("state").map(|s| s.as_str()), Some("test_state"));
        assert_eq!(
            pairs.get("code_challenge").map(|s| s.as_str()),
            Some("test_challenge")
        );
        assert_eq!(
            pairs.get("code_challenge_method").map(|s| s.as_str()),
            Some("S256")
        );
    }

    #[test]
    fn test_build_state() {
        let state1 = MicrosoftLoginSession::build_state();
        let state2 = MicrosoftLoginSession::build_state();

        assert_eq!(state1.len(), 32);
        assert!(state1.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(state1, state2);
    }

    #[tokio::test]
    async fn test_callback_flow_success() {
        let session = MicrosoftLoginSession::start("dummy_client_id")
            .await
            .unwrap();
        let port = session.listener.local_addr().unwrap().port();
        let state = session.state.clone();

        let client_task = tokio::spawn(async move {
            let mut stream = TcpStream::connect(format!("127.0.0.1:{port}"))
                .await
                .unwrap();
            let req = format!(
                "GET /callback?code=mock_code_123&state={state} HTTP/1.1\r\nHost: localhost\r\n\r\n"
            );
            stream.write_all(req.as_bytes()).await.unwrap();

            let mut resp = [0u8; 1024];
            let n = stream.read(&mut resp).await.unwrap();
            let resp_bytes = resp.get(..n).unwrap_or_default();
            let resp_str = String::from_utf8_lossy(resp_bytes);
            assert!(resp_str.starts_with("HTTP/1.1 200 OK"));
        });

        let code = session.wait_for_callback().await.unwrap();
        assert_eq!(code, "mock_code_123");
        client_task.await.unwrap();
    }

    #[tokio::test]
    async fn test_callback_flow_ignores_favicon_and_succeeds() {
        let session = MicrosoftLoginSession::start("dummy_client_id")
            .await
            .unwrap();
        let port = session.listener.local_addr().unwrap().port();
        let state = session.state.clone();

        let client_task = tokio::spawn(async move {
            // Send favicon request
            let mut stream1 = TcpStream::connect(format!("127.0.0.1:{port}"))
                .await
                .unwrap();
            stream1
                .write_all(b"GET /favicon.ico HTTP/1.1\r\nHost: localhost\r\n\r\n")
                .await
                .unwrap();
            let mut resp1 = [0u8; 256];
            let n1 = stream1.read(&mut resp1).await.unwrap();
            let resp1_bytes = resp1.get(..n1).unwrap_or_default();
            let resp1_str = String::from_utf8_lossy(resp1_bytes);
            assert!(resp1_str.starts_with("HTTP/1.1 404 Not Found"));
            drop(stream1);

            // Send actual callback
            let mut stream2 = TcpStream::connect(format!("127.0.0.1:{port}"))
                .await
                .unwrap();
            let req = format!(
                "GET /callback?code=real_code_456&state={state} HTTP/1.1\r\nHost: localhost\r\n\r\n"
            );
            stream2.write_all(req.as_bytes()).await.unwrap();
        });

        let code = session.wait_for_callback().await.unwrap();
        assert_eq!(code, "real_code_456");
        client_task.await.unwrap();
    }

    #[tokio::test]
    async fn test_callback_flow_state_mismatch() {
        let session = MicrosoftLoginSession::start("dummy_client_id")
            .await
            .unwrap();
        let port = session.listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            let mut stream = TcpStream::connect(format!("127.0.0.1:{port}"))
                .await
                .unwrap();
            let req = "GET /callback?code=mock_code&state=wrong_state HTTP/1.1\r\nHost: localhost\r\n\r\n";
            let _ = stream.write_all(req.as_bytes()).await;
        });

        let err = session.wait_for_callback().await.unwrap_err();
        assert!(matches!(err, AuthError::StateMismatch { .. }));
    }

    #[tokio::test]
    async fn test_callback_flow_error_response() {
        let session = MicrosoftLoginSession::start("dummy_client_id")
            .await
            .unwrap();
        let port = session.listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            let mut stream = TcpStream::connect(format!("127.0.0.1:{port}"))
                .await
                .unwrap();
            let req = "GET /callback?error=access_denied&error_description=User+cancelled HTTP/1.1\r\nHost: localhost\r\n\r\n";
            let _ = stream.write_all(req.as_bytes()).await;
        });

        let err = session.wait_for_callback().await.unwrap_err();
        assert!(matches!(err, AuthError::OAuthCallbackError { .. }));
    }
}
