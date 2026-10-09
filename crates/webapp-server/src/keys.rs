//! Redis key-prefix constants. Every record type gets a 2-letter prefix here,
//! documented with its key shape and what it stores, so this is the one place
//! to check before picking a new prefix. Apps built on the framework should
//! avoid these prefixes for their own keys.
//!
//! All keys are additionally namespaced by `Config::key_prefix`, which
//! `Redis` applies automatically.

/// `AC<account_id>` — account record (JSON `webapp_common::Account`). No TTL;
/// permanent once created.
pub const ACCOUNT: &str = "AC";

/// `IE<email>` — email identity: value is the `account_id` it belongs to.
/// Claimed with `SET ... NX` so two concurrent sign-ups can't create two
/// accounts for one address. No TTL.
pub const IDENTITY_EMAIL: &str = "IE";

/// `ID<discord_id>` — Discord identity: value is the `account_id` it belongs
/// to. Claimed with `SET ... NX`, like `IE`. No TTL.
pub const IDENTITY_DISCORD: &str = "ID";

/// `SS<token>` — session: value is the `account_id`. Sliding TTL, refreshed
/// (via `GETEX`) on every authenticated request.
pub const SESSION: &str = "SS";
/// The sliding TTL applied to a session on creation and on each use.
pub const SESSION_TTL_SECS: u64 = 90 * 24 * 60 * 60;

/// `LC<code>` — a one-time email login code (change 3). Consumed with
/// `GETDEL`.
pub const LOGIN_CODE: &str = "LC";
/// How long an unconsumed login code remains valid.
pub const LOGIN_CODE_TTL_SECS: u64 = 24 * 60 * 60;

/// `RL<ip>` — rate-limit gate for sending login emails (change 3). Value is
/// unused; the key's existence (and TTL) is the signal. Set via `SET ... NX EX`.
pub const RATE_LIMIT: &str = "RL";
/// How long an `RL<ip>` gate blocks further login emails from that IP.
pub const RATE_LIMIT_TTL_SECS: u64 = 2 * 60;
