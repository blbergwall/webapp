//! Discord activity sign-in (`discord-activity` feature): the Embedded App
//! SDK's `authorize` code, exchanged for a bearer-token session.

use axum::{Json, extract::State};
use webapp_common::{DiscordLoginRequest, DiscordLoginResponse};

use crate::{WebApp, account, discord, error::ApiError, session};

/// `POST /api/auth/discord` — signs in with a Discord OAuth2 code, creating
/// the account on first login. Returns a bearer session token (no cookie:
/// cookies are unreliable inside the activity iframe).
pub(crate) async fn login(
    State(app): State<WebApp>,
    Json(request): Json<DiscordLoginRequest>,
) -> Result<Json<DiscordLoginResponse>, ApiError> {
    let config = app.config().discord.as_ref().ok_or(ApiError::NotFound)?;
    let code = request.code.trim();
    if code.is_empty() {
        return Err(ApiError::BadRequest);
    }

    let access_token = discord::exchange_code(&app, config, code).await?;
    let user = discord::fetch_user(&app, &access_token).await?;

    let account =
        account::find_or_create_by_discord(app.redis(), &user.id, user.display_name()).await?;
    let token = session::create(app.redis(), &account.id, session::SessionKind::Activity).await?;

    Ok(Json(DiscordLoginResponse {
        token,
        discord_access_token: access_token,
        account,
    }))
}
