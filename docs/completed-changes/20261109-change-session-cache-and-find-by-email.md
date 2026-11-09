# Session Cache and Find Account by Email

**Status:** done

Two framework additions needed by `neverending-contrivance`'s rework onto webapp, released together as `v0.5.0`. The full reasoning is in `C:\Data\Dropbox\dev\rust\neverending-contrivance\docs\completed-research\20261109-rework-to-use-webapp.md` (section "Framework change needed"). Its project plan is `C:\Data\Dropbox\dev\rust\neverending-contrivance\docs\projects\webapp-rework.md` (this is change 0 there).

## 1. Opt-in in-memory session + account cache

Today `Auth` makes two Upstash calls on every request (`GETEX FS<token>`, `GET FA<account_id>`). Single-server apps should be able to skip both on most requests.

- Off by default. Proposed as a `session-cache` Cargo feature (matching `ws` / `discord-activity`); a runtime builder option is the alternative to weigh in this change. Document it as **single-instance deployments only**.
  **Decision:** a runtime option set in code: `Config.session_cache: bool` (default `false`), which the app sets before `WebApp::new`. The cache needs no new crates and no endpoints, so a Cargo feature would save nothing at compile time but would double the feature test matrix (4 → 8) and add `#[cfg]` branches. It isn't read from an env var, so a deploy can't switch it on by mistake.
- `Auth` checks an in-memory map `token → (Account, last_refreshed, last_used)` first. On a miss it makes today's two calls and fills the entry.
- **Sliding TTL:** a hit refreshes the Redis session TTL (`EXPIRE FS<token> <ttl>`) only when the last refresh is older than an interval (about a day), so active sessions never expire in Redis.
  **Decision:** refresh after 1 day for website sessions, and after 1 hour for activity sessions (their TTL is only 24 hours, so a 1-day interval would let them expire while cached). If `EXPIRE` reports the key is gone, the entry is dropped and the request gets a 401.
- **Consistency:** `logout` removes the token from the cache. Every framework write to an `FA` record (currently Google linking in `account::find_or_create_by_google`) updates cached entries for that account.
  **Decision:** a generation counter, bumped by every removal and account update, guards against a request that read Redis just before a logout or account write and then fills the cache with the stale entry: a miss only inserts its entry if the generation hasn't changed since it started.
- **Memory:** entries idle longer than a cutoff (about an hour) are dropped by a sweep and reload from Redis on the next request.
  **Decision:** idle cutoff 1 hour. The sweep runs inline during a cache lookup at most every 10 minutes, so there's no background task (and no `tokio` dependency).
- Redis stays the source of truth, so a restart only empties the cache.

## 2. Find an account by email

Apps must not touch `F*` keys, but an app's admin screen needs email → account (e.g. "grant credits to *email*"). Expose something like `webapp_server::account::find_by_email(redis, email) -> Result<Option<Account>>`, normalizing the address the same way email login does and following `FE<email>`.

**Decision:** `pub async fn find_by_email(redis: &Redis, email: &str) -> Result<Option<Account>>`. An address that fails normalization returns `Ok(None)`. It reads Redis directly (no cache), since admin lookups are rare.

## Also

- Tests for both, including live Redis tests for cache refresh and invalidation.
- Update `docs/implementation-guide.md` and `CLAUDE.md` (new feature, new API).
- Release `v0.5.0` per the Releasing steps (pushing and tagging only when the user asks).

## Implementation Plan

**Files to add:**
- `crates/webapp-server/src/session_cache.rs` — `SessionCache`: `Mutex<State {entries: HashMap<token, Entry {account, refreshed, used}>, generation, last_sweep}>`. Synchronous methods only (the lock is never held across an `await`), taking `now: Instant` so unit tests can move time: `lookup(token, now) -> Lookup {Hit(Account) | Refresh(Account) | Miss {generation}}` (also runs the sweep when due and marks the entry used), `insert(token, account, generation, now)` (skipped if the generation moved), `refreshed(token, now)`, `remove(token)`, `update_account(&Account)`. Timing constants (refresh 1 day / activity 1 hour, idle 1 hour, sweep 10 minutes) and unit tests live here.

**Files to modify:**
- `crates/webapp-server/src/config.rs` — `pub session_cache: bool`, documented as single-instance only; `from_env` sets `false`.
- `crates/webapp-server/src/lib.rs` — `Inner` holds `Option<SessionCache>`, built in `WebApp::new` from the config; `pub(crate) fn session_cache()`. Module docs mention the option.
- `crates/webapp-server/src/session.rs` — `refresh(redis, token) -> Result<bool>` (`EXPIRE` with the kind's TTL) and `cache_refresh_interval(token)` (1 day, or 1 hour for an activity token), next to `ttl_secs`, which picks the TTL the same way. The cache's `lookup` takes the interval as a parameter.
- `crates/webapp-server/src/redis.rs` — `expire(key, ttl_secs) -> Result<bool>` helper.
- `crates/webapp-server/src/auth.rs` — move the lookup into `resolve_account(app, token, now) -> Result<Option<Account>>`: no cache → today's two calls; hit → return; refresh due → `EXPIRE`, drop + `None` if gone; miss → today's two calls, then `insert`. `logout` also removes the token from the cache.
- `crates/webapp-server/src/account.rs` — `pub async fn find_by_email`; `find_or_create_by_google` takes `&WebApp` and calls `update_account` right after it writes the linked `FA` record.
- `crates/webapp-server/src/google.rs` — pass `&app` to `find_or_create_by_google`.
- `crates/webapp-server/src/live_tests.rs` — new tests: cache fill and hit (proved by deleting `FA` behind the cache), refresh resets the Redis TTL, refresh of a deleted session drops it, logout invalidates, Google linking updates a cached entry; `find_by_email` (normalized hit, unknown, invalid). Update the Google test's call.
- `example/server/src/main.rs` — turn the cache on (the example is a single instance), with a comment saying why.
- `docs/implementation-guide.md` — §2 Server: session cache (how to enable, single-instance rule, behaviour) and `account::find_by_email`.
- `CLAUDE.md` — Framework API conventions: the cache and `find_by_email`; the session TTL bullet notes the cached refresh interval.
- `Cargo.toml` — workspace version `0.5.0`.

**Approach:**
`Auth` stays the single entry point, so `/api/auth/me`, app routes and `POST /api/auth/ws-ticket` all benefit. With the cache off, behaviour and Redis traffic are unchanged. Redis stays the source of truth: every cached session was refreshed in Redis within its refresh interval, which is shorter than its TTL, so the cache never serves a session Redis has already expired.

**Risks / things to watch for:**
- Multi-instance deployments would serve logged-out sessions from another machine's cache; the docs must say single instance only.
- A session deleted in Redis by anything other than this process's `logout` stays usable until its next refresh (at most a day). Today nothing else deletes sessions.
- Never hold the `std::sync::Mutex` across an `.await`.
- Release: commit and bump to `0.5.0`; tag and push only when the user asks.
