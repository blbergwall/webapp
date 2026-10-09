use anyhow::Result;
use axum::http::{HeaderMap, HeaderValue, header};

use crate::{keys, random::random_hex, redis::Redis};

/// Name of the website-mode session cookie.
pub const SESSION_COOKIE: &str = "session";

/// Name of the cookie binding an emailed login code to the browser that
/// requested it. Scoped to the email endpoints.
pub const PENDING_LOGIN_COOKIE: &str = "pending_login";

/// Creates a session for `account_id` and returns its token.
pub async fn create(redis: &Redis, account_id: &str) -> Result<String> {
    let token = random_hex(32);
    redis
        .set_ex(&session_key(&token), account_id, keys::SESSION_TTL_SECS)
        .await?;
    Ok(token)
}

/// Looks up the account id for a session token, refreshing the session's
/// sliding TTL in the same round trip.
pub async fn resolve(redis: &Redis, token: &str) -> Result<Option<String>> {
    redis.getex(&session_key(token), keys::SESSION_TTL_SECS).await
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
    fn session_cookie_attributes() {
        let cookie = session_cookie("abc");
        let cookie = cookie.to_str().unwrap();
        assert!(cookie.starts_with("session=abc;"));
        for attribute in ["HttpOnly", "Secure", "SameSite=Lax", "Path=/", "Max-Age=7776000"] {
            assert!(cookie.contains(attribute), "missing {attribute}");
        }
    }
}
