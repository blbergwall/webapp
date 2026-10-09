use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use webapp_common::Account;

use crate::{keys, random::random_hex, redis::Redis};

/// Loads an account by id.
pub async fn get(redis: &Redis, account_id: &str) -> Result<Option<Account>> {
    let Some(json) = redis.get(&format!("{}{account_id}", keys::ACCOUNT)).await? else {
        return Ok(None);
    };
    let account = serde_json::from_str(&json)
        .with_context(|| format!("corrupt account record {account_id}"))?;
    Ok(Some(account))
}

/// Returns the account that `identity_key` (e.g. `IE<email>`, unprefixed)
/// points to, creating one with `make_account(new_id, created)` if the
/// identity is new.
///
/// The new `AC` record is written *before* the identity is claimed with
/// `SET NX`, so a reader can never follow an identity to a missing account.
/// If a concurrent request claims the identity first, this request's orphan
/// record is deleted and the winner's account is returned.
pub(crate) async fn find_or_create(
    redis: &Redis,
    identity_key: &str,
    make_account: impl FnOnce(String, u64) -> Account,
) -> Result<Account> {
    if let Some(account) = follow_identity(redis, identity_key).await? {
        return Ok(account);
    }

    let account = make_account(random_hex(16), now_secs());
    let account_key = format!("{}{}", keys::ACCOUNT, account.id);
    redis
        .set(&account_key, &serde_json::to_string(&account)?)
        .await?;

    if redis.set_nx_ex(identity_key, &account.id, None).await? {
        return Ok(account);
    }

    redis.del(&account_key).await?;
    follow_identity(redis, identity_key)
        .await?
        .with_context(|| format!("identity {identity_key} claimed but its account is missing"))
}

/// The account for a (normalized) email address, created on first sign-in
/// with the address's local part as its display name.
pub(crate) async fn find_or_create_by_email(redis: &Redis, email: &str) -> Result<Account> {
    let identity_key = format!("{}{email}", keys::IDENTITY_EMAIL);
    find_or_create(redis, &identity_key, |id, created| Account {
        id,
        created,
        display_name: email.split('@').next().unwrap_or(email).to_string(),
        email: Some(email.to_string()),
        discord_id: None,
    })
    .await
}

async fn follow_identity(redis: &Redis, identity_key: &str) -> Result<Option<Account>> {
    match redis.get(identity_key).await? {
        Some(account_id) => get(redis, &account_id).await,
        None => Ok(None),
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}
