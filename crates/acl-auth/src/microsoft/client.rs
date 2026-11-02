use reqwest::Response;

use crate::AuthError;
use crate::Result;
use crate::microsoft::types::*;

async fn process_xbox_response(res: Response) -> Result<XboxLiveAuthResponse> {
    let status = res.status();
    if !status.is_success() {
        let body_text = res.text().await.unwrap_or_default();

        let message = if let Ok(err) = serde_json::from_str::<XboxErrorResponse>(&body_text) {
            match err.xerr {
                Some(2148916233) => {
                    "Account does not have an Xbox profile. Please visit xbox.com to create one."
                        .to_string()
                }
                Some(2148916238) => {
                    "Child account requires parental consent in a Microsoft Family.".to_string()
                }
                Some(2148916235) => {
                    "Xbox Live is not available in your country/region.".to_string()
                }
                Some(code) => format!(
                    "Xbox error (XErr: {code}): {}",
                    err.message.unwrap_or_default()
                ),
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

    process_xbox_response(res).await
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

    process_xbox_response(res).await
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

        let message = if let Ok(err) = serde_json::from_str::<MinecraftErrorResponse>(&body_text) {
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

pub async fn refresh_ms_token(
    client: &reqwest::Client,
    token_url: &str,
    client_id: &str,
    refresh_token: &str,
) -> Result<MsTokenResponse> {
    let params = [
        ("client_id", client_id),
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("scope", "XboxLive.signin offline_access"),
    ];

    let res = client.post(token_url).form(&params).send().await?;

    let status = res.status();
    if !status.is_success() {
        let body_text = res.text().await.unwrap_or_default();

        // Check whether the refresh token has expired.
        if status == reqwest::StatusCode::BAD_REQUEST
            && let Ok(err) = serde_json::from_str::<MsTokenError>(&body_text)
            && err.error == "invalid_grant"
        {
            return Err(AuthError::RefreshTokenExpired);
        }

        return Err(AuthError::TokenRequestFailed {
            status: status.as_u16(),
            message: body_text,
        });
    }

    let token: MsTokenResponse = res.json().await?;

    Ok(token)
}
