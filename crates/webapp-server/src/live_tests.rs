//! Tests against the real Upstash database configured in `.env`. Ignored by
//! default; run with `cargo test -p webapp-server -- --ignored`.
//!
//! Each run namespaces its keys under a random prefix and deletes them at the
//! end, so it never touches app data.

use webapp_common::Account;

use crate::{
    Config, WebApp, account, keys,
    random::random_hex,
    session::{self, SessionKind},
};

fn test_app() -> WebApp {
    dotenvy::dotenv().ok();
    let mut config = Config::from_env().expect("live tests need Upstash settings in .env");
    config.key_prefix = format!("webapp-test:{}:", random_hex(4));
    WebApp::new(config)
}

async fn cleanup(app: &WebApp) {
    let pattern = app.redis().key("*");
    let keys = app.redis().command(&["KEYS", &pattern]).await.unwrap();
    for key in keys.as_array().unwrap() {
        app.redis().command(&["DEL", key.as_str().unwrap()]).await.unwrap();
    }
}

fn new_account(email: &str) -> impl FnOnce(String, u64) -> Account + '_ {
    move |id, created| Account {
        id,
        created,
        display_name: "tester".to_string(),
        email: Some(email.to_string()),
        discord_id: None,
        google_id: None,
    }
}

#[tokio::test]
#[ignore]
async fn account_find_or_create_and_race() {
    let app = test_app();
    let redis = app.redis();
    let identity = format!("{}race@example.com", keys::IDENTITY_EMAIL);

    // Two concurrent creates for one identity must agree on one account.
    let (a, b) = tokio::join!(
        account::find_or_create(redis, &identity, new_account("race@example.com")),
        account::find_or_create(redis, &identity, new_account("race@example.com")),
    );
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_eq!(a.id, b.id);

    // Exactly one account record survives (the loser's orphan was deleted).
    let pattern = redis.key(&format!("{}*", keys::ACCOUNT));
    let accounts = redis.command(&["KEYS", &pattern]).await.unwrap();
    assert_eq!(accounts.as_array().unwrap().len(), 1);

    // A later lookup finds the same account without creating another.
    let again = account::find_or_create(redis, &identity, |_, _| panic!("should exist"))
        .await
        .unwrap();
    assert_eq!(again, a);
    assert_eq!(account::get(redis, &a.id).await.unwrap(), Some(a));

    cleanup(&app).await;
}

#[tokio::test]
#[ignore]
async fn session_lifecycle_and_prefix() {
    let app = test_app();
    let redis = app.redis();

    let token = session::create(redis, "acct1", SessionKind::Website).await.unwrap();
    assert_eq!(session::resolve(redis, &token).await.unwrap().as_deref(), Some("acct1"));

    // Stored under the namespaced key, with the sliding TTL applied.
    let full_key = redis.key(&format!("{}{token}", keys::SESSION));
    assert!(full_key.starts_with("webapp-test:"));
    let ttl = redis.command(&["TTL", &full_key]).await.unwrap();
    let ttl = ttl.as_i64().unwrap();
    assert!(ttl > (keys::SESSION_TTL_SECS - 60) as i64, "ttl was {ttl}");

    session::delete(redis, &token).await.unwrap();
    assert_eq!(session::resolve(redis, &token).await.unwrap(), None);

    // Activity sessions get the short TTL, on creation and on each refresh.
    #[cfg(feature = "discord-activity")]
    {
        let token = session::create(redis, "acct2", SessionKind::Activity).await.unwrap();
        assert!(token.starts_with(session::ACTIVITY_TOKEN_PREFIX));
        let full_key = redis.key(&format!("{}{token}", keys::SESSION));
        redis.command(&["EXPIRE", &full_key, "100"]).await.unwrap();
        assert_eq!(session::resolve(redis, &token).await.unwrap().as_deref(), Some("acct2"));
        let ttl = redis.command(&["TTL", &full_key]).await.unwrap().as_i64().unwrap();
        let expected = keys::ACTIVITY_SESSION_TTL_SECS as i64;
        assert!(ttl > expected - 60 && ttl <= expected, "ttl was {ttl}");
    }

    cleanup(&app).await;
}

