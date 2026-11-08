use anyhow::Result;
use axum::http::{HeaderMap, HeaderValue, header};

use crate::{keys, random::random_hex, redis::Redis};

/// Name of the website-mode session cookie.
pub const SESSION_COOKIE: &str = "session";

/// Name of the cookie binding an emailed login code to the browser that
/// requested it. Scoped to the email endpoints.
pub const PENDING_LOGIN_COOKIE: &str = "pending_login";

/// Name of the cookie holding a Discord web sign-in's OAuth2 `state`, which
/// binds the sign-in to the browser that started it. Scoped to the Discord
/// web endpoints.
pub const DISCORD_STATE_COOKIE: &str = "discord_state";

/// Marks a Discord activity session's token, so its (shorter) TTL can be
/// chosen from the token alone, without another Redis round trip.
#[cfg(feature = "discord-activity")]
pub const ACTIVITY_TOKEN_PREFIX: &str = "dc_";

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SessionKind {
    /// Cookie session from a website sign-in.
    Website,
    /// Bearer-token session from Discord activity sign-in
    /// (`discord-activity` feature).
    #[cfg(feature = "discord-activity")]
    Activity,
}

/// Creates a session for `account_id` and returns its token.
pub async fn create(redis: &Redis, account_id: &str, kind: SessionKind) -> Result<String> {
    let token = match kind {
        SessionKind::Website => random_hex(32),
        #[cfg(feature = "discord-activity")]
        SessionKind::Activity => format!("{ACTIVITY_TOKEN_PREFIX}{}", random_hex(32)),
    };
    redis
        .set_ex(&session_key(&token), account_id, ttl_secs(&token))
        .await?;
    Ok(token)
}

/// Looks up the account id for a session token, refreshing the session's
/// sliding TTL in the same command.
pub async fn resolve(redis: &Redis, token: &str) -> Result<Option<String>> {
    redis.getex(&session_key(token), ttl_secs(token)).await
}

/// The sliding TTL for a session, by the kind its token encodes.
#[cfg_attr(not(feature = "discord-activity"), allow(unused_variables))]
fn ttl_secs(token: &str) -> u64 {
    #[cfg(feature = "discord-activity")]
    if token.starts_with(ACTIVITY_TOKEN_PREFIX) {
        return keys::ACTIVITY_SESSION_TTL_SECS;
    }
    keys::SESSION_TTL_SECS
}

pub async fn delete(redis: &Redis, token: &str) -> Result<()> {
    redis.del(&session_key(token)).await
}

fn session_key(token: &str) -> String {
    format!("{}{token}", keys::SESSION)
}

/// The value of cookie `name` from the request's `Cookie` header(s), if any.
pub fn cookie_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value)
}

/// `Set-Cookie` value for a new website-mode session.
pub fn session_cookie(token: &str) -> HeaderValue {
    let max_age = keys::SESSION_TTL_SECS;
    let cookie =
        format!("{SESSION_COOKIE}={token}; HttpOnly; Secure; SameSite=Lax; Path=/; Max-Age={max_age}");
    HeaderValue::from_str(&cookie).expect("session tokens are hex, so always a valid header")
}

/// `Set-Cookie` value that removes the session cookie.
pub fn clear_session_cookie() -> HeaderValue {
    HeaderValue::from_static("session=; HttpOnly; Secure; SameSite=Lax; Path=/; Max-Age=0")
}

/// `Set-Cookie` value for the pending-login binding (see
/// [`PENDING_LOGIN_COOKIE`]); lives as long as the login code it guards.
pub fn pending_login_cookie(value: &str) -> HeaderValue {
    let max_age = keys::LOGIN_CODE_TTL_SECS;
    let cookie = format!(
        "{PENDING_LOGIN_COOKIE}={value}; HttpOnly; Secure; SameSite=Lax; Path=/api/auth/email; Max-Age={max_age}"
    );
    HeaderValue::from_str(&cookie).expect("pending values are hex, so always a valid header")
}

/// `Set-Cookie` value that removes the pending-login cookie.
pub fn clear_pending_login_cookie() -> HeaderValue {
    HeaderValue::from_static(
        "pending_login=; HttpOnly; Secure; SameSite=Lax; Path=/api/auth/email; Max-Age=0",
    )
}

/// `Set-Cookie` value for a Discord web sign-in's `state` (see
/// [`DISCORD_STATE_COOKIE`]). `SameSite=Lax` is enough: only the SPA's
/// same-origin `confirm` call reads it.
pub fn discord_state_cookie(state: &str) -> HeaderValue {
    let max_age = keys::DISCORD_STATE_TTL_SECS;
    let cookie = format!(
        "{DISCORD_STATE_COOKIE}={state}; HttpOnly; Secure; SameSite=Lax; Path=/api/auth/discord/web; Max-Age={max_age}"
    );
    HeaderValue::from_str(&cookie).expect("state values are hex, so always a valid header")
}

/// `Set-Cookie` value that removes the Discord state cookie.
pub fn clear_discord_state_cookie() -> HeaderValue {
    HeaderValue::from_static(
        "discord_state=; HttpOnly; Secure; SameSite=Lax; Path=/api/auth/discord/web; Max-Age=0",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(cookies: &[&str]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for cookie in cookies {
            headers.append(header::COOKIE, HeaderValue::from_str(cookie).unwrap());
        }
        headers
    }

    #[test]
    fn finds_cookie_among_others() {
        let headers = headers(&["a=1; session=abc; b=2"]);
        assert_eq!(cookie_value(&headers, "session"), Some("abc"));
    }

    #[test]
    fn finds_cookie_in_second_header() {
        let headers = headers(&["a=1", "session=abc"]);
        assert_eq!(cookie_value(&headers, "session"), Some("abc"));
    }

    #[test]
    fn exact_name_match_only() {
        let headers = headers(&["xsession=bad; session_old=bad"]);
        assert_eq!(cookie_value(&headers, "session"), None);
    }

    #[test]
    fn missing_cookie() {
        assert_eq!(cookie_value(&HeaderMap::new(), "session"), None);
    }

    #[test]
    fn ttl_by_token_kind() {
        assert_eq!(ttl_secs("abc123"), keys::SESSION_TTL_SECS);
        #[cfg(feature = "discord-activity")]
        assert_eq!(ttl_secs("dc_abc123"), keys::ACTIVITY_SESSION_TTL_SECS);
    }

    #[test]
    fn session_cookie_attributes() {
        let cookie = session_cookie("abc");
        let cookie = cookie.to_str().unwrap();
        assert!(cookie.starts_with("session=abc;"));
        for attribute in ["HttpOnly", "Secure", "SameSite=Lax", "Path=/", "Max-Age=7776000"] {
            assert!(cookie.contains(attribute), "missing {attribute}");
        }
    }

    #[test]
    fn discord_state_cookie_attributes() {
        let cookie = discord_state_cookie("abc");
        let cookie = cookie.to_str().unwrap();
        assert!(cookie.starts_with("discord_state=abc;"));
        for attribute in [
            "HttpOnly",
            "Secure",
            "SameSite=Lax",
            "Path=/api/auth/discord/web;",
            "Max-Age=600",
        ] {
            assert!(cookie.contains(attribute), "missing {attribute}");
        }
        let cleared = clear_discord_state_cookie();
        let cleared = cleared.to_str().unwrap();
        assert!(cleared.starts_with("discord_state=;"));
        assert!(cleared.contains("Path=/api/auth/discord/web;") && cleared.contains("Max-Age=0"));
    }
}
