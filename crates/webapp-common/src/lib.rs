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
