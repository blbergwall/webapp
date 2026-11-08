//! Discord web sign-in ("Sign in with Discord" in website mode): OAuth2's
//! authorization-code flow, driven by the SPA (the shared pattern is in
//! `oauth.rs`). The scope is `identify` only, and a Discord account is never
//! matched to an email account.

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
};
use webapp_common::{Account, OAuthConfirmRequest, OAuthStartResponse};

use crate::{
    WebApp, account, discord,
    error::ApiError,
    oauth::{self, DISCORD},
    session,
};

/// `POST /api/auth/discord/web/start` — returns `{url}` (Discord's authorize
/// page) and sets the `discord_state` cookie. 404 if Discord isn't
/// configured.
pub(crate) async fn start(State(app): State<WebApp>) -> Result<impl IntoResponse, ApiError> {
    let config = app.config().discord.as_ref().ok_or(ApiError::NotFound)?;
    let state = DISCORD.new_state();
    let url = discord::authorize_url(config, &state, &oauth::redirect_uri(&app)?);
    Ok((
        [(header::SET_COOKIE, DISCORD.state_cookie(&state))],
        Json(OAuthStartResponse { url }),
    ))
}

/// `POST /api/auth/discord/web/confirm` — signs in with the `code` and
/// `state` Discord redirected back with, creating the account on first
/// sign-in, sets the session cookie and returns the `Account`.
///
/// 400 if the state doesn't match this browser's cookie (expired, or started
/// in another browser) or the code is empty; 401 if Discord rejects the code;
/// 404 if Discord isn't configured. The state cookie is cleared either way.
pub(crate) async fn confirm(
    State(app): State<WebApp>,
    headers: HeaderMap,
    Json(request): Json<OAuthConfirmRequest>,
) -> Response {
    DISCORD.confirm_response(sign_in(&app, &headers, &request).await)
}

/// Returns the new session token and the account.
async fn sign_in(
    app: &WebApp,
    headers: &HeaderMap,
    request: &OAuthConfirmRequest,
) -> Result<(String, Account), ApiError> {
    let config = app.config().discord.as_ref().ok_or(ApiError::NotFound)?;
    DISCORD.check_state(headers, &request.state)?;
    let code = request.code.trim();
    if code.is_empty() {
        return Err(ApiError::BadRequest);
    }

    let redirect_uri = oauth::redirect_uri(app)?;
    let access_token = discord::exchange_code(app, config, code, Some(&redirect_uri)).await?;
    let user = discord::fetch_user(app, &access_token).await?;

    let account =
        account::find_or_create_by_discord(app.redis(), &user.id, user.display_name()).await?;
    let token = session::create(app.redis(), &account.id, session::SessionKind::Website).await?;
    Ok((token, account))
}
