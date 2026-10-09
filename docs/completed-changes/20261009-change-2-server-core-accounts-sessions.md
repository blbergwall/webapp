# Change 2: Server Core: Redis, Accounts, Sessions

**Status:** done

Order: 2 of 6. Architecture source: `docs/completed-research/20261009-web-app-framework.md`.

Implement the foundation of `webapp-server` (and its shared types in `webapp-common`):

- **Upstash Redis REST client:** port `neverending-contrivance`'s `server/src/redis.rs`, with an optional `REDIS_KEY_PREFIX` namespace. Export it publicly so apps can store their own data.
- **Key module:** the 2-letter prefixes and TTLs (`AC`, `IE`, `ID`, `SS`, `LC`, `RL`), documented the same way as `neverending-contrivance`'s `keys.rs`.
- **IDs and tokens:** generated with `getrandom` and hex-encoded.
- **Accounts:** `AC<account_id>` records holding `{id, created, display_name, email?, discord_id?}`, plus identity maps `IE<email>` and `ID<discord_id>` that point to `account_id`. Each identity is claimed with `SET NX` so two concurrent creates can't make two accounts.
- **Sessions:** `SS<token>` maps to `account_id`, with a 90-day sliding TTL.
- **`Auth` extractor:** accepts `Authorization: Bearer` or the session cookie. Cookie parsing and building are hand-written (no `axum-extra`).
- **`auth_router()` routes:**
  - `GET /api/auth/config`
  - `GET /api/auth/me`
  - `POST /api/auth/logout`
- **CSRF:** state-changing endpoints accept JSON bodies only.
- **Tests:** unit tests for token generation, cookie parsing, and the extractor's header/cookie precedence.

## Clarifications

The change is one cohesive foundation (the other changes build on it), so it isn't split.

- **Which Upstash Redis database should the example app use for local dev/testing?**
  **Decision:** A new free-tier Upstash database just for webapp. Its REST URL and token go in a gitignored `.env` at the repo root; a committed `.env.example` lists the variable names.
- **What should an email-created account's initial `display_name` be?**
  **Decision:** The email's local part (e.g. `jane` from `jane@example.com`). Discord accounts use the Discord global name, falling back to the username (change 4). Apps can let users rename.
- **How do apps wire the library in?** (decided in planning)
  **Decision:** Idiomatic axum state, no globals. `WebApp` is a cheap-to-clone handle (`Arc` inside) holding the config and the Redis client. `auth_router()` and the `Auth` extractor work with any app state `S` where `WebApp: FromRef<S>`. An app with no state of its own can use `WebApp` itself as its state.
- **How is CSRF enforced?** (decided in planning)
  **Decision:** In the `Auth` extractor, so it automatically covers every app endpoint, not just the framework's. When a request is authenticated by **cookie** and its method isn't GET/HEAD/OPTIONS, it must carry `Content-Type: application/json`, or the request gets 403. Bearer-token requests are exempt, since a cross-site attacker can't attach the header. Logout applies the same rule.
- **Bearer vs. cookie precedence:** if an `Authorization: Bearer` header is present, it wins. An invalid bearer token is a 401, with no fallback to the cookie.
- **Sliding TTL:** sessions are read with `GETEX key EX <ttl>`, which refreshes the TTL in the same round trip as the read.

## Implementation Plan

**Files to modify:**
- `Cargo.toml`: add workspace dependencies `serde`, `serde_json`, `reqwest` (`default-features = false`, `rustls` + `json`), `getrandom`, `anyhow`, `dotenvy`.
- `.gitignore`: un-ignore `.env.example` (`!.env.example`).
- `crates/webapp-common/Cargo.toml` + `src/lib.rs`: add `serde`. Define the shared types `Account {id, created, display_name, email, discord_id}` (`created` in unix seconds) and `AuthConfig {discord_client_id}`.
- `crates/webapp-server/Cargo.toml`: depend on `webapp-common`, `axum`, `tokio`, `reqwest`, `serde`, `serde_json`, `getrandom`, `anyhow`; `dotenvy` as a dev-dependency only (for the live Redis test).
- `crates/webapp-server/src/lib.rs`: module declarations; `WebApp` (`from_env()`, `redis()`, `config()`); `auth_router()`; re-exports.
- `example/server/Cargo.toml` + `src/main.rs`: load `.env` with `dotenvy`, build `WebApp::from_env()`, merge `auth_router()`, and add a protected `GET /api/hello` using `Auth`.
- `CLAUDE.md`: document the `.env` setup and the live Redis test command.

