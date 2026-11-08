# Change: Discord Web Login

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
