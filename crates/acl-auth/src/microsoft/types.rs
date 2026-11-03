// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

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

#[derive(Clone, Serialize, Deserialize)]
pub struct MsTokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: u64,
    pub scope: Option<String>,
    pub refresh_token: Option<String>,
    pub user_id: Option<String>,
}

impl std::fmt::Debug for MsTokenResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MsTokenResponse")
            .field("token_type", &self.token_type)
            .field("expires_in", &self.expires_in)
            .field("scope", &self.scope)
            .field("user_id", &self.user_id)
            .field("access_token", &"[REDACTED]")
            .field("refresh_token", &"[REDACTED]")
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MsTokenError {
    pub error: String,
    pub error_description: Option<String>,
}

#[derive(Serialize)]
pub struct XboxLiveProperties {
    #[serde(rename = "AuthMethod")]
    pub auth_method: String,
    #[serde(rename = "SiteName")]
    pub site_name: String,
    #[serde(rename = "RpsTicket")]
    pub rps_ticket: String,
}

impl std::fmt::Debug for XboxLiveProperties {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("XboxLiveProperties")
            .field("auth_method", &self.auth_method)
            .field("site_name", &self.site_name)
            .field("rps_ticket", &"[REDACTED]")
            .finish()
    }
}

#[derive(Serialize)]
pub struct XboxLiveAuthRequest {
    #[serde(rename = "Properties")]
    pub properties: XboxLiveProperties,
    #[serde(rename = "RelyingParty")]
    pub relying_party: String,
    #[serde(rename = "TokenType")]
    pub token_type: String,
}

impl std::fmt::Debug for XboxLiveAuthRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("XboxLiveAuthRequest")
            .field("properties", &self.properties)
            .field("relying_party", &self.relying_party)
            .field("token_type", &self.token_type)
            .finish()
    }
}

#[derive(Debug, Deserialize)]
pub struct XboxLiveXui {
    pub uhs: String,
}

#[derive(Debug, Deserialize)]
pub struct XboxLiveDisplayClaims {
    pub xui: Vec<XboxLiveXui>,
}

#[derive(Deserialize)]
pub struct XboxLiveAuthResponse {
    #[serde(rename = "Token")]
    pub token: String,
    #[serde(rename = "DisplayClaims")]
    pub display_claims: XboxLiveDisplayClaims,
}

impl std::fmt::Debug for XboxLiveAuthResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("XboxLiveAuthResponse")
            .field("token", &"[REDACTED]")
            .field("display_claims", &self.display_claims)
            .finish()
    }
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

#[derive(Serialize)]
pub struct XstsAuthRequest {
    #[serde(rename = "Properties")]
    pub properties: XstsProperties,
    #[serde(rename = "RelyingParty")]
    pub relying_party: String,
    #[serde(rename = "TokenType")]
    pub token_type: String,
}

impl std::fmt::Debug for XstsAuthRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("XstsAuthRequest")
            .field("properties", &self.properties)
            .field("relying_party", &self.relying_party)
            .field("token_type", &self.token_type)
            .finish()
    }
}

#[derive(Serialize)]
pub struct XstsProperties {
    #[serde(rename = "SandboxId")]
    pub sandbox_id: String,
    #[serde(rename = "UserTokens")]
    pub user_tokens: Vec<String>,
}

impl std::fmt::Debug for XstsProperties {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("XstsProperties")
            .field("sandbox_id", &self.sandbox_id)
            .field("user_tokens", &"[REDACTED]")
            .finish()
    }
}

#[derive(Serialize)]
pub struct MinecraftLoginRequest {
    #[serde(rename = "identityToken")]
    pub identity_token: String,
}

impl std::fmt::Debug for MinecraftLoginRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MinecraftLoginRequest")
            .field("identity_token", &"[REDACTED]")
            .finish()
    }
}

#[derive(Deserialize)]
pub struct MinecraftLoginResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: u64,
}

