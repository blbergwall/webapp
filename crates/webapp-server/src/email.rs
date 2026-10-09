//! Passwordless email (magic-link) login, ported from
//! `neverending-contrivance` onto the JSON API.

use std::net::SocketAddr;

use anyhow::{Context, Result, bail};
use axum::{
    Extension, Json,
    extract::{ConnectInfo, State},
    http::{HeaderMap, StatusCode, header},
    response::{AppendHeaders, IntoResponse},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use webapp_common::{EmailConfirmRequest, EmailStartRequest};

use crate::{
    WebApp, account,
    config::EmailDelivery,
    error::ApiError,
    keys,
    random::random_hex,
    session::{self, PENDING_LOGIN_COOKIE},
};

/// Value of an `LC<code>` record.
#[derive(Serialize, Deserialize)]
struct LoginCode {
    email: String,
    /// Must match the requesting browser's `pending_login` cookie for an
    /// automatic (`auto: true`) confirm.
    pending: String,
}

/// `POST /api/auth/email/start` — emails a one-time login link and binds it
/// to this browser with the `pending_login` cookie. 204 on success.
pub(crate) async fn start(
    State(app): State<WebApp>,
    connect_info: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    Json(request): Json<EmailStartRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let config = app.config();
    if matches!(config.email, EmailDelivery::Disabled) {
        return Err(ApiError::NotFound);
    }
    let email = normalize_email(&request.email).ok_or(ApiError::BadRequest)?;

    let peer = connect_info.map(|Extension(ConnectInfo(addr))| addr);
    let ip = client_ip(&headers, peer);
    let gate = format!("{}{ip}", keys::RATE_LIMIT);
    if !app
        .redis()
        .set_nx_ex(&gate, "1", Some(keys::RATE_LIMIT_TTL_SECS))
        .await?
    {
        return Err(ApiError::TooManyRequests);
    }

    let code = random_hex(32);
    let pending = random_hex(32);
    let record = serde_json::to_string(&LoginCode {
        email: email.clone(),
        pending: pending.clone(),
    })
    .context("failed to serialize login code")?;
    app.redis()
        .set_ex(&format!("{}{code}", keys::LOGIN_CODE), &record, keys::LOGIN_CODE_TTL_SECS)
        .await?;

    let base_url = config.public_base_url.as_deref().unwrap_or_default();
    let link = format!("{base_url}/?login_code={code}");
    match &config.email {
        EmailDelivery::Resend { api_key, from } => {
            send_login_email(&app, api_key, from, &email, &link).await?;
        }
        EmailDelivery::Console => println!("[DEV_MODE] login link for {email}: {link}"),
        EmailDelivery::Disabled => unreachable!("checked above"),
    }

    Ok((
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, session::pending_login_cookie(&pending))],
    ))
}

/// `POST /api/auth/email/confirm` — consumes a login code, signs in (creating
/// the account on first login), and returns the `Account`.
///
/// 410 if the code is expired or used. With `auto: true`, 409 (code left
/// unconsumed) unless this browser holds the matching `pending_login` cookie.
pub(crate) async fn confirm(
    State(app): State<WebApp>,
    headers: HeaderMap,
    Json(request): Json<EmailConfirmRequest>,
) -> Result<impl IntoResponse, ApiError> {
    if matches!(app.config().email, EmailDelivery::Disabled) {
        return Err(ApiError::NotFound);
    }
    let redis = app.redis();
    let code_key = format!("{}{}", keys::LOGIN_CODE, request.code);

    if request.auto {
        let record = redis.get(&code_key).await?.ok_or(ApiError::Gone)?;
        let record: LoginCode = serde_json::from_str(&record).context("corrupt login code")?;
        if session::cookie_value(&headers, PENDING_LOGIN_COOKIE) != Some(record.pending.as_str()) {
            return Err(ApiError::Conflict);
        }
    }

    // GETDEL makes the code single-use even if two confirms race.
    let record = redis.getdel(&code_key).await?.ok_or(ApiError::Gone)?;
    let record: LoginCode = serde_json::from_str(&record).context("corrupt login code")?;

    let account = account::find_or_create_by_email(redis, &record.email).await?;
    let token = session::create(redis, &account.id, session::SessionKind::Website).await?;

    Ok((
        AppendHeaders([
            (header::SET_COOKIE, session::session_cookie(&token)),
            (header::SET_COOKIE, session::clear_pending_login_cookie()),
        ]),
        Json(account),
    ))
}

/// Trims and lowercases an address, returning `None` unless it looks like
/// `local@domain`. Deliberately minimal: Resend rejects anything else that
/// is undeliverable.
fn normalize_email(raw: &str) -> Option<String> {
    let email = raw.trim().to_lowercase();
    let (local, domain) = email.split_once('@')?;
    let valid = !local.is_empty()
        && !domain.is_empty()
        && !domain.contains('@')
        && email.len() <= 254
        && !email.chars().any(char::is_whitespace);
    valid.then_some(email)
}

/// The client IP for rate limiting: Fly.io's documented `Fly-Client-IP`
/// header (not a client-suppliable `X-Forwarded-For`), falling back to the
/// TCP peer address when the app serves with connect info (local dev), else
/// one shared `unknown` bucket.
fn client_ip(headers: &HeaderMap, peer: Option<SocketAddr>) -> String {
    headers
        .get("Fly-Client-IP")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
        .or_else(|| peer.map(|addr| addr.ip().to_string()))
        .unwrap_or_else(|| "unknown".to_string())
}

/// Sends the magic-link login email via Resend's REST API.
async fn send_login_email(app: &WebApp, api_key: &str, from: &str, to: &str, link: &str) -> Result<()> {
    let app_name = &app.config().app_name;
    let escaped_name = html_escape(app_name);
    let response = app
        .http()
        .post("https://api.resend.com/emails")
        .bearer_auth(api_key)
        .json(&json!({
            "from": from,
            "to": [to],
            "subject": format!("Sign in to {app_name}"),
            "html": format!(
                "<p>Click the link below to finish signing in to {escaped_name}:</p><p><a href=\"{link}\">{link}</a></p><p>This link expires in 24 hours.</p>"
            ),
        }))
        .send()
        .await
        .context("failed to reach Resend API")?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        bail!("Resend API returned status {status}: {body}");
    }
    Ok(())
}

fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    #[test]
    fn normalizes_valid_emails() {
        assert_eq!(normalize_email("  Jane@Example.COM "), Some("jane@example.com".to_string()));
    }

    #[test]
    fn rejects_invalid_emails() {
        for bad in ["", "jane", "@example.com", "jane@", "a@b@c", "ja ne@example.com"] {
            assert_eq!(normalize_email(bad), None, "{bad:?}");
        }
        let long = format!("{}@example.com", "a".repeat(250));
        assert_eq!(normalize_email(&long), None);
    }

    #[test]
    fn client_ip_prefers_fly_header() {
        let mut headers = HeaderMap::new();
        headers.insert("Fly-Client-IP", HeaderValue::from_static("1.2.3.4"));
        let peer = Some("127.0.0.1:5000".parse().unwrap());
        assert_eq!(client_ip(&headers, peer), "1.2.3.4");
        assert_eq!(client_ip(&HeaderMap::new(), peer), "127.0.0.1");
        assert_eq!(client_ip(&HeaderMap::new(), None), "unknown");
    }

    #[test]
    fn escapes_html() {
        assert_eq!(html_escape("<a & \"b\">"), "&lt;a &amp; &quot;b&quot;&gt;");
    }
}
