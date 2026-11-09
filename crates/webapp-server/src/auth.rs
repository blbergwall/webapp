use std::time::Instant;

use anyhow::Result;
use axum::{
    Json,
    extract::{FromRef, FromRequestParts, State},
    http::{HeaderMap, Method, StatusCode, header, request::Parts},
    response::IntoResponse,
};
use webapp_common::{Account, AuthConfig};

use crate::{
    WebApp, account,
    error::ApiError,
    session::{self, SESSION_COOKIE},
    session_cache::Lookup,
};

/// Extractor for the signed-in account. Put it on any handler that requires
/// a session; unauthenticated requests are rejected with 401.
///
/// Accepts the session cookie (website mode) and, with the
/// `discord-activity` feature, `Authorization: Bearer <token>` (Discord
/// activity mode). A bearer header, when accepted and present, wins — an
/// invalid one is a 401 with no fallback to the cookie. Without the feature
/// the header is ignored.
///
/// CSRF: a cookie-authenticated request with a state-changing method must
/// send `Content-Type: application/json` (rejected with 403 otherwise).
/// Cross-site HTML forms can't send that type without a CORS preflight, which
/// this server never grants.
pub struct Auth(pub Account);

impl<S> FromRequestParts<S> for Auth
where
    WebApp: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app = WebApp::from_ref(state);
        let (token, source) = session_token(&parts.headers).ok_or(ApiError::Unauthorized)?;
        if !csrf_ok(&parts.method, &parts.headers, source) {
            return Err(ApiError::Forbidden);
        }
        let token = token.to_string();
        let account = resolve_account(&app, &token, Instant::now())
            .await?
            .ok_or(ApiError::Unauthorized)?;
        Ok(Auth(account))
    }
}

/// The account a session token belongs to, or `None` if the session doesn't
/// exist. With the session cache on, usually answered from memory; otherwise
/// two Redis calls (the session, which also slides its TTL, then the
/// account).
pub(crate) async fn resolve_account(app: &WebApp, token: &str, now: Instant) -> Result<Option<Account>> {
    let Some(cache) = app.session_cache() else {
        return load_account(app, token).await;
    };
    match cache.lookup(token, now, session::cache_refresh_interval(token)) {
        Lookup::Hit(account) => Ok(Some(account)),
        Lookup::Refresh(account) => {
            if session::refresh(app.redis(), token).await? {
                cache.refreshed(token, now);
                Ok(Some(account))
            } else {
                cache.remove(token);
                Ok(None)
            }
        }
        Lookup::Miss { generation } => {
            let account = load_account(app, token).await?;
            if let Some(account) = &account {
                cache.insert(token, account.clone(), generation, now);
            }
            Ok(account)
        }
    }
}

async fn load_account(app: &WebApp, token: &str) -> Result<Option<Account>> {
    match session::resolve(app.redis(), token).await? {
        Some(account_id) => account::get(app.redis(), &account_id).await,
        None => Ok(None),
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum TokenSource {
    /// `Authorization: Bearer` (`discord-activity` feature).
    #[cfg(feature = "discord-activity")]
    Bearer,
    Cookie,
}

/// The session token a request carries, and where it came from.
pub(crate) fn session_token(headers: &HeaderMap) -> Option<(&str, TokenSource)> {
    #[cfg(feature = "discord-activity")]
    if let Some(value) = headers.get(header::AUTHORIZATION) {
        let token = value.to_str().ok()?.strip_prefix("Bearer ")?.trim();
        return Some((token, TokenSource::Bearer));
    }
    session::cookie_value(headers, SESSION_COOKIE).map(|token| (token, TokenSource::Cookie))
}

/// Whether a request passes the CSRF rule described on [`Auth`]. Only cookies
/// need it: browsers attach them to cross-site requests automatically, but
/// never a bearer header.
pub(crate) fn csrf_ok(method: &Method, headers: &HeaderMap, source: TokenSource) -> bool {
    if source != TokenSource::Cookie
        || matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
    {
        return true;
    }
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(';')
                .next()
                .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("application/json"))
        })
}

/// `GET /api/auth/config`
pub(crate) async fn config(State(app): State<WebApp>) -> Json<AuthConfig> {
    Json(AuthConfig {
        discord_client_id: app.config().discord.as_ref().map(|discord| discord.client_id.clone()),
        google_sign_in: app.config().google.is_some(),
    })
}