**Files to add:**
- `crates/webapp-server/src/config.rs`: `Config` read from env: `UPSTASH_REDIS_REST_URL`, `UPSTASH_REDIS_REST_TOKEN`, optional `REDIS_KEY_PREFIX`, optional `DISCORD_CLIENT_ID`.
- `crates/webapp-server/src/redis.rs`: `Redis` client struct, ported from `neverending-contrivance` but holding one reused `reqwest::Client`, URL, token, and prefix (the reference read env vars and built a new client on every call). Public `command()` (raw, unprefixed) and `key()` (applies the prefix) so apps can run any command. Typed helpers take unprefixed keys and apply the prefix: `get`, `getex`, `getdel`, `set`, `set_ex`, `set_nx_ex`, `del`.
- `crates/webapp-server/src/keys.rs`: documented prefixes and TTLs: `AC`, `IE`, `ID`, `SS` (90d), `LC` (24h), `RL` (2 min).
- `crates/webapp-server/src/random.rs`: `random_hex(bytes)` using `getrandom::fill`. 32 bytes for session tokens, 16 for account IDs.
- `crates/webapp-server/src/account.rs`: `get(id)` and the crate-internal `find_or_create(identity_key, make_account)`, which changes 3 and 4 wrap for email and Discord. It writes `AC<newid>` first, then claims the identity with `SET NX`. If another request won the claim, it deletes its own orphan `AC` record and uses the winner's ID, so a reader never finds an identity that points at a missing account.
- `crates/webapp-server/src/session.rs`: `create(account_id) -> token`, `resolve(token) -> Option<account_id>` (via `GETEX`), `delete(token)`; hand-written cookie helpers: `cookie_value(headers, name)`, `session_cookie(token)` (`HttpOnly; Secure; SameSite=Lax; Path=/; Max-Age`), `clear_session_cookie()`.
- `crates/webapp-server/src/auth.rs`: the `Auth(pub Account)` extractor (`FromRequestParts`). A pure `session_token(parts)` returns the token and its source (bearer or cookie); a pure `csrf_ok(parts)` applies the JSON rule. Handlers for `config`, `me`, and `logout`.
- `crates/webapp-server/src/error.rs`: `ApiError {Unauthorized, Forbidden, Internal(anyhow::Error)}` implementing `IntoResponse`. Internal errors are logged with `eprintln!` and return a generic 500 body, so error details never leak to clients.
- `.env.example`: the variable names with comments.
- `crates/webapp-server/src/live_tests.rs`: `#[ignore]` tests against the real Upstash DB (run with `cargo test -p webapp-server -- --ignored`). They cover the account create/claim race, session create/resolve/delete, and the key prefix. Each run uses a random key prefix and deletes its keys afterwards. *(Changed during implementation: this was `tests/redis_live.rs`, but a `tests/` integration test can't reach the crate-private `account::find_or_create`, so it became an in-crate `#[cfg(test)]` module.)*

**Approach:**
- Port the proven reference code, improved only where the framework needs it: a struct instead of env reads on every call, a key prefix, and a reused HTTP client.
- No `tracing`, `axum-extra`, `time` or `uuid`.
- Unit tests cover token generation, cookie parsing, bearer/cookie precedence, and the CSRF rule. The live test covers the Redis paths.

**Risks / things to watch for:**
- reqwest 0.13's `rustls` feature uses `aws-lc-rs`, which can need cmake/NASM to build on Windows. If the build fails, switch to `rustls-no-provider` plus `ring`.
- `Secure` cookies over `http://localhost`: Chrome and Firefox accept them on localhost, but confirm this in change 5.
- Every `Auth` request costs one Upstash round trip (the `GETEX`), plus one `GET` for the account: 2 per request. That's acceptable for now. Caching the account in the session value could be considered later.

## User Feedback

- New Upstash dev DB created and `.env` filled in by the user. Live tests (`cargo test -p webapp-server -- --ignored`) pass: account create race, session lifecycle and TTL, and key prefix. No `webapp-test:*` keys were left behind. Against the real DB, the example server returns 401 for an unknown bearer token.
