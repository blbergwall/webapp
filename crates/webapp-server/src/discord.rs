//! Discord activity login: exchanges the Embedded App SDK's OAuth2 code for
//! the user's identity, ported from `full-rust`.
//!
//! The client only ever supplies the code. Who the user is comes solely from
//! `users/@me`, called with an access token this server obtained itself.

use anyhow::{Context, anyhow};
use axum::{Json, extract::State};
use serde::Deserialize;
use webapp_common::{DiscordLoginRequest, DiscordLoginResponse};

use crate::{WebApp, account, config::DiscordConfig, error::ApiError, session};

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
struct DiscordUser {
    id: String,
    username: String,
    global_name: Option<String>,
}

impl DiscordUser {
    /// Discord's display name if the user set one, else their username.
    fn display_name(&self) -> &str {
        self.global_name
            .as_deref()
            .filter(|name| !name.trim().is_empty())
            .unwrap_or(&self.username)
    }
}

/// `POST /api/auth/discord` — signs in with a Discord OAuth2 code, creating
/// the account on first login. Returns a bearer session token (no cookie:
/// cookies are unreliable inside the activity iframe).
pub(crate) async fn login(
    State(app): State<WebApp>,
    Json(request): Json<DiscordLoginRequest>,
) -> Result<Json<DiscordLoginResponse>, ApiError> {
    let discord = app.config().discord.as_ref().ok_or(ApiError::NotFound)?;
    let code = request.code.trim();
    if code.is_empty() {
        return Err(ApiError::BadRequest);
    }

    let access_token = exchange_code(&app, discord, code).await?;
    let user = fetch_user(&app, &access_token).await?;

    let account =
        account::find_or_create_by_discord(app.redis(), &user.id, user.display_name()).await?;
    let token = session::create(app.redis(), &account.id).await?;

    Ok(Json(DiscordLoginResponse {
        token,
        discord_access_token: access_token,
        account,
    }))
}

/// Exchanges an authorization code for an access token. A code Discord
/// rejects (`invalid_grant`: expired, reused or forged) is a 401; anything
/// else — including `invalid_client`, i.e. wrong server credentials — is a
/// logged 500.
async fn exchange_code(app: &WebApp, discord: &DiscordConfig, code: &str) -> Result<String, ApiError> {
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

async fn fetch_user(app: &WebApp, access_token: &str) -> Result<DiscordUser, ApiError> {
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
