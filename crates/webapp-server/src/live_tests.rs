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
