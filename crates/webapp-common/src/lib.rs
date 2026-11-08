//! Shared API types used by both `webapp-server` and `webapp-client`.
//!
//! Must only use WASM-compatible dependencies, since the client depends on it.

use serde::{Deserialize, Serialize};

/// A user account. One account can be reached through several identities
/// (an email address, a Discord user, a Google user), each of which maps to
/// `id`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    /// Creation time, in unix seconds.
    pub created: u64,
    pub display_name: String,
    pub email: Option<String>,
    pub discord_id: Option<String>,
    /// Google's user ID (`sub`). Absent in records written before Google
    /// sign-in existed.
    #[serde(default)]
    pub google_id: Option<String>,
}

/// Response body of `GET /api/auth/config`: runtime settings the client
/// needs before it can sign in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuthConfig {
    /// `None` when the server has no Discord app configured. `Some` means
    /// Discord sign-in is available: the web button in website mode, and
    /// activity sign-in (with the `discord-activity` feature).
    pub discord_client_id: Option<String>,
    /// Whether "Sign in with Google" is available (website mode).
    #[serde(default)]
    pub google_sign_in: bool,
}

/// Response body of the OAuth `start` endpoints
/// (`POST /api/auth/discord/web/start`, `POST /api/auth/google/start`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OAuthStartResponse {
    /// The provider's authorize page; the client navigates the browser there.
    pub url: String,
}

/// Request body of the OAuth `confirm` endpoints: the `code` and `state`
/// query params the provider redirected back to the site with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OAuthConfirmRequest {
    pub code: String,
    pub state: String,
}

/// Request body of `POST /api/auth/discord`: the OAuth2 code from the
/// Embedded App SDK's `authorize` command (`discord-activity` feature).
#[cfg(feature = "discord-activity")]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiscordLoginRequest {
    pub code: String,
}

/// Response body of `POST /api/auth/discord` (`discord-activity` feature).
#[cfg(feature = "discord-activity")]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiscordLoginResponse {
    /// Session token: keep in memory and send as `Authorization: Bearer`.
    pub token: String,
    /// Discord OAuth2 access token, for the SDK's `authenticate` command if the
    /// app needs SDK features beyond sign-in.
    pub discord_access_token: String,
    pub account: Account,
}

/// Request body of `POST /api/auth/email/start`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmailStartRequest {
    pub email: String,
}

/// Request body of `POST /api/auth/email/confirm`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmailConfirmRequest {
    pub code: String,
    /// `true` when the client confirms automatically on page load. The server
    /// then only accepts the code if this browser is the one that requested
    /// it (409 otherwise, code not consumed), so link scanners that run JS
    /// can't burn it. A user's explicit "Finish signing in" click sends
    /// `false`.
    pub auto: bool,
}

/// Response body of `POST /api/auth/ws-ticket`: a single-use ticket to pass
/// as `?ticket=` on a WebSocket upgrade, valid for about 30 seconds.
#[cfg(feature = "ws")]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WsTicketResponse {
    pub ticket: String,
}
