pub mod client;
pub mod pkce;
pub mod session;
pub mod types;

pub use types::{MicrosoftLoginResult, MicrosoftRefreshResult};

use crate::{
    Account, AuthError, Result,
    microsoft::{
        client::{
            exchange_msa_for_xbox_token, exchange_xbox_token_for_xsts_token,
            exchange_xsts_for_minecraft_token, fetch_minecraft_profile,
        },
        session::MicrosoftLoginSession,
        types::*,
    },
};
pub struct MicrosoftAuth;

impl MicrosoftAuth {
    /// Perform the login operation; the caller must first create a session and open the browser.
    pub async fn login(
        session: MicrosoftLoginSession,
        client: &reqwest::Client,
    ) -> Result<MicrosoftLoginResult> {
        let code = session.wait_for_callback().await?;

        let ms_token = session
            .exchange_code_for_ms_token(client, MICROSOFT_OAUTH_TOKEN_URL, &code)
            .await?;

        let xbox_token =
            exchange_msa_for_xbox_token(client, XBOXLIVE_AUTHORIZE_URL, &ms_token.access_token)
                .await?;

        let xsts_token =
            exchange_xbox_token_for_xsts_token(client, XSTS_AUTHORIZE_URL, &xbox_token.token)
                .await?;

        let uhs = xsts_token
            .display_claims
            .xui
            .into_iter()
            .next()
            .map(|x| x.uhs)
            .ok_or_else(|| AuthError::TokenRequestFailed {
                status: 200,
                message: "Missing 'uhs' in Xbox Live response".into(),
            })?;

        let minecraft_token =
            exchange_xsts_for_minecraft_token(client, MINECRAFT_LOGIN_URL, &uhs, &xsts_token.token)
                .await?;

        let profile =
            fetch_minecraft_profile(client, MINECRAFT_PROFILE_URL, &minecraft_token.access_token)
                .await?;

        let account = Account::new_microsoft(profile.name, profile.id)?;

        let tokens = MicrosoftTokenBundle {
            refresh_token: ms_token.refresh_token.unwrap_or_default(),
            access_token: minecraft_token.access_token,
            expires_at: minecraft_token.expires_in,
        };

        Ok(MicrosoftLoginResult { account, tokens })
    }
}
