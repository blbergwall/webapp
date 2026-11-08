# Change: Discord Activity Behind a Cargo Feature

**Status:** done

Source: `docs/completed-research/20261108-separate-discord-activity-support.md` (the "Activity-only vs. shared" table and the Recommendation).

Make Discord activity support opt-in, so website-only apps don't ship the Discord SDK, can't be put into activity mode by accident, and don't carry activity-only endpoints, config and docs. Keep the server-side Discord OAuth core, which a future web "Log in with Discord" will share, unconditional.

- **New `discord-activity` Cargo feature, off by default,** on `webapp-common`, `webapp-server` and `webapp-client`, mirroring `ws`. The server and client features enable `webapp-common/discord-activity`. Apps opt in with `features = ["discord-activity"]` on both dependencies.
- **Behind the feature (activity-only):**
  - Client: the vendored SDK snippet (`webapp-client/src/discord.rs`, `authorize`, `authenticate_discord_sdk`, `is_discord_activity`); activity mode in `<AuthGate>` (`frame_id` detection, the `DiscordSignedOut` stage, `discord_sign_in`); the bearer token store in `api.rs`. With the feature off, `<AuthGate>` is always in website mode and `discord.js` is neither imported nor copied into `dist/`.
  - Server: `POST /api/auth/discord`; the `Authorization: Bearer` branch of `Auth` and its CSRF exemption; the activity session kind (`dc_` prefix, 24-hour TTL).
  - Common: `DiscordLoginRequest` / `DiscordLoginResponse`.
- **Stays unconditional (shared with the future web login):** `DiscordConfig`, the `oauth2/token` code exchange, the `users/@me` lookup and `DiscordUser`, `account::find_or_create_by_discord`, the `FD` identity key, `Account.discord_id`. Split server `discord.rs` into the shared core (`discord.rs`) and the activity login handler (`activity.rs`, gated by one `mod` line).
- **Rename activity-only identifiers** so they won't be confused with a web Discord login: e.g. `SessionKind::Discord` → `SessionKind::Activity`, `DISCORD_SESSION_TTL_SECS` → `ACTIVITY_SESSION_TTL_SECS`. Keep the `dc_` token value so live tokens keep their TTL.
- **Example app:** enable `discord-activity` on both example crates so it still demonstrates both modes.
- **Docs:** implementation guide gets a "Discord activity (optional)" section (feature flags; `no_sri` / `no_autoreload` in `Trunk.toml` are only needed with it; Developer Portal setup). Update `CLAUDE.md` (build/check commands, API conventions, auth API contract marking `/api/auth/discord` as feature-gated, Discord quirks section), `deploy.md` and `.env.example` comments.
- **Tests/checks:** all four feature combinations of `ws` × `discord-activity` build for `webapp-server` (host) and `webapp-client` (`wasm32-unknown-unknown`); tests pass with and without the feature; a website-only build of the client has no `snippets/…/discord.js` import.
- **Release:** breaking, so a new minor version (`v0.2.0`). Push and tag only when the user asks.
- **Dependent app (Deep Space, `../ds`):** its follow-up is `../ds/docs/backlog-changes/20261108-change-enable-webapp-discord-activity-feature.md` (enable the feature, bump to `v0.2.0`). Because ds's local `.cargo/config.toml` patches webapp to this working copy, ds local builds lose activity mode (silently, no build error) as soon as this change lands. Do the ds change soon after, and keep the feature name in that file in sync if it changes.

Open questions to settle during the change:
- Gate `AuthConfig.discord_client_id` now, or leave it until the web-login research reshapes `AuthConfig`?
  **Decision:** Leave it unconditional. It's harmless (`None` without Discord credentials; the client ID is public), and the web-login research will reshape `AuthConfig`.
- Feature name: `discord-activity` (explicit) vs. `activity` (shorter).
  **Decision:** `discord-activity`.

## Clarifications

- Should this change also bump the workspace version?
  **Decision:** Yes, set `[workspace.package] version = "0.2.0"` in this change. Tag and push only when the user asks.

## Implementation Plan

