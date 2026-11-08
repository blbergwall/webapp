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
/// request. Discord activity sessions' tokens (`discord-activity` feature)
/// start with `dc_`, which selects the shorter TTL.
pub const SESSION: &str = "FS";
/// Sliding TTL of a website (cookie) session: keeps users signed in across
/// visits.
pub const SESSION_TTL_SECS: u64 = 90 * 24 * 60 * 60;
/// Sliding TTL of a Discord activity session (`discord-activity` feature).
/// Its token only lives in the activity's memory and every launch signs in
/// afresh, so an unused session is abandoned; this short TTL cleans it up.
#[cfg(feature = "discord-activity")]
pub const ACTIVITY_SESSION_TTL_SECS: u64 = 24 * 60 * 60;

/// Lifetime of the `discord_state` cookie: how long a Discord web sign-in
/// may take between "Sign in with Discord" and returning to the site. A
/// cookie, not a Redis record, so it has no prefix.
pub const DISCORD_STATE_TTL_SECS: u64 = 10 * 60;

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

/// `FW<ticket>` — single-use WebSocket ticket (`ws` feature): JSON
/// `webapp_common::Account` of the session that requested it. Consumed with
/// `GETDEL` by `WsAuth`.
pub const WS_TICKET: &str = "FW";
/// How long an unused WebSocket ticket remains valid.
pub const WS_TICKET_TTL_SECS: u64 = 30;

/// `FT<account_id>` — WebSocket ticket rate-limit counter (`ws` feature):
/// tickets issued to the account in the current window. `INCR`ed, with the
/// window's TTL set on the first hit, in one `EVAL`.
pub const WS_TICKET_RATE: &str = "FT";
/// Length of a ticket rate-limit window.
pub const WS_TICKET_RATE_WINDOW_SECS: u64 = 60;
/// Tickets one account may request per window (429 beyond that).
pub const WS_TICKET_RATE_LIMIT: i64 = 20;

/// Every record prefix the framework uses. Add new ones here too, so the
/// ownership test covers them.
pub const FRAMEWORK_PREFIXES: &[&str] = &[
    ACCOUNT,
    IDENTITY_EMAIL,
    IDENTITY_DISCORD,
    SESSION,
    LOGIN_CODE,
    RATE_LIMIT,
    WS_TICKET,
    WS_TICKET_RATE,
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
