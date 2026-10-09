//! Tests against the real Upstash database configured in `.env`. Ignored by
//! default; run with `cargo test -p webapp-server -- --ignored`.
//!
//! Each run namespaces its keys under a random prefix and deletes them at the
//! end, so it never touches app data.

use webapp_common::Account;

use crate::{Config, WebApp, account, keys, random::random_hex, session};

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

    let token = session::create(redis, "acct1").await.unwrap();
    assert_eq!(session::resolve(redis, &token).await.unwrap().as_deref(), Some("acct1"));

    // Stored under the namespaced key, with the sliding TTL applied.
    let full_key = redis.key(&format!("{}{token}", keys::SESSION));
    assert!(full_key.starts_with("webapp-test:"));
    let ttl = redis.command(&["TTL", &full_key]).await.unwrap();
    let ttl = ttl.as_i64().unwrap();
    assert!(ttl > (keys::SESSION_TTL_SECS - 60) as i64, "ttl was {ttl}");

    session::delete(redis, &token).await.unwrap();
    assert_eq!(session::resolve(redis, &token).await.unwrap(), None);

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
