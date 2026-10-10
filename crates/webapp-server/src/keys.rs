//! Redis key-prefix constants. Every record type gets a 2-character prefix
//! here, documented with its key shape and what it stores, so this is the one
//! place to check before picking a new prefix.
//!
//! Prefix ownership (see `docs/implementation-guide.md`):
//! - `F*` — reserved for this framework. Every framework prefix starts with
//!   `F` (enforced by a test below).
//! - `A*`–`E*` — reserved for applications built on the framework.
//! - Other first letters — unassigned, kept free for e.g. reusable crates.
//!
//! All keys are additionally namespaced by `Config::key_prefix`, which
//! `Redis` applies automatically.

/// `FA<account_id>` — account record (JSON `webapp_common::Account`). No TTL;
/// permanent once created.
pub const ACCOUNT: &str = "FA";

/// `FE<email>` — email identity: value is the `account_id` it belongs to.
/// Claimed with `SET ... NX` so two concurrent sign-ups can't create two
/// accounts for one address. No TTL.
pub const IDENTITY_EMAIL: &str = "FE";

/// `FD<discord_id>` — Discord identity: value is the `account_id` it belongs
/// to. Claimed with `SET ... NX`, like `FE`. No TTL.
pub const IDENTITY_DISCORD: &str = "FD";

/// `FS<token>` — session: value is the `account_id`. Sliding TTL, refreshed
/// (via `GETEX`, in the same command as the read) on every authenticated
/// request. Discord sessions' tokens start with
/// `session::DISCORD_TOKEN_PREFIX`, which selects the shorter TTL.
pub const SESSION: &str = "FS";
/// Sliding TTL of a website (cookie) session: keeps users signed in across
/// visits.
pub const SESSION_TTL_SECS: u64 = 90 * 24 * 60 * 60;
/// Sliding TTL of a Discord activity session. Its token only lives in the
/// activity's memory and every launch signs in afresh, so an unused session
/// is abandoned; this short TTL cleans it up.
pub const DISCORD_SESSION_TTL_SECS: u64 = 24 * 60 * 60;

/// `FL<code>` — a one-time email login code: JSON `{email, pending}`.
/// Consumed with `GETDEL`.
pub const LOGIN_CODE: &str = "FL";
/// How long an unconsumed login code remains valid.
pub const LOGIN_CODE_TTL_SECS: u64 = 24 * 60 * 60;

/// `FR<ip>` — rate-limit gate for sending login emails. Value is unused; the
/// key's existence (and TTL) is the signal. Set via `SET ... NX EX`.
pub const RATE_LIMIT: &str = "FR";
/// How long an `FR<ip>` gate blocks further login emails from that IP.
pub const RATE_LIMIT_TTL_SECS: u64 = 2 * 60;

/// Every record prefix the framework uses. Add new ones here too, so the
/// ownership test covers them.
pub const FRAMEWORK_PREFIXES: &[&str] = &[
    ACCOUNT,
    IDENTITY_EMAIL,
    IDENTITY_DISCORD,
    SESSION,
    LOGIN_CODE,
    RATE_LIMIT,
];

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn framework_prefixes_are_two_chars_starting_with_f() {
        for prefix in FRAMEWORK_PREFIXES {
            assert_eq!(prefix.len(), 2, "{prefix:?} must be 2 characters");
            assert!(prefix.starts_with('F'), "{prefix:?} must start with F");
            assert!(prefix.chars().all(|c| c.is_ascii_alphanumeric()), "{prefix:?}");
        }
    }

    #[test]
    fn framework_prefixes_are_unique() {
        let unique: HashSet<_> = FRAMEWORK_PREFIXES.iter().collect();
        assert_eq!(unique.len(), FRAMEWORK_PREFIXES.len());
    }
}