#[tokio::test]
#[ignore]
async fn find_account_by_email() {
    let app = test_app();
    let redis = app.redis();

    let account = account::find_or_create_by_email(redis, "fiona@example.com").await.unwrap();
    let found = account::find_by_email(redis, "  Fiona@Example.COM ").await.unwrap();
    assert_eq!(found, Some(account));
    assert_eq!(account::find_by_email(redis, "nobody@example.com").await.unwrap(), None);
    assert_eq!(account::find_by_email(redis, "not-an-email").await.unwrap(), None);

    cleanup(&app).await;
}

#[tokio::test]
#[ignore]
async fn resolve_account_without_cache() {
    use std::time::Instant;

    use crate::auth::resolve_account;

    let app = test_app();
    let redis = app.redis();
    let account = account::find_or_create_by_email(redis, "plain@example.com").await.unwrap();
    let token = session::create(redis, &account.id, SessionKind::Website).await.unwrap();

    assert_eq!(resolve_account(&app, &token, Instant::now()).await.unwrap(), Some(account));
    session::delete(redis, &token).await.unwrap();
    assert_eq!(resolve_account(&app, &token, Instant::now()).await.unwrap(), None);

    cleanup(&app).await;
}

#[tokio::test]
#[ignore]
async fn session_cache_refresh_and_invalidation() {
    use std::time::{Duration, Instant};

    use axum::{
        extract::State,
        http::{HeaderMap, HeaderValue, Method, StatusCode, header},
        response::IntoResponse,
    };

    use crate::auth::{self, resolve_account};

    dotenvy::dotenv().ok();
    let mut config = Config::from_env().expect("live tests need Upstash settings in .env");
    config.key_prefix = format!("webapp-test:{}:", random_hex(4));
    config.session_cache = true;
    let app = WebApp::new(config);
    let redis = app.redis();

    const MINUTE: Duration = Duration::from_secs(60);
    const DAY: Duration = Duration::from_secs(24 * 60 * 60);
    let ttl = |token: &str| {
        let key = redis.key(&format!("{}{token}", keys::SESSION));
        async move { redis.command(&["TTL", &key]).await.unwrap().as_i64().unwrap() }
    };

    let account = account::find_or_create_by_email(redis, "cache@example.com").await.unwrap();
    let token = session::create(redis, &account.id, SessionKind::Website).await.unwrap();
    let start = Instant::now();

    // A miss reads Redis and fills the cache.
    assert_eq!(resolve_account(&app, &token, start).await.unwrap(), Some(account.clone()));

    // Hits don't touch Redis: the session's (shortened) TTL isn't reset. The
    // entry stays in use every 30 minutes, so the idle sweep keeps it.
    redis
        .command(&["EXPIRE", &redis.key(&format!("{}{token}", keys::SESSION)), "100"])
        .await
        .unwrap();
    let mut now = start;
    while now + 30 * MINUTE < start + DAY {
        now += 30 * MINUTE;
        assert_eq!(resolve_account(&app, &token, now).await.unwrap(), Some(account.clone()));
    }
    assert!(ttl(&token).await <= 100);

    // A day after the last refresh, a hit resets the Redis TTL.
    let refreshed = start + DAY;
    assert_eq!(resolve_account(&app, &token, refreshed).await.unwrap(), Some(account.clone()));
    assert!(ttl(&token).await > (keys::SESSION_TTL_SECS - 60) as i64);

    // A session deleted behind the cache's back is dropped at its next
    // refresh (and then misses in Redis).
    session::delete(redis, &token).await.unwrap();
    let mut now = refreshed;
    while now + 30 * MINUTE < refreshed + DAY {
        now += 30 * MINUTE;
        assert!(resolve_account(&app, &token, now).await.unwrap().is_some());
    }
    assert_eq!(resolve_account(&app, &token, refreshed + DAY).await.unwrap(), None);
    assert_eq!(resolve_account(&app, &token, refreshed + DAY).await.unwrap(), None);

    // Logout drops the cached session at once.
    let token = session::create(redis, &account.id, SessionKind::Website).await.unwrap();
    let now = Instant::now();
    assert!(resolve_account(&app, &token, now).await.unwrap().is_some());
    let mut headers = HeaderMap::new();
    let cookie = format!("{}={token}", session::SESSION_COOKIE);
    headers.insert(header::COOKIE, HeaderValue::from_str(&cookie).unwrap());
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
    let response = auth::logout(State(app.clone()), Method::POST, headers)
        .await
        .unwrap()
        .into_response();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(resolve_account(&app, &token, now + MINUTE).await.unwrap(), None);

    // Linking Google to an email account updates its cached sessions.
    let bob = account::find_or_create_by_email(redis, "bob@gmail.com").await.unwrap();
    let token = session::create(redis, &bob.id, SessionKind::Website).await.unwrap();
    let cached = resolve_account(&app, &token, now).await.unwrap().unwrap();
    assert_eq!(cached.google_id, None);
    let linked = account::find_or_create_by_google(&app, "g9", Some("bob@gmail.com"), Some("Bob"))
        .await
        .unwrap();
    assert_eq!(linked.id, bob.id);
    let cached = resolve_account(&app, &token, now + MINUTE).await.unwrap().unwrap();
    assert_eq!(cached.google_id.as_deref(), Some("g9"));

    cleanup(&app).await;
}