impl std::fmt::Debug for MinecraftLoginResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MinecraftLoginResponse")
            .field("token_type", &self.token_type)
            .field("expires_in", &self.expires_in)
            .field("access_token", &"[REDACTED]")
            .finish()
    }
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
#[derive(Clone, Serialize, Deserialize)]
pub struct MicrosoftTokenBundle {
    /// Microsoft long-lived refresh token (valid for 90 days).
    pub refresh_token: String,
    /// Minecraft game access token (valid for 24 hours).
    pub access_token: String,
    /// Expiration time point of the access token.
    pub expires_at: i64,
}

// Manually implement Debug to avoid printing sensitive information.
impl std::fmt::Debug for MicrosoftTokenBundle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MicrosoftTokenBundle")
            .field("refresh_token", &"[REDACTED]")
            .field("access_token", &"[REDACTED]")
            .field("expires_at", &self.expires_at)
            .finish()
    }
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

#[derive(Debug, Clone)]
pub struct MicrosoftLoginResult {
    pub account: Account,
    pub tokens: MicrosoftTokenBundle,
}

#[derive(Debug, Clone)]
pub struct MicrosoftRefreshResult {
    pub tokens: MicrosoftTokenBundle,
    pub username: String,
    pub uuid: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_bundle_is_expired() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        // Valid for 1 hour (> 300s buffer) -> not expired
        let future_bundle = MicrosoftTokenBundle {
            refresh_token: "ref".into(),
            access_token: "acc".into(),
            expires_at: now.saturating_add(3600),
        };
        assert!(!future_bundle.is_expired());

        // Expires in 200 seconds (within 300s buffer) -> considered expired
        let buffer_bundle = MicrosoftTokenBundle {
            refresh_token: "ref".into(),
            access_token: "acc".into(),
            expires_at: now.saturating_add(200),
        };
        assert!(buffer_bundle.is_expired());

