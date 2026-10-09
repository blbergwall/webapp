# Change 4: Discord Activity Login (Server)

**Status:** done

Order: 4 of 6. Depends on change 2. Architecture source: `docs/completed-research/20261009-web-app-framework.md`.

Port `full-rust`'s Discord code exchange to an HTTP endpoint in `webapp-server`:

- `POST /api/auth/discord {code}`:
  - exchanges the code at `discord.com/api/oauth2/token` using `DISCORD_CLIENT_ID` and `DISCORD_CLIENT_SECRET`
  - fetches `users/@me` (`identify` scope only)
  - finds the account via `ID<discord_id>`, or creates it with `display_name` set to `global_name`, falling back to `username`
  - creates a session
  - returns `{token, discord_access_token, account}`; no cookie is set, because activity mode uses a bearer token
- `GET /api/auth/config` includes `discord_client_id`. Discord is optional: if the env vars are unset, the Discord endpoint returns 404 and the config omits the client ID.
- Never trust identity data that comes from the client. Only the server-side exchange establishes who the user is.

## Clarifications

The change is small and self-contained, so it isn't split.

- **Which Discord application should webapp use for development?**
  **Decision:** A new Discord application ("webapp example") with Activities enabled, separate from `full-rust`'s, so URL mappings and tunnels don't collide. The user creates it and puts `DISCORD_CLIENT_ID` and `DISCORD_CLIENT_SECRET` in `.env`.
- **Config** (decided in planning): Discord login is enabled only when both env vars are set. Setting only one is a startup error, so a half-configured app fails loudly. `Config.discord_client_id` becomes `Config.discord: Option<DiscordConfig {client_id, client_secret}>`; `/api/auth/config` still returns only the client ID.
- **Token exchange encoding:** reqwest's `form` feature. Its `serde_urlencoded` is already in the dependency tree through axum, so no new crate is added.
- **No `redirect_uri`:** the Embedded App SDK's `authorize` doesn't use one, and `full-rust`'s exchange worked without it.
- **Response contract:**
  - `200 {token, discord_access_token, account}`.
  - `404` when Discord isn't configured.
  - `401` when Discord rejects the code (expired, reused, or forged). The Discord error body is logged server-side.
  - `500` when Discord is unreachable or returns something unexpected.
  - `400` for an empty code.
  - The refresh token is discarded.
- **Display name:** Discord's `global_name`, falling back to `username`, set only when the account is created. Keeping it in sync with later Discord name changes is out of scope.
- **Rate limiting:** none. Each call needs a fresh, valid, single-use Discord code, which an attacker can't mint.

## Implementation Plan

**Files to modify:**
- `Cargo.toml`: add `form` to the reqwest features.
- `crates/webapp-common/src/lib.rs`: add `DiscordLoginRequest {code}` and `DiscordLoginResponse {token, discord_access_token, account}`.
- `crates/webapp-server/src/config.rs`: replace `discord_client_id` with `discord: Option<DiscordConfig>`; `from_env` rejects a partial configuration.
- `crates/webapp-server/src/auth.rs`: read the client ID from `config.discord` for `/api/auth/config`.
- `crates/webapp-server/src/account.rs`: add `find_or_create_by_discord(discord_id, display_name)`.
- `crates/webapp-server/src/session.rs`: drop the remaining `#[allow(dead_code)]`, if any is left.
- `crates/webapp-server/src/lib.rs`: add `mod discord` and the `POST /api/auth/discord` route; re-export `DiscordConfig`.
- `crates/webapp-server/src/live_tests.rs`: add a test that `find_or_create_by_discord` keys on the Discord ID (same ID → same account, different ID → new account).
- `.env.example`, `CLAUDE.md`: add `DISCORD_CLIENT_SECRET` and a row for the endpoint in the API contract table.

**Files to add:**
- `crates/webapp-server/src/discord.rs`:
  - `login` handler: exchange the code at `https://discord.com/api/oauth2/token`, then `GET https://discord.com/api/users/@me` with the access token, then `find_or_create_by_discord`, then `session::create`.
  - Typed `TokenResponse` and `DiscordUser` structs instead of `full-rust`'s untyped `serde_json::Value` indexing.
  - Unit tests for display-name selection and parsing the `users/@me` payload.

**Approach:**
- Port `full-rust`'s `exchange_discord_code` onto the shared HTTP client and the change-2 account/session primitives.
- Live checks against real Discord (once the credentials are in `.env`): a made-up code should produce `invalid_grant`, which maps to 401. That proves the client ID and secret are accepted, because a bad secret produces `invalid_client` instead, and the log shows which. The full successful flow needs a real code from inside Discord, so it's verified in change 5.

**Risks / things to watch for:**
- Discord rate-limits the token endpoint. Not an issue at dev scale; 429s surface as 500 and are logged.
- **Never trust client data:** the request body carries only the code. The identity comes solely from `users/@me`, called with the token the server obtained itself.

## User Feedback

- **How are the Activity URL Mappings set up, and can the existing named Cloudflare tunnel from `ftv` be reused?**
  **Decision:** The user created a separate named tunnel `webapp-dev` (upgraded `cloudflared` to 2026.10.0) and routed `webapp-dev.fadetovoid.win` to it, so ftv keeps `dev.fadetovoid.win` and both can run at once. The dev command is `cloudflared tunnel run --url http://localhost:8080 webapp-dev`, and the Root Mapping is `/` → `webapp-dev.fadetovoid.win`. To be documented in `CLAUDE.md` in change 5.
- Verified through the tunnel: `/health` → `ok`, `/api/auth/config` returns the new app's client ID, and the trunk page loads.
- Credential check: the new Discord app's ID and secret are in `.env`. A made-up code through the tunnel → 401, with Discord's `invalid_grant` logged (not `invalid_client`), so the credentials are accepted. An empty code → 400. The full sign-in with a real code is verified in change 5.
