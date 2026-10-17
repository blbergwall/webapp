//! Authenticated WebSockets (`ws` feature), via single-use tickets.
//!
//! A browser `WebSocket` can't send an `Authorization` header, and a
//! cookie-authenticated upgrade is open to cross-site WebSocket hijacking. So
//! the client first requests a ticket over normal `Auth` (bearer or cookie,
//! CSRF rule included), then opens the socket with `?ticket=…`, which
//! [`WsAuth`] consumes before the upgrade.

use anyhow::Context;
use axum::{
    Json,
    extract::{FromRef, FromRequestParts, State},
    http::request::Parts,
};
use webapp_common::{Account, WsTicketResponse};

use crate::{WebApp, auth::Auth, error::ApiError, keys, random::random_hex};

/// Random bytes in a ticket (hex-encoded, so twice as many characters).
const TICKET_BYTES: usize = 32;

/// Increments a rate-limit counter, starting its window on the first hit.
/// One atomic command, so a counter can never be left without a TTL.
const RATE_SCRIPT: &str = "local n = redis.call('INCR', KEYS[1]) \
    if n == 1 then redis.call('EXPIRE', KEYS[1], ARGV[1]) end \
    return n";

/// `POST /api/auth/ws-ticket` — issues a single-use WebSocket ticket for the
/// signed-in account. 429 once the account exceeds
/// `keys::WS_TICKET_RATE_LIMIT` tickets per window.
pub(crate) async fn ticket(
    State(app): State<WebApp>,
    Auth(account): Auth,
) -> Result<Json<WsTicketResponse>, ApiError> {
    let redis = app.redis();
    let counter = redis.key(&format!("{}{}", keys::WS_TICKET_RATE, account.id));
    let window = keys::WS_TICKET_RATE_WINDOW_SECS.to_string();
    let issued = redis
        .command(&["EVAL", RATE_SCRIPT, "1", &counter, &window])
        .await?
        .as_i64()
        .context("unexpected ticket rate counter reply")?;
    if issued > keys::WS_TICKET_RATE_LIMIT {
        return Err(ApiError::TooManyRequests);
    }

    let ticket = random_hex(TICKET_BYTES);
    redis
        .set_ex(
            &format!("{}{ticket}", keys::WS_TICKET),
            &serde_json::to_string(&account).context("failed to serialize account")?,
            keys::WS_TICKET_TTL_SECS,
        )
        .await?;
    Ok(Json(WsTicketResponse { ticket }))
}

/// Extractor for a WebSocket route: consumes the `?ticket=` from
/// `POST /api/auth/ws-ticket` and yields its account, or rejects with 401.
///
/// Put it before `WebSocketUpgrade` in the handler's arguments, so requests
/// without a valid ticket are rejected before upgrading:
///
/// ```ignore
/// async fn ws(WsAuth(account): WsAuth, upgrade: WebSocketUpgrade) -> Response {
///     upgrade.on_upgrade(move |socket| handle(socket, account))
/// }
/// ```
///
/// The socket is authenticated once, at connect: signing out doesn't close
/// sockets that are already open.
pub struct WsAuth(pub Account);

impl<S> FromRequestParts<S> for WsAuth
where
    WebApp: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let ticket = ticket_param(parts.uri.query()).ok_or(ApiError::Unauthorized)?;
        let app = WebApp::from_ref(state);
        // GETDEL makes the ticket single-use even if two upgrades race.
        let account = app
            .redis()
            .getdel(&format!("{}{ticket}", keys::WS_TICKET))
            .await?
            .ok_or(ApiError::Unauthorized)?;
        let account = serde_json::from_str(&account).context("corrupt WebSocket ticket")?;
        Ok(WsAuth(account))
    }
}

/// The `ticket` query parameter, if it has the shape of an issued ticket
/// (anything else can't exist in Redis, so it's rejected without a lookup).
fn ticket_param(query: Option<&str>) -> Option<&str> {
    query?
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(key, _)| *key == "ticket")
        .map(|(_, value)| value)
        .filter(|value| {
            value.len() == 2 * TICKET_BYTES && value.chars().all(|c| c.is_ascii_hexdigit())
        })
}

#[cfg(test)]
mod tests {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        response::IntoResponse,
    };

    use super::*;
    use crate::{Config, EmailDelivery};

    fn ticket() -> String {
        random_hex(TICKET_BYTES)
    }

    #[test]
    fn finds_ticket_among_params() {
        let ticket = ticket();
        let query = format!("a=1&ticket={ticket}&b=2");
        assert_eq!(ticket_param(Some(&query)), Some(ticket.as_str()));
    }

    #[test]
    fn rejects_missing_or_malformed_tickets() {
        let long = format!("ticket={}0", ticket());
        let non_hex = format!("ticket={}", "z".repeat(2 * TICKET_BYTES));
        let wrong_name = format!("xticket={}", ticket());
        for query in ["", "ticket=", "ticket=abc", &long, &non_hex, &wrong_name] {
            assert_eq!(ticket_param(Some(query)), None, "{query:?}");
        }
        assert_eq!(ticket_param(None), None);
    }

    /// A `WebApp` whose Redis is unreachable: these rejections must happen
    /// before any lookup.
    fn offline_app() -> WebApp {
        WebApp::new(Config {
            upstash_url: "http://127.0.0.1:9".to_string(),
            upstash_token: String::new(),
            key_prefix: String::new(),
            discord: None,
            app_name: "test".to_string(),
            public_base_url: None,
            email: EmailDelivery::Disabled,
        })
    }

    #[tokio::test]
    async fn ws_auth_rejects_without_valid_ticket_before_redis() {
        let app = offline_app();
        for uri in ["/api/ws", "/api/ws?ticket=", "/api/ws?ticket=not-a-ticket"] {
            let (mut parts, _) = Request::get(uri).body(Body::empty()).unwrap().into_parts();
            let rejection = WsAuth::from_request_parts(&mut parts, &app)
                .await
                .err()
                .expect("must reject");
            assert_eq!(rejection.into_response().status(), StatusCode::UNAUTHORIZED, "{uri}");
        }
    }
}