#[tokio::test]
#[ignore]
async fn email_login_flow() {
    use axum::{
        Json,
        extract::State,
        http::{HeaderMap, HeaderValue, StatusCode, header},
        response::IntoResponse,
    };
    use webapp_common::{EmailConfirmRequest, EmailStartRequest};

    use crate::{EmailDelivery, email};

    dotenvy::dotenv().ok();
    let mut config = Config::from_env().expect("live tests need Upstash settings in .env");
    config.key_prefix = format!("webapp-test:{}:", random_hex(4));
    config.email = EmailDelivery::Console;
    config.public_base_url = Some("http://localhost:8080".to_string());
    let app = WebApp::new(config);

    let start = |email: &str| {
        let app = app.clone();
        let request = EmailStartRequest { email: email.to_string() };
        async move {
            email::start(State(app), None, HeaderMap::new(), Json(request))
                .await
                .into_response()
        }
    };
    let confirm = |code: &str, auto: bool, cookie: Option<&str>| {
        let app = app.clone();
        let request = EmailConfirmRequest { code: code.to_string(), auto };
        let mut headers = HeaderMap::new();
        if let Some(cookie) = cookie {
            headers.insert(header::COOKIE, HeaderValue::from_str(cookie).unwrap());
        }
        async move { email::confirm(State(app), headers, Json(request)).await.into_response() }
    };

    // Invalid address is rejected before touching the rate limit.
    assert_eq!(start("not-an-email").await.status(), StatusCode::BAD_REQUEST);

    // Start: 204 and a pending cookie.
    let response = start(" Tester@Example.com ").await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let set_cookie = response.headers()[header::SET_COOKIE].to_str().unwrap();
    let pending = set_cookie
        .strip_prefix("pending_login=")
        .and_then(|rest| rest.split(';').next())
        .unwrap()
        .to_string();

    // Second request from the same (unknown) IP inside the window: 429.
    assert_eq!(start("tester@example.com").await.status(), StatusCode::TOO_MANY_REQUESTS);

    // Find the issued code (the console link isn't captured here).
    let pattern = app.redis().key(&format!("{}*", keys::LOGIN_CODE));
    let codes = app.redis().command(&["KEYS", &pattern]).await.unwrap();
    let codes = codes.as_array().unwrap();
    assert_eq!(codes.len(), 1);
    let code = codes[0].as_str().unwrap().rsplit(keys::LOGIN_CODE).next().unwrap().to_string();

    // Auto-confirm from another browser (no / wrong cookie): 409, code kept.
    assert_eq!(confirm(&code, true, None).await.status(), StatusCode::CONFLICT);
    let wrong = "pending_login=0000";
    assert_eq!(confirm(&code, true, Some(wrong)).await.status(), StatusCode::CONFLICT);

    // Auto-confirm from the requesting browser: signed in.
    let cookie = format!("pending_login={pending}");
    let response = confirm(&code, true, Some(&cookie)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let cookies: Vec<_> = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|value| value.to_str().unwrap().to_string())
        .collect();
    assert!(cookies.iter().any(|c| c.starts_with("session=") && !c.starts_with("session=;")));
    assert!(cookies.iter().any(|c| c.starts_with("pending_login=;")));
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let account: Account = serde_json::from_slice(&body).unwrap();
    assert_eq!(account.email.as_deref(), Some("tester@example.com"));
    assert_eq!(account.display_name, "tester");

    // The code is single-use.
    assert_eq!(confirm(&code, false, None).await.status(), StatusCode::GONE);

    // Signing in again with the same address reaches the same account.
    let again = account::find_or_create_by_email(app.redis(), "tester@example.com")
        .await
        .unwrap();
    assert_eq!(again.id, account.id);

    cleanup(&app).await;
}

