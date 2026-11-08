//! Discord OAuth2 core, shared by every Discord sign-in (web login in
//! `discord_web.rs`, activity login in `activity.rs`): builds the authorize
//! URL, exchanges an authorization code for an access token and looks up the
//! user. Ported from `full-rust`.
//!
//! Who the user is comes solely from `users/@me`, called with an access token
//! this server obtained itself; the client only ever supplies the code.

use anyhow::{Context, anyhow};
use reqwest::Url;
use serde::Deserialize;

use crate::{WebApp, config::DiscordConfig, error::ApiError};

const AUTHORIZE_URL: &str = "https://discord.com/oauth2/authorize";
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

/// Discord's authorize page for a web sign-in: the `identify` scope only,
/// and `prompt=none`, so a user who already authorized the app goes straight
/// back without the approval screen.
pub(crate) fn authorize_url(discord: &DiscordConfig, state: &str, redirect_uri: &str) -> String {
    Url::parse_with_params(
        AUTHORIZE_URL,
        [
            ("response_type", "code"),
            ("client_id", discord.client_id.as_str()),
            ("scope", "identify"),
            ("state", state),
            ("redirect_uri", redirect_uri),
            ("prompt", "none"),
        ],
    )
    .expect("AUTHORIZE_URL is a valid URL")
    .into()
}

/// Exchanges an authorization code for an access token. `redirect_uri` must
/// be the one the authorize request used (web sign-in); the activity's SDK
/// flow has none. A code Discord rejects (`invalid_grant`: expired, reused,
/// forged, or a mismatched `redirect_uri`) is a 401; anything else —
/// including `invalid_client`, i.e. wrong server credentials — is a logged
/// 500.
pub(crate) async fn exchange_code(
    app: &WebApp,
    discord: &DiscordConfig,
    code: &str,
    redirect_uri: Option<&str>,
) -> Result<String, ApiError> {
    let mut form = vec![
        ("client_id", discord.client_id.as_str()),
        ("client_secret", discord.client_secret.as_str()),
        ("grant_type", "authorization_code"),
        ("code", code),
    ];
    if let Some(redirect_uri) = redirect_uri {
        form.push(("redirect_uri", redirect_uri));
    }
    let response = app
        .http()
        .post(TOKEN_URL)
        .form(&form)
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
    fn authorize_url_has_encoded_params() {
        let config = DiscordConfig {
            client_id: "123".to_string(),
            client_secret: "secret".to_string(),
        };
        let url = authorize_url(&config, "abc", "http://localhost:8080/");
        let parsed = Url::parse(&url).unwrap();
        assert_eq!(parsed.as_str().split('?').next(), Some(AUTHORIZE_URL));
        let params: Vec<(String, String)> = parsed.query_pairs().into_owned().collect();
        let expected = [
            ("response_type", "code"),
            ("client_id", "123"),
            ("scope", "identify"),
            ("state", "abc"),
            ("redirect_uri", "http://localhost:8080/"),
            ("prompt", "none"),
        ];
        let expected: Vec<(String, String)> =
            expected.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        assert_eq!(params, expected);
        assert!(url.contains("redirect_uri=http%3A%2F%2Flocalhost%3A8080%2F"), "{url}");
        assert!(!url.contains("secret"));
    }

    #[test]
    fn display_name_falls_back_to_username() {
        assert_eq!(user(r#"{"id":"1","username":"nelly","global_name":null}"#).display_name(), "nelly");
        assert_eq!(user(r#"{"id":"1","username":"nelly"}"#).display_name(), "nelly");
        assert_eq!(user(r#"{"id":"1","username":"nelly","global_name":"  "}"#).display_name(), "nelly");
    }
}