/// `GET /api/auth/me`
pub(crate) async fn me(Auth(account): Auth) -> Json<Account> {
    Json(account)
}

/// `POST /api/auth/logout` — deletes the presented session (if any) and
/// clears the session cookie. Succeeds even with no session, so a client can
/// always get back to a signed-out state.
pub(crate) async fn logout(
    State(app): State<WebApp>,
    method: Method,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    if let Some((token, source)) = session_token(&headers) {
        if !csrf_ok(&method, &headers, source) {
            return Err(ApiError::Forbidden);
        }
        session::delete(app.redis(), token).await?;
        // After the delete: a request that read the session before it can no
        // longer cache it (the generation moves), and any later one finds
        // nothing in Redis.
        if let Some(cache) = app.session_cache() {
            cache.remove(token);
        }
    }
    Ok((
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, session::clear_session_cookie())],
    ))
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    fn headers(pairs: &[(header::HeaderName, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.append(name.clone(), HeaderValue::from_str(value).unwrap());
        }
        headers
    }

    #[cfg(feature = "discord-activity")]
    #[test]
    fn bearer_token() {
        let headers = headers(&[(header::AUTHORIZATION, "Bearer abc")]);
        assert_eq!(session_token(&headers), Some(("abc", TokenSource::Bearer)));
    }

    #[test]
    fn cookie_token() {
        let headers = headers(&[(header::COOKIE, "session=xyz")]);
        assert_eq!(session_token(&headers), Some(("xyz", TokenSource::Cookie)));
    }

    #[cfg(not(feature = "discord-activity"))]
    #[test]
    fn bearer_ignored_without_activity_feature() {
        let both = headers(&[
            (header::AUTHORIZATION, "Bearer abc"),
            (header::COOKIE, "session=xyz"),
        ]);
        assert_eq!(session_token(&both), Some(("xyz", TokenSource::Cookie)));
        let bearer_only = headers(&[(header::AUTHORIZATION, "Bearer abc")]);
        assert_eq!(session_token(&bearer_only), None);
    }

    #[cfg(feature = "discord-activity")]
    #[test]
    fn bearer_wins_over_cookie() {
        let headers = headers(&[
            (header::AUTHORIZATION, "Bearer abc"),
            (header::COOKIE, "session=xyz"),
        ]);
        assert_eq!(session_token(&headers), Some(("abc", TokenSource::Bearer)));
    }

    #[cfg(feature = "discord-activity")]
    #[test]
    fn non_bearer_authorization_does_not_fall_back_to_cookie() {
        let headers = headers(&[
            (header::AUTHORIZATION, "Basic abc"),
            (header::COOKIE, "session=xyz"),
        ]);
        assert_eq!(session_token(&headers), None);
    }

    #[test]
    fn no_token() {
        assert_eq!(session_token(&HeaderMap::new()), None);
    }

    #[test]
    fn csrf_allows_safe_methods_with_cookie() {
        assert!(csrf_ok(&Method::GET, &HeaderMap::new(), TokenSource::Cookie));
    }

    #[cfg(feature = "discord-activity")]
    #[test]
    fn csrf_allows_bearer_writes_without_json() {
        assert!(csrf_ok(&Method::POST, &HeaderMap::new(), TokenSource::Bearer));
    }

    #[test]
    fn csrf_rejects_cookie_writes_without_json() {
        assert!(!csrf_ok(&Method::POST, &HeaderMap::new(), TokenSource::Cookie));
        let form = headers(&[(header::CONTENT_TYPE, "application/x-www-form-urlencoded")]);
        assert!(!csrf_ok(&Method::POST, &form, TokenSource::Cookie));
        let text = headers(&[(header::CONTENT_TYPE, "text/plain")]);
        assert!(!csrf_ok(&Method::DELETE, &text, TokenSource::Cookie));
    }

    #[test]
    fn csrf_allows_cookie_writes_with_json() {
        let json = headers(&[(header::CONTENT_TYPE, "application/json; charset=utf-8")]);
        assert!(csrf_ok(&Method::POST, &json, TokenSource::Cookie));
    }
}