#[tokio::test]
#[ignore]
async fn discord_account_keyed_by_discord_id() {
    let app = test_app();
    let redis = app.redis();

    let first = account::find_or_create_by_discord(redis, "111", "Nelly").await.unwrap();
    assert_eq!(first.discord_id.as_deref(), Some("111"));
    assert_eq!(first.display_name, "Nelly");
    assert_eq!(first.email, None);

    // Same Discord user again (even under a new name): same account.
    let again = account::find_or_create_by_discord(redis, "111", "Renamed").await.unwrap();
    assert_eq!(again, first);

    // A different Discord user: a different account.
    let other = account::find_or_create_by_discord(redis, "222", "Other").await.unwrap();
    assert_ne!(other.id, first.id);

    cleanup(&app).await;
}

#[tokio::test]
#[ignore]
async fn google_account_matching_by_trusted_email() {
    let app = test_app();
    let redis = app.redis();

    // New Google user with a trusted email: a new account that also owns the
    // email identity, so a later email sign-in reaches it.
    let created = account::find_or_create_by_google(&app, "g1", Some("ann@gmail.com"), Some("Ann"))
        .await
        .unwrap();
    assert_eq!(created.google_id.as_deref(), Some("g1"));
    assert_eq!(created.email.as_deref(), Some("ann@gmail.com"));
    assert_eq!(created.display_name, "Ann");
    let by_email = account::find_or_create_by_email(redis, "ann@gmail.com").await.unwrap();
    assert_eq!(by_email.id, created.id);

    // Same Google user again: same account.
    let again = account::find_or_create_by_google(&app, "g1", Some("ann@gmail.com"), None)
        .await
        .unwrap();
    assert_eq!(again, created);

    // An existing email account gets linked to a Google user with that
    // trusted address, and remembers the link.
    let email_account = account::find_or_create_by_email(redis, "bob@example.com").await.unwrap();
    let linked = account::find_or_create_by_google(&app, "g2", Some("bob@example.com"), Some("Bob"))
        .await
        .unwrap();
    assert_eq!(linked.id, email_account.id);
    assert_eq!(linked.google_id.as_deref(), Some("g2"));
    assert_eq!(linked.display_name, email_account.display_name, "linking keeps the account's name");
    let stored = account::get(redis, &linked.id).await.unwrap().unwrap();
    assert_eq!(stored.google_id.as_deref(), Some("g2"));

    // Without a trusted email: a separate account with no email, which never
    // claims the address (an email sign-in for it is still its own account).
    let untrusted = account::find_or_create_by_google(&app, "g3", None, Some("Cy")).await.unwrap();
    assert_eq!(untrusted.email, None);
    assert_eq!(untrusted.google_id.as_deref(), Some("g3"));
    let cy_email = account::find_or_create_by_email(redis, "cy@example.com").await.unwrap();
    assert_ne!(cy_email.id, untrusted.id);

    // Display name falls back to the email's local part, then "Google user".
    let unnamed = account::find_or_create_by_google(&app, "g4", Some("dee@gmail.com"), Some("  "))
        .await
        .unwrap();
    assert_eq!(unnamed.display_name, "dee");
    let anonymous = account::find_or_create_by_google(&app, "g5", None, None).await.unwrap();
    assert_eq!(anonymous.display_name, "Google user");

    cleanup(&app).await;
}