**Files to modify:**
- `Cargo.toml` (root) — `version = "0.2.0"`.
- `crates/webapp-common/Cargo.toml` — `discord-activity = []` feature.
- `crates/webapp-common/src/lib.rs` — gate `DiscordLoginRequest` / `DiscordLoginResponse`.
- `crates/webapp-server/Cargo.toml` — `discord-activity = ["webapp-common/discord-activity"]`.
- `crates/webapp-server/src/discord.rs` — keep only the shared OAuth core: `DiscordUser`, `exchange_code`, `fetch_user` (made `pub(crate)`). Move the `login` handler out. Until web login exists, the core has no callers without the feature, so the module gets `#[cfg_attr(not(feature = "discord-activity"), allow(dead_code))]` with a comment; its unit tests run in both builds.
- `crates/webapp-server/src/lib.rs` — `#[cfg] mod activity;`; add the `/api/auth/discord` route only with the feature; crate docs mention the feature.
- `crates/webapp-server/src/auth.rs` — `TokenSource::Bearer` and the `Authorization: Bearer` branch of `session_token` exist only with the feature. Without it the header is ignored and only the cookie counts (so the CSRF rule always applies). `csrf_ok` exempts non-cookie sources. Gate the bearer unit tests; add a test that a bearer header is ignored without the feature.
- `crates/webapp-server/src/session.rs` — `SessionKind::Discord` → `SessionKind::Activity` and `DISCORD_TOKEN_PREFIX` → `ACTIVITY_TOKEN_PREFIX` (value stays `dc_`), both gated; `ttl_secs` checks the prefix only with the feature.
- `crates/webapp-server/src/keys.rs` — `DISCORD_SESSION_TTL_SECS` → `ACTIVITY_SESSION_TTL_SECS`, gated; doc comments.
- `crates/webapp-server/src/live_tests.rs` — gate and rename the activity-session TTL test.
- `crates/webapp-client/Cargo.toml` — `discord-activity = ["webapp-common/discord-activity"]`.
- `crates/webapp-client/src/lib.rs` — gate `mod discord` and its re-exports (`authenticate_discord_sdk`, `is_discord_activity`).
- `crates/webapp-client/src/discord.rs` — absorbs the activity sign-in sequence from `gate.rs` (`sign_in()`: config → `authorize` → `POST /api/auth/discord` → store both tokens, returning the `Account`; `sign_out()`: clear both tokens), so `gate.rs` only needs a few `cfg` lines.
- `crates/webapp-client/src/api.rs` — gate the bearer `TOKEN` store, `set_token`, and the `Authorization` header.
- `crates/webapp-client/src/gate.rs` — gate `Stage::DiscordSignedOut`, its view arm, `discord_sign_in`, and the activity branches in start-up and logout. Without the feature `<AuthGate>` is always in website mode.
- `example/server/Cargo.toml`, `example/client/Cargo.toml` — `features = ["ws", "discord-activity"]`.
- `docs/implementation-guide.md` — tags → `v0.2.0`; new "Discord activity (optional `discord-activity` feature)" section (enabling it, `Trunk.toml` settings only needed with it, sessions, SDK, `authenticate_discord_sdk`, portal setup pointer); the server/client sections describe website mode as the default; WebSockets section numbering.
- `CLAUDE.md` — project summary, check commands (feature combinations), live-test command, `webapp-server`/`webapp-client` conventions, auth API contract (`/api/auth/discord` feature-gated), Discord quirks section.
- `README.md` — features, tags → `v0.2.0`.
- `docs/deploy.md` — Discord env vars and portal setup apply only to apps with the feature.
- `.env.example` — Discord comment.

**Files to add:**
- `crates/webapp-server/src/activity.rs` — the `POST /api/auth/discord` handler (moved from `discord.rs`), using the shared core.

**Approach:**
Mirror the `ws` feature: `#[cfg(feature = "discord-activity")]` on the activity-only items, with the shared Discord OAuth core left unconditional for the future web login. Keep the `cfg` footprint small by moving activity code into its own modules (`webapp-server/src/activity.rs`; the client's existing `discord.rs`) rather than sprinkling `cfg` through shared functions.

**Risks / things to watch for:**
- **Unused-code warnings** in each feature combination (e.g. an unused `source` parameter, unused imports). Check all four `ws` × `discord-activity` combinations on both library crates, with tests.
- **Behaviour change without the feature:** a request with `Authorization: Bearer` and a session cookie now authenticates by the cookie (and must pass CSRF), instead of 401-ing on the bearer. Intended: no bearer tokens exist in a website-only app.
- **Deep Space** (`../ds`) builds against this working copy through its `.cargo` patch, so it loses activity mode until its own follow-up change enables the feature. Don't touch ds in this change.
- Confirm a website-only client build has no `discord.js` import (build the example client once without the feature, or check `cargo check` plus a quick `trunk build` with the feature removed).

**Implementation notes:**
- `gate.rs`: activity names follow the rename (`Stage::ActivitySignedOut`, `activity_sign_in`); the client `discord.rs` gained `sign_in()` / `sign_out()` and `authorize` became private.
- `session::ttl_secs` carries `allow(unused_variables)` without the feature (its `token` is only read for the `dc_` check).
- Verified: no warnings in any of the four feature combinations for either library crate (`cargo check`/`clippy`, with tests); `cargo test -p webapp-server` passes in all four; live Redis tests pass with `ws,discord-activity`.
- Verified with `trunk build --release` of the example client into scratch dirs: **without** the feature, the JS glue has no snippet import and the wasm is 925 KB (vs. 980 KB with it). The website-only `dist/snippets/` still held a stale `webapp-client-45de136…/js/discord.js`: wasm-bindgen never cleans its output dir in the shared `CARGO_TARGET_DIR`, so old snippets get copied along. Nothing imports it, and a clean build (Fly's Docker build) doesn't have it.
- The snippet folder hash changed with the version bump (`webapp-client-67879044…`), so `ds-server`'s test that uses the old path as a literal is unaffected (it only checks the path shape).

## User Feedback

- Website mode tested on localhost (example app): all good.
- Activity mode (Discord, through the tunnel) was not tested before committing, at the user's choice; test it with the Deep Space follow-up change.
