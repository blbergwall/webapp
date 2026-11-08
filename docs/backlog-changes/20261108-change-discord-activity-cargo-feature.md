# Change: Discord Activity Behind a Cargo Feature

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
- Feature name: `discord-activity` (explicit) vs. `activity` (shorter).