#[cfg(feature = "ws")]
#[tokio::test]
#[ignore]
async fn ws_ticket_single_use_expiry_and_rate_limit() {
    use axum::{
        Json,
        body::Body,
        extract::{FromRequestParts, State},
        http::{Request, StatusCode},
        response::IntoResponse,
    };

    use crate::{Auth, WsAuth, ws};

    let app = test_app();
    let redis = app.redis();
    let account = account::find_or_create_by_email(redis, "ws@example.com").await.unwrap();

    let issue = || {
        let app = app.clone();
        let account = account.clone();
        async move { ws::ticket(State(app), Auth(account)).await }
    };
    let upgrade = |ticket: String| {
        let app = app.clone();
        async move {
            let uri = format!("/api/ws?ticket={ticket}");
            let (mut parts, _) = Request::get(uri).body(Body::empty()).unwrap().into_parts();
            WsAuth::from_request_parts(&mut parts, &app).await
        }
    };
    let status = |result: Result<WsAuth, crate::ApiError>| match result {
        Ok(_) => StatusCode::OK,
        Err(err) => err.into_response().status(),
    };

    // A ticket resolves to the requesting account exactly once.
    let Json(issued) = issue().await.unwrap();
    let ticket_key = redis.key(&format!("{}{}", keys::WS_TICKET, issued.ticket));
    let ttl = redis.command(&["TTL", &ticket_key]).await.unwrap().as_i64().unwrap();
    assert!(ttl > 0 && ttl <= keys::WS_TICKET_TTL_SECS as i64, "ttl was {ttl}");
    let WsAuth(authed) = upgrade(issued.ticket.clone()).await.unwrap();
    assert_eq!(authed, account);
    assert_eq!(status(upgrade(issued.ticket).await), StatusCode::UNAUTHORIZED);

    // A well-formed ticket that was never issued is rejected.
    assert_eq!(status(upgrade(random_hex(32)).await), StatusCode::UNAUTHORIZED);

    // An expired ticket is rejected.
    let Json(issued) = issue().await.unwrap();
    let ticket_key = redis.key(&format!("{}{}", keys::WS_TICKET, issued.ticket));
    redis.command(&["PEXPIRE", &ticket_key, "1"]).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert_eq!(status(upgrade(issued.ticket).await), StatusCode::UNAUTHORIZED);

    // Rate limit: 2 tickets so far; the rest of the window's allowance
    // succeeds, then 429. The counter carries the window's TTL.
    for _ in 2..keys::WS_TICKET_RATE_LIMIT {
        let _ = issue().await.unwrap();
    }
    let over = issue().await.expect_err("over the limit");
    assert_eq!(over.into_response().status(), StatusCode::TOO_MANY_REQUESTS);
    let counter = redis.key(&format!("{}{}", keys::WS_TICKET_RATE, account.id));
    let ttl = redis.command(&["TTL", &counter]).await.unwrap().as_i64().unwrap();
    assert!(ttl > 0 && ttl <= keys::WS_TICKET_RATE_WINDOW_SECS as i64, "ttl was {ttl}");

    cleanup(&app).await;
}
