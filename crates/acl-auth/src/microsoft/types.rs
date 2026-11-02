use serde::Deserialize;
use serde::Serialize;

use crate::Account;

pub const MICROSOFT_OAUTH_AUTHORIZE_URL: &str = "https://login.live.com/oauth20_authorize.srf";
pub const MICROSOFT_OAUTH_TOKEN_URL: &str = "https://login.live.com/oauth20_token.srf";
pub const XBOXLIVE_AUTHORIZE_URL: &str = "https://user.auth.xboxlive.com/user/authenticate";
pub const XSTS_AUTHORIZE_URL: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";

pub const MINECRAFT_LOGIN_URL: &str =
    "https://api.minecraftservices.com/authentication/login_with_xbox";
pub const MINECRAFT_PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";

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

/// Authentication credentials
/// Must be stored securely and never printed.
#[derive(Serialize, Deserialize)]
pub struct MicrosoftTokenBundle {
    /// Microsoft long-lived refresh token (valid for 90 days).
    pub refresh_token: String,
    /// Minecraft game access token (valid for 24 hours).
    pub access_token: String,
    /// Expiration time point of the access token.
    pub expires_at: i64,
}

impl MicrosoftTokenBundle {
    /// Determine whether the access token has expired, with a 5-minute buffer.
    pub fn is_expired(&self) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        self.expires_at <= now.saturating_add(300)
    }
}

pub struct MicrosoftLoginResult {
    pub account: Account,
    pub tokens: MicrosoftTokenBundle,
}

pub struct MicrosoftRefreshResult {
    pub tokens: MicrosoftTokenBundle,
    pub username: String,
}
