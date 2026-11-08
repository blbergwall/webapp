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

/// Returns the account that `identity_key` (e.g. `FE<email>`, unprefixed)
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
        google_id: None,
    })
    .await
}

/// The account for a Discord user, created on first sign-in with the given
/// display name. Shared by web and activity sign-in, so a Discord user
/// reaches the same account either way.
pub(crate) async fn find_or_create_by_discord(
    redis: &Redis,
    discord_id: &str,
    display_name: &str,
) -> Result<Account> {
    let identity_key = format!("{}{discord_id}", keys::IDENTITY_DISCORD);
    find_or_create(redis, &identity_key, |id, created| Account {
        id,
        created,
        display_name: display_name.to_string(),
        email: None,
        discord_id: Some(discord_id.to_string()),
        google_id: None,
    })
    .await
}

/// The account for a Google user (`sub`). `trusted_email` is the user's
/// normalized address, given only when Google is authoritative for it (see
/// `google::trusted_email`); it's what lets a Google sign-in and an email
/// sign-in for the same address reach one account:
///
/// 1. A known Google user (`FG<sub>`) gets their account.
/// 2. Otherwise, if an account already owns `FE<trusted_email>`, the Google
///    identity is linked to it.
/// 3. Otherwise a new account is created owning `FG<sub>` and, with a
///    trusted email, `FE<email>` too, so a later email sign-in finds it.
///
/// Without a trusted email the account is keyed by `sub` alone, with no
/// email, and never claims an address.
pub(crate) async fn find_or_create_by_google(
    redis: &Redis,
    sub: &str,
    trusted_email: Option<&str>,
    name: Option<&str>,
) -> Result<Account> {
    let google_key = format!("{}{sub}", keys::IDENTITY_GOOGLE);
    if let Some(account) = follow_identity(redis, &google_key).await? {
        return Ok(account);
    }

    if let Some(email) = trusted_email {
        let email_key = format!("{}{email}", keys::IDENTITY_EMAIL);
        if let Some(mut account) = follow_identity(redis, &email_key).await? {
            if !redis.set_nx_ex(&google_key, &account.id, None).await? {
                // A concurrent sign-in claimed the Google identity first.
                return follow_identity(redis, &google_key)
                    .await?
                    .with_context(|| format!("identity {google_key} claimed but its account is missing"));
            }
            account.google_id = Some(sub.to_string());
            let account_key = format!("{}{}", keys::ACCOUNT, account.id);
            redis.set(&account_key, &serde_json::to_string(&account)?).await?;
            return Ok(account);
        }
    }

    let display_name = name
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .or_else(|| trusted_email.and_then(|email| email.split('@').next()))
        .unwrap_or("Google user")
        .to_string();
    let account = find_or_create(redis, &google_key, |id, created| Account {
        id,
        created,
        display_name,
        email: trusted_email.map(str::to_string),
        discord_id: None,
        google_id: Some(sub.to_string()),
    })
    .await?;

    if let Some(email) = trusted_email {
        // Claim the address for this account. If a concurrent email sign-up
        // got there first, the two accounts simply stay separate.
        let email_key = format!("{}{email}", keys::IDENTITY_EMAIL);
        redis.set_nx_ex(&email_key, &account.id, None).await?;
    }
    Ok(account)
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
