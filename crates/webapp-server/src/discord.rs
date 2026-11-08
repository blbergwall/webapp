//! Discord OAuth2 core, shared by every Discord sign-in: exchanges an
//! authorization code for an access token and looks up the user. Ported from
//! `full-rust`.
//!
//! Who the user is comes solely from `users/@me`, called with an access token
//! this server obtained itself; the client only ever supplies the code.
//!
//! Today only Discord activity sign-in (`activity.rs`, `discord-activity`
//! feature) calls this. It stays compiled without that feature because the
//! planned web "Log in with Discord" builds on it too.
#![cfg_attr(not(feature = "discord-activity"), allow(dead_code))]

use anyhow::{Context, anyhow};
use serde::Deserialize;

use crate::{WebApp, config::DiscordConfig, error::ApiError};

const TOKEN_URL: &str = "https://discord.com/api/oauth2/token";
const USER_URL: &str = "https://discord.com/api/users/@me";

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
}

#[derive(Deserialize)]
struct TokenError {
    error: String,
}

/// The fields of Discord's user object this framework uses.
#[derive(Debug, Deserialize)]
pub(crate) struct DiscordUser {
    pub(crate) id: String,
    username: String,
    global_name: Option<String>,
}

impl DiscordUser {
    /// Discord's display name if the user set one, else their username.
    pub(crate) fn display_name(&self) -> &str {
        self.global_name
            .as_deref()
            .filter(|name| !name.trim().is_empty())
            .unwrap_or(&self.username)
    }
}

/// Exchanges an authorization code for an access token. A code Discord
/// rejects (`invalid_grant`: expired, reused or forged) is a 401; anything
/// else — including `invalid_client`, i.e. wrong server credentials — is a
/// logged 500.
pub(crate) async fn exchange_code(
    app: &WebApp,
    discord: &DiscordConfig,
    code: &str,
) -> Result<String, ApiError> {
    let response = app
        .http()
        .post(TOKEN_URL)
        .form(&[
            ("client_id", discord.client_id.as_str()),
            ("client_secret", discord.client_secret.as_str()),
            ("grant_type", "authorization_code"),
            ("code", code),
        ])
        .send()
        .await
        .context("failed to reach Discord token endpoint")?;

    let status = response.status();
    let body = response
        .text()
        .await
        .context("failed to read Discord token response")?;

    if status.is_success() {
        let token: TokenResponse =
            serde_json::from_str(&body).context("unexpected Discord token response")?;
        return Ok(token.access_token);
    }

    let error = serde_json::from_str::<TokenError>(&body).ok().map(|e| e.error);
    if error.as_deref() == Some("invalid_grant") {
        eprintln!("Discord rejected login code: {body}");
        return Err(ApiError::Unauthorized);
    }
    Err(anyhow!("Discord token exchange failed with status {status}: {body}").into())
}

/// The Discord user an access token belongs to (`users/@me`).
pub(crate) async fn fetch_user(app: &WebApp, access_token: &str) -> Result<DiscordUser, ApiError> {
    let response = app
        .http()
        .get(USER_URL)
        .bearer_auth(access_token)
        .send()
        .await
        .context("failed to reach Discord users/@me")?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(anyhow!("Discord users/@me returned status {status}: {body}").into());
    }
    let user = response
        .json()
        .await
        .context("unexpected Discord users/@me response")?;
    Ok(user)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(json: &str) -> DiscordUser {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn parses_users_me_payload_ignoring_extra_fields() {
        let parsed = user(
            r#"{"id":"80351110224678912","username":"nelly","global_name":"Nelly",
                "avatar":"8342729096ea3675442027381ff50dfe","discriminator":"0"}"#,
        );
        assert_eq!(parsed.id, "80351110224678912");
        assert_eq!(parsed.display_name(), "Nelly");
    }

    #[test]
    fn display_name_falls_back_to_username() {
        assert_eq!(user(r#"{"id":"1","username":"nelly","global_name":null}"#).display_name(), "nelly");
        assert_eq!(user(r#"{"id":"1","username":"nelly"}"#).display_name(), "nelly");
        assert_eq!(user(r#"{"id":"1","username":"nelly","global_name":"  "}"#).display_name(), "nelly");
    }
}
