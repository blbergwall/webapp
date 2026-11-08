//! Pieces shared by the OAuth web sign-ins (Discord in `discord_web.rs`,
//! Google in `google.rs`). Both follow the same SPA-driven pattern:
//!
//! 1. `start` returns the provider's authorize URL and binds a random `state`
//!    to this browser with a per-provider cookie.
//! 2. The provider redirects to `{PUBLIC_BASE_URL}/?code=…&state=…`, where
//!    `<AuthGate>` picks them up. The state starts with the provider's name
//!    (`discord.…`, `google.…`), so the client knows which `confirm` to call.
//! 3. `confirm` checks the state against the cookie, signs in, and answers
//!    with the session cookie; the state cookie is cleared on every outcome.
//!
//! The server never redirects, so trunk's dev proxy needs no special config.

use anyhow::Context;
use axum::{
    Json,
    http::{HeaderMap, HeaderValue, header},
    response::{AppendHeaders, IntoResponse, Response},
};
use webapp_common::Account;

use crate::{WebApp, error::ApiError, keys, random::random_hex, session};

/// One OAuth provider's names.
pub(crate) struct Provider {
    /// Prefix of its states (`<name>.<hex>`); also how the client routes a
    /// return to the right `confirm`.
    pub(crate) name: &'static str,
    /// Its state cookie.
    pub(crate) cookie: &'static str,
    /// The state cookie's `Path`: must cover the provider's `confirm`.
    pub(crate) path: &'static str,
}

pub(crate) const DISCORD: Provider = Provider {
    name: "discord",
    cookie: "discord_state",
    path: "/api/auth/discord/web",
};

pub(crate) const GOOGLE: Provider = Provider {
    name: "google",
    cookie: "google_state",
    path: "/api/auth/google",
};

impl Provider {
    /// A fresh state: `<name>.` and 32 random hex characters.
    pub(crate) fn new_state(&self) -> String {
        format!("{}.{}", self.name, random_hex(16))
    }

    /// `Set-Cookie` value binding `state` to this browser. `SameSite=Lax` is
    /// enough: only the SPA's same-origin `confirm` call reads it.
    pub(crate) fn state_cookie(&self, state: &str) -> HeaderValue {
        let max_age = keys::OAUTH_STATE_TTL_SECS;
        let cookie = format!(
            "{}={state}; HttpOnly; Secure; SameSite=Lax; Path={}; Max-Age={max_age}",
            self.cookie, self.path
        );
        HeaderValue::from_str(&cookie).expect("states are `name.hex`, so always a valid header")
    }

    /// `Set-Cookie` value that removes the state cookie.
    pub(crate) fn clear_state_cookie(&self) -> HeaderValue {
        let cookie = format!(
            "{}=; HttpOnly; Secure; SameSite=Lax; Path={}; Max-Age=0",
            self.cookie, self.path
        );
        HeaderValue::from_str(&cookie).expect("cookie names and paths are valid header text")
    }

    /// 400 unless `state` is the one this browser's cookie holds (missing:
    /// expired, or started in another browser).
    pub(crate) fn check_state(&self, headers: &HeaderMap, state: &str) -> Result<(), ApiError> {
        if state_matches(session::cookie_value(headers, self.cookie), state) {
            Ok(())
        } else {
            Err(ApiError::BadRequest)
        }
    }

    /// The `confirm` response: the session cookie and account on success,
    /// the error otherwise, with the state cookie cleared either way so a
    /// state works once.
    pub(crate) fn confirm_response(&self, result: Result<(String, Account), ApiError>) -> Response {
        let clear_state = (header::SET_COOKIE, self.clear_state_cookie());
        match result {
            Ok((token, account)) => (
                AppendHeaders([(header::SET_COOKIE, session::session_cookie(&token)), clear_state]),
                Json(account),
            )
                .into_response(),
            Err(err) => (AppendHeaders([clear_state]), err).into_response(),
        }
    }
}

/// Whether the `state` the provider returned is the one this browser's
/// cookie holds. A missing or empty cookie never matches.
fn state_matches(cookie: Option<&str>, state: &str) -> bool {
    cookie.is_some_and(|cookie| !cookie.is_empty() && cookie == state)
}

/// `{PUBLIC_BASE_URL}/`: the same string in the authorize URL, the token
/// exchange and the provider's registered redirects. `Config` requires
/// `PUBLIC_BASE_URL` whenever Discord or Google is configured.
pub(crate) fn redirect_uri(app: &WebApp) -> Result<String, ApiError> {
    let base = app
        .config()
        .public_base_url
        .as_deref()
        .context("PUBLIC_BASE_URL is required for Discord and Google sign-in")?;
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

    #[test]
    fn states_are_prefixed_and_random() {
        let state = GOOGLE.new_state();
        let hex = state.strip_prefix("google.").expect("google. prefix");
        assert_eq!(hex.len(), 32);
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(state, GOOGLE.new_state());
        assert!(DISCORD.new_state().starts_with("discord."));
    }

    #[test]
    fn state_cookie_attributes() {
        let cookie = GOOGLE.state_cookie("google.abc");
        let cookie = cookie.to_str().unwrap();
        assert!(cookie.starts_with("google_state=google.abc;"));
        for attribute in ["HttpOnly", "Secure", "SameSite=Lax", "Path=/api/auth/google;", "Max-Age=600"] {
            assert!(cookie.contains(attribute), "missing {attribute}");
        }
        let cleared = DISCORD.clear_state_cookie();
        let cleared = cleared.to_str().unwrap();
        assert!(cleared.starts_with("discord_state=;"));
        assert!(cleared.contains("Path=/api/auth/discord/web;") && cleared.contains("Max-Age=0"));
    }

    #[test]
    fn check_state_reads_the_providers_own_cookie() {
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, HeaderValue::from_static("discord_state=s1; google_state=s2"));
        assert!(DISCORD.check_state(&headers, "s1").is_ok());
        assert!(GOOGLE.check_state(&headers, "s2").is_ok());
        assert!(GOOGLE.check_state(&headers, "s1").is_err());
    }
}
