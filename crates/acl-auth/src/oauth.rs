use std::collections::HashMap;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

use rand::{RngCore, rngs::OsRng};
use reqwest::Response;
use reqwest::Url;
use serde::Deserialize;
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

use crate::AuthError;
use crate::Result;

// What a terrible company!
// It’s comparable to the Journey to the West.
pub const MICROSOFT_OAUTH_AUTHORIZE_URL: &str = "https://login.live.com/oauth20_authorize.srf";
pub const MICROSOFT_OAUTH_TOKEN_URL: &str = "https://login.live.com/oauth20_token.srf";
pub const XBOXLIVE_AUTHORIZE_URL: &str = "https://user.auth.xboxlive.com/user/authenticate";
pub const XSTS_AUTHORIZE_URL: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";

pub const MINECRAFT_LOGIN_URL: &str =
    "https://api.minecraftservices.com/authentication/login_with_xbox";
pub const MINECRAFT_PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";

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
    pub client_id: String,
    pub listener: tokio::net::TcpListener,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MsTokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: u64,
    pub scope: Option<String>,
    pub refresh_token: Option<String>,
    pub user_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MsTokenError {
    pub error: String,
    pub error_description: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct XboxLiveProperties {
    #[serde(rename = "AuthMethod")]
    pub auth_method: String,
    #[serde(rename = "SiteName")]
    pub site_name: String,
    #[serde(rename = "RpsTicket")]
    pub rps_ticket: String,
}

#[derive(Debug, Serialize)]
pub struct XboxLiveAuthRequest {
    #[serde(rename = "Properties")]
    pub properties: XboxLiveProperties,
    #[serde(rename = "RelyingParty")]
    pub relying_party: String,
    #[serde(rename = "TokenType")]
    pub token_type: String,
}

#[derive(Debug, Deserialize)]
pub struct XboxLiveXui {
    pub uhs: String,
}

#[derive(Debug, Deserialize)]
pub struct XboxLiveDisplayClaims {
    pub xui: Vec<XboxLiveXui>,
}

#[derive(Debug, Deserialize)]
pub struct XboxLiveAuthResponse {
    #[serde(rename = "Token")]
    pub token: String,
    #[serde(rename = "DisplayClaims")]
    pub display_claims: XboxLiveDisplayClaims,
}

#[derive(Debug, Deserialize)]
pub struct XboxErrorResponse {
    #[serde(rename = "XErr")]
    pub xerr: Option<u64>,
    #[serde(rename = "Message")]
    pub message: Option<String>,
    #[serde(rename = "Redirect")]
    pub redirect: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct XstsAuthRequest {
    #[serde(rename = "Properties")]
    pub properties: XstsProperties,
    #[serde(rename = "RelyingParty")]
    pub relying_party: String,
    #[serde(rename = "TokenType")]
    pub token_type: String,
}

#[derive(Debug, Serialize)]
pub struct XstsProperties {
    #[serde(rename = "SandboxId")]
    pub sandbox_id: String,
    #[serde(rename = "UserTokens")]
    pub user_tokens: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct MinecraftLoginRequest {
    #[serde(rename = "identityToken")]
    pub identity_token: String,
}

#[derive(Debug, Deserialize)]
pub struct MinecraftLoginResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: u64,
}

#[derive(Debug, Deserialize)]
pub struct MinecraftErrorResponse {
    pub error: Option<String>,
    #[serde(rename = "errorMessage")]
    pub error_message: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct MinecraftProfileResponse {
    pub id: String,
    pub name: String,
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
            client_id: client_id.to_string(),
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

            let messgae = if let Ok(err) = serde_json::from_str::<MsTokenError>(&body_text) {
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
                message: messgae,
            });
        }

        let token: MsTokenResponse = res.json().await?;

        Ok(token)
    }

    async fn process_xbox_response(res: Response) -> Result<XboxLiveAuthResponse> {
        let status = res.status();
        if !status.is_success() {
            let body_text = res.text().await.unwrap_or_default();

            let message = if let Ok(err) = serde_json::from_str::<XboxErrorResponse>(&body_text) {
                match err.xerr {
                    Some(2148916233) => "Account does not have an Xbox profile. Please visit xbox.com to create one.".to_string(),
                Some(2148916238) => "Child account requires parental consent in a Microsoft Family.".to_string(),
                Some(2148916235) => "Xbox Live is not available in your country/region.".to_string(),
                Some(code) => format!("Xbox error (XErr: {code}): {}", err.message.unwrap_or_default()),
                None => err.message.unwrap_or(body_text),
                }
            } else {
                status
                    .canonical_reason()
                    .unwrap_or("Unknown error")
                    .to_string()
            };

            return Err(AuthError::TokenRequestFailed {
                status: status.as_u16(),
                message,
            });
        }

        let xbox_res: XboxLiveAuthResponse = res.json().await?;

        Ok(xbox_res)
    }

    pub async fn exchange_msa_for_xbox_token(
        client: &reqwest::Client,
        url: &str,
        access_token: &str,
    ) -> Result<XboxLiveAuthResponse> {
        let properties = XboxLiveProperties {
            auth_method: "RPS".to_string(),
            site_name: "user.auth.xboxlive.com".to_string(),
            rps_ticket: format!("d={access_token}"),
        };

        let req = XboxLiveAuthRequest {
            properties,
            relying_party: "http://auth.xboxlive.com".to_string(),
            token_type: "JWT".to_string(),
        };

        let res = client.post(url).json(&req).send().await?;

        Self::process_xbox_response(res).await
    }

    pub async fn exchange_xbox_token_for_xsts_token(
        client: &reqwest::Client,
        url: &str,
        xbl_token: &str,
    ) -> Result<XboxLiveAuthResponse> {
        let properties = XstsProperties {
            sandbox_id: "RETAIL".to_string(),
            user_tokens: vec![xbl_token.to_string()],
        };

        let req = XstsAuthRequest {
            properties,
            relying_party: "rp://api.minecraftservices.com/".to_string(),
            token_type: "JWT".to_string(),
        };

        let res = client.post(url).json(&req).send().await?;

        Self::process_xbox_response(res).await
    }

    pub async fn exchange_xsts_for_minecraft_token(
        client: &reqwest::Client,
        url: &str,
        uhs: &str,
        xsts_token: &str,
    ) -> Result<MinecraftLoginResponse> {
        let req = MinecraftLoginRequest {
            identity_token: format!("XBL3.0 x={uhs};{xsts_token}"),
        };

        let res = client.post(url).json(&req).send().await?;

        let status = res.status();

        if !status.is_success() {
            let body_text = res.text().await.unwrap_or_default();

            let message =
                if let Ok(err) = serde_json::from_str::<MinecraftErrorResponse>(&body_text) {
                    match err.error_message {
                        Some(desc) => format!("{:?}:{desc}", err.error),
                        None => "Unknown error".to_string(),
                    }
                } else {
                    body_text
                };

            return Err(AuthError::TokenRequestFailed {
                status: status.as_u16(),
                message,
            });
        }

        let token: MinecraftLoginResponse = res.json().await?;

        Ok(token)
    }

    pub async fn fetch_minecraft_profile(
        client: &reqwest::Client,
        url: &str,
        token: &str,
    ) -> Result<MinecraftProfileResponse> {
        let res = client.get(url).bearer_auth(token).send().await?;
        let status = res.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(AuthError::TokenRequestFailed {
                    status: 404,
                    message: "This account has not purchased Minecraft Java Edition or has not created a profile.".to_string(),
                });
        }

        if !status.is_success() {
            let body_text = res.text().await.unwrap_or_default();
            return Err(AuthError::TokenRequestFailed {
                status: status.as_u16(),
                message: body_text,
            });
        }

        let profile: MinecraftProfileResponse = res.json().await?;
        Ok(profile)
    }
}