        // Already expired in the past -> expired
        let past_bundle = MicrosoftTokenBundle {
            refresh_token: "ref".into(),
            access_token: "acc".into(),
            expires_at: now.saturating_sub(100),
        };
        assert!(past_bundle.is_expired());
    }

    #[test]
    fn test_serde_ms_token_response() {
        let json = r#"{
            "access_token": "ms_access_123",
            "token_type": "Bearer",
            "expires_in": 3600,
            "scope": "XboxLive.signin offline_access",
            "refresh_token": "ms_refresh_456",
            "user_id": "user_789"
        }"#;

        let res: MsTokenResponse = serde_json::from_str(json).unwrap();
        assert_eq!(res.access_token, "ms_access_123");
        assert_eq!(res.expires_in, 3600);
        assert_eq!(res.refresh_token.as_deref(), Some("ms_refresh_456"));
    }

    #[test]
    fn test_serde_xbox_responses() {
        let auth_json = r#"{
            "IssueInstant": "2026-11-02T12:00:00Z",
            "NotAfter": "2026-11-03T12:00:00Z",
            "Token": "xbl_token_xyz",
            "DisplayClaims": {
                "xui": [
                    { "uhs": "123456789" }
                ]
            }
        }"#;

        let res: XboxLiveAuthResponse = serde_json::from_str(auth_json).unwrap();
        assert_eq!(res.token, "xbl_token_xyz");
        assert_eq!(res.display_claims.xui.len(), 1);
        assert_eq!(res.display_claims.xui[0].uhs, "123456789");

        let err_json = r#"{
            "Identity": "0",
            "XErr": 2148916238,
            "Message": "Child account",
            "Redirect": "https://start.ui.xboxlive.com"
        }"#;

        let err_res: XboxErrorResponse = serde_json::from_str(err_json).unwrap();
        assert_eq!(err_res.xerr, Some(2148916238));
        assert_eq!(err_res.message.as_deref(), Some("Child account"));
    }

    #[test]
    fn test_serde_minecraft_responses() {
        let login_json = r#"{
            "username": "4566e69fc90748ee8d71d7ba5aaafced",
            "roles": [],
            "access_token": "mc_token_abc",
            "token_type": "Bearer",
            "expires_in": 86400
        }"#;
        let login_res: MinecraftLoginResponse = serde_json::from_str(login_json).unwrap();
        assert_eq!(login_res.access_token, "mc_token_abc");
        assert_eq!(login_res.expires_in, 86400);

        let profile_json = r#"{
            "id": "4566e69fc90748ee8d71d7ba5aaafced",
            "name": "Steve"
        }"#;
        let profile_res: MinecraftProfileResponse = serde_json::from_str(profile_json).unwrap();
        assert_eq!(profile_res.id, "4566e69fc90748ee8d71d7ba5aaafced");
        assert_eq!(profile_res.name, "Steve");
    }

    #[test]
    fn test_microsoft_login_and_refresh_result_derive() {
        let account = Account::new_offline("Steve".to_string()).unwrap();
        let tokens = MicrosoftTokenBundle {
            refresh_token: "ref_123".into(),
            access_token: "acc_123".into(),
            expires_at: 1234567890,
        };
        let login_res = MicrosoftLoginResult {
            account,
            tokens: tokens.clone(),
        };
        let login_clone = login_res.clone();
        assert_eq!(login_clone.account.username, "Steve");
        assert_eq!(login_clone.tokens.refresh_token, "ref_123");
        let debug_str = format!("{login_res:?}");
        assert!(debug_str.contains("MicrosoftLoginResult"));
        assert!(debug_str.contains("Steve"));

        let refresh_res = MicrosoftRefreshResult {
            tokens,
            username: "Steve".to_string(),
            uuid: "4566e69fc90748ee8d71d7ba5aaafced".to_string(),
        };
        let refresh_clone = refresh_res.clone();
        assert_eq!(refresh_clone.username, "Steve");
        assert_eq!(refresh_clone.uuid, "4566e69fc90748ee8d71d7ba5aaafced");
        let refresh_debug = format!("{refresh_res:?}");
        assert!(refresh_debug.contains("MicrosoftRefreshResult"));
    }

    #[test]
    fn test_auth_request_types_debug_redaction() {
        let xbl_props = XboxLiveProperties {
            auth_method: "RPS".to_string(),
            site_name: "user.auth.xboxlive.com".to_string(),
            rps_ticket: "secret_rps_ticket".to_string(),
        };
        let xbl_props_debug = format!("{xbl_props:?}");
        assert!(!xbl_props_debug.contains("secret_rps_ticket"));
        assert!(xbl_props_debug.contains("[REDACTED]"));
        assert!(xbl_props_debug.contains("user.auth.xboxlive.com"));

        let xbl_req = XboxLiveAuthRequest {
            properties: xbl_props,
            relying_party: "http://auth.xboxlive.com".to_string(),
            token_type: "JWT".to_string(),
        };
        let xbl_req_debug = format!("{xbl_req:?}");
        assert!(!xbl_req_debug.contains("secret_rps_ticket"));
        assert!(xbl_req_debug.contains("[REDACTED]"));
        assert!(xbl_req_debug.contains("http://auth.xboxlive.com"));

        let xsts_props = XstsProperties {
            sandbox_id: "RETAIL".to_string(),
            user_tokens: vec!["secret_token_1".to_string(), "secret_token_2".to_string()],
        };
        let xsts_props_debug = format!("{xsts_props:?}");
        assert!(!xsts_props_debug.contains("secret_token_1"));
        assert!(!xsts_props_debug.contains("secret_token_2"));
        assert!(xsts_props_debug.contains("[REDACTED]"));
        assert!(xsts_props_debug.contains("RETAIL"));

        let xsts_req = XstsAuthRequest {
            properties: xsts_props,
            relying_party: "rp".to_string(),
            token_type: "JWT".to_string(),
        };
        let xsts_req_debug = format!("{xsts_req:?}");
        assert!(!xsts_req_debug.contains("secret_token_1"));
        assert!(xsts_req_debug.contains("[REDACTED]"));

        let mc_req = MinecraftLoginRequest {
            identity_token: "secret_identity_token".to_string(),
        };
        let mc_req_debug = format!("{mc_req:?}");
        assert!(!mc_req_debug.contains("secret_identity_token"));
        assert!(mc_req_debug.contains("[REDACTED]"));
    }
}
