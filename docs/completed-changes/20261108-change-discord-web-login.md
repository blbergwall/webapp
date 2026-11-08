# Change: Discord Web Login

**Status:** done

Source: `docs/completed-research/20261108-research-discord-web-login.md` (Findings and recommendations).

Add "Sign in with Discord" to website mode, next to the email form. It uses Discord's OAuth2 authorization-code flow and ends in the normal 90-day session cookie. It's always compiled (no Cargo feature) and enabled at runtime whenever `DISCORD_CLIENT_ID` / `DISCORD_CLIENT_SECRET` are set. It builds on the shared Discord core in `webapp-server/src/discord.rs`.

- **SPA-driven flow, mirroring email login (no server redirects, so trunk's dev proxy needs no `no_redirect`):**
  1. `POST /api/auth/discord/web/start` (JSON `{}`) → 200 `{url}`: the Discord authorize URL (`response_type=code`, `scope=identify`, `prompt=none`, `state`, `redirect_uri={PUBLIC_BASE_URL}/`). It sets a `discord_state` cookie holding a fresh random state (`HttpOnly; Secure; SameSite=Lax; Path=/api/auth/discord/web; Max-Age=600`). Returns 404 when Discord isn't configured. The client goes there with `window.location.assign(url)`.
  2. Discord sends the browser to `{PUBLIC_BASE_URL}/?code=…&state=…` (or `?error=…&state=…` when the user cancels).
  3. `<AuthGate>`'s website start sees `code` + `state` (or `error` + `state`), removes the query string, and calls `POST /api/auth/discord/web/confirm {code, state}`.
  4. Server: a missing or mismatched state cookie is a 400; the cookie is cleared on every confirm. It exchanges the code **with** `redirect_uri`, looks up `users/@me`, runs `find_or_create_by_discord` (same `FD` identity as the activity, so the same account), creates a `SessionKind::Website` session, sets the session cookie, and returns `Account`. A code Discord rejects is a 401, as today.
- **State in a cookie only** (no Redis record, no new `F?` prefix). It binds the flow to the browser to prevent login CSRF.
- **Accounts:** `identify` scope only, and email and Discord accounts stay separate. No auto-linking by email (rejected). Explicit linking is a possible later change.
- **Server:**
  - `discord.rs`: add `authorize_url(config, state, redirect_uri)`. `exchange_code` takes an optional `redirect_uri` (`None` for the activity). Remove the `allow(dead_code)` on `discord.rs` and `account::find_or_create_by_discord`.
  - New `discord_web.rs` with `start` / `confirm`, routed in `auth_router()`.
  - `session.rs`: `DISCORD_STATE_COOKIE` with set/clear helpers.
  - `config.rs`: require `PUBLIC_BASE_URL` when Discord credentials are set.
- **Common:** `DiscordWebStartResponse {url}`, `DiscordWebConfirmRequest {code, state}`.
- **Client (`gate.rs`):**
  - Website start: fetch `/api/auth/config` when showing the sign-in form. `discord_client_id: Some(..)` → show a "Sign in with Discord" button (`webapp-*` classes) next to the email form.
  - Handle the `code`/`state` and `error`/`state` returns, with a busy state and messages: "cancelled", "expired or invalid, try again", "couldn't reach the server".
  - `AuthConfig` is unchanged.
- **Example app:** shows the button when Discord is configured (no code change expected beyond what `<AuthGate>` provides).
- **Docs:**
  - `CLAUDE.md`: auth API contract rows and conventions.
  - Implementation guide: website sign-in methods, the `code`/`state` query names reserved on `/`, and the Developer Portal redirects.
  - `deploy.md`: under OAuth2 → Redirects, register `{PUBLIC_BASE_URL}/` for each environment, e.g. `http://localhost:8080/` and the production root. Keep the activity's `https://127.0.0.1` note, and say it's optional once a real redirect exists.
  - `.env.example` comment: Discord credentials now also enable web login, and need `PUBLIC_BASE_URL`.
- **Tests:** authorize-URL building (encoding, parameters); the state check (missing, mismatch, match); return-query parsing; `Config` requiring `PUBLIC_BASE_URL` with Discord credentials. Manual end-to-end test with a real Discord account on `http://localhost:8080` (also confirms that Discord accepts the `http://localhost` redirect).
- **Release:** new minor version (`v0.3.0`). Tag and push only when the user asks. **Apps that set Discord credentials (Deep Space) get the button automatically on upgrade.** Their upgrade must register `{PUBLIC_BASE_URL}/` redirects in each Discord app (dev and production), or the button lands on Discord's "Invalid OAuth2 redirect_uri" page.

## Clarifications

- Nothing ambiguous; the research settled the design.
- Version bump: as with the previous change, set `[workspace.package] version = "0.3.0"` in this change. Tag and push only when the user asks.
- Testing `Config::from_env` requiring `PUBLIC_BASE_URL` would mean mutating process env vars in tests (`set_var` is `unsafe` in edition 2024 and races other tests), so that rule is checked by hand instead of a unit test.

## Implementation Plan

**Files to modify:**
- `Cargo.toml` (root) — `version = "0.3.0"`.
- `crates/webapp-common/src/lib.rs` — add `DiscordWebStartResponse {url}` and `DiscordWebConfirmRequest {code, state}` (unconditional); `AuthConfig` doc: `Some` means Discord sign-in is available.
- `crates/webapp-server/src/discord.rs` — add `authorize_url(config, state, redirect_uri) -> String` (built with `reqwest::Url::parse_with_params`, so no new dependency); `exchange_code` takes `redirect_uri: Option<&str>` and adds it to the form when `Some`; drop the module-level `allow(dead_code)` and update the module docs. Unit test for the URL.
- `crates/webapp-server/src/activity.rs` — pass `None` as the redirect URI.
- `crates/webapp-server/src/account.rs` — drop the `allow(dead_code)` on `find_or_create_by_discord`.
- `crates/webapp-server/src/session.rs` — `DISCORD_STATE_COOKIE` plus `discord_state_cookie(value)` / `clear_discord_state_cookie()` (`HttpOnly; Secure; SameSite=Lax; Path=/api/auth/discord/web; Max-Age=600`). Unit test for the attributes.
- `crates/webapp-server/src/config.rs` — bail if Discord credentials are set without `PUBLIC_BASE_URL`.
- `crates/webapp-server/src/keys.rs` — `DISCORD_STATE_TTL_SECS = 600` next to the other TTLs (it's a cookie lifetime, not a Redis record, so no prefix).
- `crates/webapp-server/src/lib.rs` — `mod discord_web;`, routes `POST /api/auth/discord/web/start` and `POST /api/auth/discord/web/confirm` in `auth_router()`; crate docs.
- `crates/webapp-client/src/gate.rs` — in website mode: fetch `/api/auth/config` once (alongside `/me`) into a `discord_web: RwSignal<bool>`; `EmailForm` shows a "Sign in with Discord" button when it's true (start → `window.location.assign(url)`); `website_start` handles a Discord return (`code`+`state` → confirm; `error`+`state` → "cancelled" message) before the `login_code` / `/me` checks; error messages per status.
- `CLAUDE.md`, `README.md`, `docs/implementation-guide.md`, `docs/deploy.md`, `.env.example` — sign-in methods, auth API contract rows, `code`/`state` reserved on `/`, Developer Portal redirects (`{PUBLIC_BASE_URL}/` per environment), Discord credentials now also enabling web login and requiring `PUBLIC_BASE_URL`.

**Files to add:**
- `crates/webapp-server/src/discord_web.rs` — `start` (404 without Discord config; random state; returns `{url}` and sets the state cookie) and `confirm` (state cookie check → 400; clears the cookie on every response; exchange with `redirect_uri`; `users/@me`; `find_or_create_by_discord`; website session cookie; returns `Account`). The state check is a small pure function with unit tests (missing, mismatch, empty, match).

**Approach:**
Mirror email login's start/confirm pair on both sides: the server never redirects, the SPA navigates to Discord and handles the return on `/`, and every call is a JSON POST (so the CSRF rule holds and trunk's proxy needs no changes). Reuse the shared Discord core; the only core change is the optional `redirect_uri`. The redirect URI is always `{PUBLIC_BASE_URL}/`, used identically in the authorize URL and the token exchange.

**Risks / things to watch for:**
- **`redirect_uri` must match byte-for-byte** between the authorize URL, the token exchange and the portal registration. `PUBLIC_BASE_URL` is already stored without a trailing slash, so always append exactly `/`.
- The `discord_state` cookie must reach `confirm`: its `Path` must cover `/api/auth/discord/web/confirm`, and `Secure` works on `http://localhost` (as the session cookie already does).
- `confirm` must clear the state cookie on error responses too (an `ApiError` return loses headers), so build the response explicitly rather than with `?` after the cookie is read.
- `<AuthGate>` must strip `code`/`state`/`error` from the URL before calling `confirm`, so a reload doesn't replay them.
- With the `discord-activity` feature, activity mode must never show the web button or treat activity query params as a return.
- Manual test needs `http://localhost:8080/` registered as a redirect in the "webapp example" Discord app.

**Implementation notes:**
- As planned, with these details: the Discord button is `<button type="button" class="webapp-discord">` inside the existing `webapp-login` form (so apps' current form styling still applies); a return's query parsing shares `query_params()` / `clear_query()` with `take_login_code`; a Discord `error` other than `access_denied` shows a generic "failed, try again" (the raw value isn't displayed).
- `confirm` returns a `Response` built explicitly so the state-clearing `Set-Cookie` is on every outcome, errors included.
- Verified: no warnings (`clippy`) in all four `ws` × `discord-activity` combinations for both library crates; server tests pass in all four (new: authorize URL, state check, state cookie attributes); live Redis tests pass.
- Smoke-tested the example server with curl: `start` → 200 `{url}` with `redirect_uri=http%3A%2F%2Flocalhost%3A8080%2F`, `prompt=none`, `scope=identify` and the `discord_state` cookie; `confirm` without the cookie → 400; with a matching state and a junk code → 401 (Discord answered `invalid_grant`); both clear the cookie.

## User Feedback

- First try of the button gave Discord's "Invalid OAuth2 redirect_uri": the redirect had been pasted into the Developer Portal as ` http://localhost:8080/` (leading space), which the portal saved as-is. Removing the space fixed it, and Discord web sign-in on `http://localhost:8080` then worked. This also confirms Discord accepts an `http://localhost` redirect. Added a note to `docs/deploy.md` that Discord matches exactly and the portal keeps pasted whitespace.
- User tested on localhost: Discord sign-in, repeat sign-in, cancelling on Discord's page, and email sign-in all work.
