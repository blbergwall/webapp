//! Server-side auth framework: axum routes, the `Auth` extractor, Upstash
//! Redis REST storage, email (Resend) and Discord login.
//!
//! Wiring it into an app:
//!
//! ```ignore
//! let webapp = WebApp::from_env()?;
//! let app = Router::new()
//!     .route("/api/hello", get(|Auth(account): Auth| async move { account.display_name }))
//!     .merge(webapp_server::auth_router())
//!     .with_state(webapp);
//! ```
//!
//! An app with its own state type keeps a `WebApp` inside it and implements
//! `FromRef<AppState> for WebApp`.

pub mod account;
mod auth;
mod config;
mod discord;
mod email;
mod error;
pub mod keys;
#[cfg(test)]
mod live_tests;
mod random;
mod redis;
mod session;
#[cfg(feature = "ws")]
mod ws;

use std::sync::Arc;

use axum::{
    Router,
    extract::FromRef,
    routing::{get, post},
};

pub use auth::Auth;
pub use config::{Config, DiscordConfig, EmailDelivery};
pub use error::ApiError;
pub use redis::Redis;
pub use webapp_common;
#[cfg(feature = "ws")]
pub use ws::WsAuth;

/// Handle to the framework's configuration and storage. Cheap to clone.
#[derive(Clone)]
pub struct WebApp(Arc<Inner>);

struct Inner {
    config: Config,
    http: reqwest::Client,
    redis: Redis,
}

impl WebApp {
    pub fn new(config: Config) -> Self {
        let http = reqwest::Client::new();
        let redis = Redis::new(&config, http.clone());
        Self(Arc::new(Inner { config, http, redis }))
    }

    /// Builds the framework from environment variables (see `Config`).
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self::new(Config::from_env()?))
    }

    pub fn config(&self) -> &Config {
        &self.0.config
    }

    /// Shared HTTP client for outbound API calls (Resend, Discord).
    pub(crate) fn http(&self) -> &reqwest::Client {
        &self.0.http
    }

    /// The Redis client, for apps to store their own data (under their own
    /// key prefixes — see [`keys`] for the ones the framework uses).
    pub fn redis(&self) -> &Redis {
        &self.0.redis
    }
}

/// The framework's `/api/auth/*` routes, ready to `merge` into an app router
/// whose state provides a `WebApp`.
pub fn auth_router<S>() -> Router<S>
where
    WebApp: FromRef<S>,
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/api/auth/config", get(auth::config))
        .route("/api/auth/me", get(auth::me))
        .route("/api/auth/logout", post(auth::logout))
        .route("/api/auth/discord", post(discord::login))
        .route("/api/auth/email/start", post(email::start))
        .route("/api/auth/email/confirm", post(email::confirm))
}

/// The WebSocket ticket endpoint (`POST /api/auth/ws-ticket`, `ws` feature).
/// Merge it alongside `auth_router()`, then authenticate the app's own
/// WebSocket routes with [`WsAuth`].
#[cfg(feature = "ws")]
pub fn ws_router<S>() -> Router<S>
where
    WebApp: FromRef<S>,
    S: Clone + Send + Sync + 'static,
{
    Router::new().route("/api/auth/ws-ticket", post(ws::ticket))
}
