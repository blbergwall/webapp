//! Shared API types used by both `webapp-server` and `webapp-client`.
//!
//! Must only use WASM-compatible dependencies, since the client depends on it.

use serde::{Deserialize, Serialize};

/// A user account. One account can be reached through several identities
/// (an email address, a Discord user), each of which maps to `id`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    /// Creation time, in unix seconds.
    pub created: u64,
    pub display_name: String,
    pub email: Option<String>,
    pub discord_id: Option<String>,
}

/// Response body of `GET /api/auth/config`: runtime settings the client
/// needs before it can sign in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuthConfig {
    /// `None` when the server has no Discord app configured.
    pub discord_client_id: Option<String>,
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
