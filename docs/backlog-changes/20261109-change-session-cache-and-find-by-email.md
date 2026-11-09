# Session Cache and Find Account by Email

Two framework additions needed by `neverending-contrivance`'s rework onto webapp, released together as `v0.5.0`. The full reasoning is in `C:\Data\Dropbox\dev\rust\neverending-contrivance\docs\completed-research\20261109-rework-to-use-webapp.md` (section "Framework change needed"). Its project plan is `C:\Data\Dropbox\dev\rust\neverending-contrivance\docs\projects\webapp-rework.md` (this is change 0 there).

## 1. Opt-in in-memory session + account cache

Today `Auth` makes two Upstash calls on every request (`GETEX FS<token>`, `GET FA<account_id>`). Single-server apps should be able to skip both on most requests.

- Off by default. Proposed as a `session-cache` Cargo feature (matching `ws` / `discord-activity`); a runtime builder option is the alternative to weigh in this change. Document it as **single-instance deployments only**.
- `Auth` checks an in-memory map `token → (Account, last_refreshed, last_used)` first. On a miss it makes today's two calls and fills the entry.
- **Sliding TTL:** a hit refreshes the Redis session TTL (`EXPIRE FS<token> <ttl>`) only when the last refresh is older than an interval (about a day), so active sessions never expire in Redis.
- **Consistency:** `logout` removes the token from the cache. Every framework write to an `FA` record (currently Google linking in `account::find_or_create_by_google`) updates cached entries for that account.
- **Memory:** entries idle longer than a cutoff (about an hour) are dropped by a sweep and reload from Redis on the next request.
- Redis stays the source of truth, so a restart only empties the cache.

## 2. Find an account by email

Apps must not touch `F*` keys, but an app's admin screen needs email → account (e.g. "grant credits to *email*"). Expose something like `webapp_server::account::find_by_email(redis, email) -> Result<Option<Account>>`, normalizing the address the same way email login does and following `FE<email>`.

## Also

- Tests for both, including live Redis tests for cache refresh and invalidation.
- Update `docs/implementation-guide.md` and `CLAUDE.md` (new feature, new API).
- Release `v0.5.0` per the Releasing steps (pushing and tagging only when the user asks).
