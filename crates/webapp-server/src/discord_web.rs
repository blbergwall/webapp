//! Discord web sign-in ("Sign in with Discord" in website mode): OAuth2's
//! authorization-code flow, driven by the SPA like email login.
//!
//! 1. `start` returns Discord's authorize URL and binds a random `state` to
//!    this browser with the `discord_state` cookie.
//! 2. Discord redirects the browser to `{PUBLIC_BASE_URL}/?code=…&state=…`,
//!    where `<AuthGate>` picks them up.
//! 3. `confirm` checks the state against the cookie, exchanges the code and
//!    signs in with a normal website session cookie.
//!
//! The server never redirects, so trunk's dev proxy needs no special config.

use anyhow::Context;
use axum::{
    Json,
    extract::State,
    http::{HeaderMap, header},
    response::{AppendHeaders, IntoResponse, Response},
};
use webapp_common::{Account, DiscordWebConfirmRequest, DiscordWebStartResponse};

use crate::{
    WebApp, account, discord,
    error::ApiError,
    random::random_hex,
    session::{self, DISCORD_STATE_COOKIE},
};

/// `POST /api/auth/discord/web/start` — returns `{url}` (Discord's authorize
/// page) and sets the `discord_state` cookie. 404 if Discord isn't
/// configured.
pub(crate) async fn start(State(app): State<WebApp>) -> Result<impl IntoResponse, ApiError> {
    let config = app.config().discord.as_ref().ok_or(ApiError::NotFound)?;
    let state = random_hex(16);
    let url = discord::authorize_url(config, &state, &redirect_uri(&app)?);
    Ok((
        [(header::SET_COOKIE, session::discord_state_cookie(&state))],
        Json(DiscordWebStartResponse { url }),
    ))
}

/// `POST /api/auth/discord/web/confirm` — signs in with the `code` and
/// `state` Discord redirected back with, creating the account on first
/// sign-in, sets the session cookie and returns the `Account`.
///
/// 400 if the state doesn't match this browser's cookie (expired, or started
/// in another browser) or the code is empty; 401 if Discord rejects the code;
/// 404 if Discord isn't configured. The state cookie is cleared either way,
/// so a state works once.
pub(crate) async fn confirm(
    State(app): State<WebApp>,
    headers: HeaderMap,
    Json(request): Json<DiscordWebConfirmRequest>,
) -> Response {
    let clear_state = (header::SET_COOKIE, session::clear_discord_state_cookie());
    match sign_in(&app, &headers, &request).await {
        Ok((token, account)) => (
            AppendHeaders([(header::SET_COOKIE, session::session_cookie(&token)), clear_state]),
            Json(account),
        )
            .into_response(),
        Err(err) => (AppendHeaders([clear_state]), err).into_response(),
    }
}

/// Returns the new session token and the account.
async fn sign_in(
    app: &WebApp,
    headers: &HeaderMap,
    request: &DiscordWebConfirmRequest,
) -> Result<(String, Account), ApiError> {
    let config = app.config().discord.as_ref().ok_or(ApiError::NotFound)?;
    if !state_matches(session::cookie_value(headers, DISCORD_STATE_COOKIE), &request.state) {
        return Err(ApiError::BadRequest);
    }
    let code = request.code.trim();
    if code.is_empty() {
        return Err(ApiError::BadRequest);
    }

    let redirect_uri = redirect_uri(app)?;
    let access_token = discord::exchange_code(app, config, code, Some(&redirect_uri)).await?;
    let user = discord::fetch_user(app, &access_token).await?;

    let account =
        account::find_or_create_by_discord(app.redis(), &user.id, user.display_name()).await?;
    let token = session::create(app.redis(), &account.id, session::SessionKind::Website).await?;
    Ok((token, account))
}

/// Whether the `state` Discord returned is the one this browser's cookie
/// holds. A missing or empty cookie never matches.
fn state_matches(cookie: Option<&str>, state: &str) -> bool {
    cookie.is_some_and(|cookie| !cookie.is_empty() && cookie == state)
}

/// `{PUBLIC_BASE_URL}/`: the same string in the authorize URL, the token
/// exchange and the Developer Portal's registered redirects. `Config`
/// requires `PUBLIC_BASE_URL` whenever Discord is configured.
fn redirect_uri(app: &WebApp) -> Result<String, ApiError> {
    let base = app
        .config()
        .public_base_url
        .as_deref()
        .context("PUBLIC_BASE_URL is required for Discord web sign-in")?;
    Ok(format!("{base}/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_must_match_the_cookie() {
        assert!(state_matches(Some("abc"), "abc"));
        assert!(!state_matches(Some("abc"), "abd"));
        assert!(!state_matches(None, "abc"));
        assert!(!state_matches(Some(""), ""));
        assert!(!state_matches(None, ""));
    }
}
