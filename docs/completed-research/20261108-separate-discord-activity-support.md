# Separate Discord Activity Support

**Status:** done

I have used this framework with one project so far, and it has worked very well. I have some other projects in mind that will not need Discord activity support. Is there an easy way to make that feature of this framework optional? Would it be better to make a new version of the framework with all the Discord activity code stripped out? Maybe move the Discord activity support into a separate crate that can be included or not? Note that one thing I am looking to add soon is a web-based "log in with Discord" feature (nothing to do with activities). Maybe that means the Discord libraries will need to stay at some level even if activities are not supported?

## Clarifications

- Does the existing app run as an activity, a website, or both?
  **Decision:** Both. Breaking it is fine: the user will update it to whatever new structure the split needs.
- How much of the planned web "Log in with Discord" feature should this research cover?
  **Decision:** Shape only. Work out which Discord code activity login and web login share, so the split puts the boundary in the right place. The web login itself gets its own research later.
- What matters most about making activity support optional?
  **Decision:** All three goals:
  1. **Smaller client:** don't ship or load the Discord SDK and activity code in website-only apps.
  2. **Simpler surface:** fewer endpoints, config, and docs for a website-only app to deal with.
  3. **Less risk:** a website-only app can't be put into activity mode by accident (for example by a `?frame_id=` URL).

## Findings

### What activity support costs a website-only app today

Server side, activity login is already optional **at runtime**. If `DISCORD_CLIENT_ID` and `DISCORD_CLIENT_SECRET` are unset, `POST /api/auth/discord` returns 404, and no crate is pulled in for it alone (`reqwest` is already used for Upstash and Resend; only its `form` feature exists for the Discord token exchange). The rest is always compiled in and always reachable:

| Cost | Where | Goal it hurts |
|---|---|---|
| **The 151 KB Discord SDK bundle (45 KB gzipped) is downloaded and evaluated on every page load.** `build/dist/example-client-*.js` starts with a static `import { discordAuthorize } from './snippets/webapp-client-…/js/discord.js'`. The import exists because `<AuthGate>`'s activity branch is reachable (it's chosen at runtime), so wasm-bindgen can't drop it. For scale: the example's wasm is 711 KB (214 KB gzipped), so the SDK adds about 20% to the bytes downloaded. | `webapp-client/src/discord.rs`, `js/discord.js` | Smaller client |
| Activity sign-in code in the wasm: the `DiscordSignedOut` stage, `discord_sign_in`, the bearer token store in `api.rs`. It's small next to the SDK. | `webapp-client/src/gate.rs`, `api.rs` | Smaller client |
| `<AuthGate>` enters activity mode whenever the URL has a `frame_id` query param. In a website-only app that leads to a "Discord sign-in isn't configured" error screen instead of the email form. | `gate.rs`, `discord::is_discord_activity` | Less risk |
| `Auth` accepts `Authorization: Bearer` and skips the CSRF check for it. Nothing but activity login issues bearer tokens. | `webapp-server/src/auth.rs` | Less risk, simpler surface |
| The `POST /api/auth/discord` route, `SessionKind::Discord` (the `dc_` token prefix and 24-hour TTL), and the `DiscordLoginRequest`/`DiscordLoginResponse` types. | `webapp-server` `discord.rs`, `session.rs`, `keys.rs`; `webapp-common` | Simpler surface |
| The Discord-proxy workarounds in `Trunk.toml` (`no_sri = true`, `no_autoreload = true`). The implementation guide says every app **must** keep them, so website-only apps lose SRI and trunk's auto-reload for nothing. | `example/client/Trunk.toml`, `docs/implementation-guide.md` | Simpler surface |
| Docs (implementation guide, `CLAUDE.md`, `deploy.md`, `.env.example`) describe both modes as one feature. | docs | Simpler surface |

### Activity-only vs. shared with a future web "Log in with Discord"

A web login would be a standard OAuth2 authorization-code redirect: the server sends the browser to `discord.com/oauth2/authorize` with a `state` value, Discord redirects back to a server callback, and the server exchanges the code and sets the normal **session cookie**. It needs no SDK and no bearer token. Splitting today's code by that:

| Shared by both Discord logins (keep unconditional) | Activity-only (make optional) |
|---|---|
| `DiscordConfig` (`DISCORD_CLIENT_ID` / `DISCORD_CLIENT_SECRET`) | The vendored SDK `js/discord.js`, `bridge.src.js`, `THIRD-PARTY-NOTICES`, and `webapp-client/src/discord.rs` (`authorize`, `authenticate_discord_sdk`, `is_discord_activity`) |
| The code exchange against `oauth2/token` (web login adds a `redirect_uri` parameter) | Activity mode in `<AuthGate>`: `frame_id` detection, the `DiscordSignedOut` stage, `discord_sign_in` |
| `users/@me` lookup and `DiscordUser::display_name` | The bearer token: the store in `webapp-client/src/api.rs`, and the `Authorization: Bearer` branch plus its CSRF exemption in `Auth` |
| `account::find_or_create_by_discord`, the `FD` identity key, `Account.discord_id` | `POST /api/auth/discord`, which returns a bearer token and `discord_access_token` |
| | `SessionKind::Discord` (the `dc_` prefix and `DISCORD_SESSION_TTL_SECS`): a web Discord login would use the 90-day cookie session |
| | `DiscordLoginRequest` / `DiscordLoginResponse`; `AuthConfig.discord_client_id` (only the SDK needs the client ID; a web login redirects from the server) |
| | The `no_sri` / `no_autoreload` Trunk settings |

So your guess is right: "Discord" and "Discord activity" are separate concerns. The server-side Discord OAuth core is small, adds no dependencies, and will be shared with web login, so it should stay in the framework unconditionally and be enabled at runtime by the credentials, the same way email login is enabled by `RESEND_API_KEY` / `DEV_MODE`. Everything heavy or risky (the SDK, bearer tokens, `frame_id` mode) is activity-only.

A naming consequence: once web login exists, "Discord session" becomes ambiguous (a web Discord login gets a cookie session). The activity-only identifiers should say *activity*, e.g. `SessionKind::Activity` and `ACTIVITY_SESSION_TTL_SECS`. The `dc_` token value can stay, so live tokens keep their TTL.

### Options considered

**A. A `discord-activity` Cargo feature (recommended).** It works exactly like the existing `ws` feature: off by default on `webapp-server`, `webapp-client` and `webapp-common`, with the activity-only column above behind `#[cfg(feature = "discord-activity")]`.
- Smaller client: with the feature off, the `#[wasm_bindgen(module = "/js/discord.js")]` block isn't compiled, so the snippet is neither imported nor copied into `dist/`, and the activity code is gone from the wasm.
- Simpler surface: no `/api/auth/discord` route, no bearer acceptance, no activity types, and website-only apps can keep trunk's SRI and auto-reload. The docs can move activity material into its own "Discord activity (optional)" section.
- Less risk: `<AuthGate>` is always in website mode. `frame_id` is never read and bearer headers are ignored, so an activity session can't be started or used.
- Cost: `cfg` attributes in `gate.rs`, `api.rs`, `auth.rs`, `session.rs`, `lib.rs` (both crates) and `webapp-common`; one more feature to check in CI-style commands (4 combinations with `ws`). No new abstractions, crates or dependencies.
- An activity app opts in on both sides: `webapp-server = { …, features = ["discord-activity"] }` and the same for `webapp-client`. A mismatch fails safe: a client with the feature talking to a server without it gets a 404 and shows "Sign-in failed".

**B. A separate crate pair (`webapp-discord-activity-server` / `-client`). Rejected.**
- Activity mode is built into `<AuthGate>` (mode detection, one stage machine, shared logout). A separate crate would need either a pluggable sign-in-provider trait in `AuthGate` (a new abstraction, against the design principles) or a duplicate `<ActivityAuthGate>`.
- The server-side handler needs `session::create`, `account::find_or_create_by_discord`, `WebApp::http()` and the client needs `api::set_token`, all `pub(crate)` today. A separate crate would force them into the public API, which works against the "simpler surface" goal.
- A separate crate pays off when the optional part has its own heavy dependencies or its own release cadence. Activity support has neither: its only extra "dependency" is a vendored JS file, and it has to be released in lockstep with `AuthGate` and `Auth`.
- Apps pin the whole repo by git tag anyway, so a separate crate saves nothing on download.

**C. A stripped-down fork with no activity support. Rejected.** Every fix to email login, sessions, Redis or WebSockets would have to be made twice, and the existing app would be left on the fork that still has activity support. A feature flag gets the same compiled result from a single codebase.

**D. Keep everything compiled in, but load the SDK lazily with a dynamic `import()`. Not a substitute; possible follow-on.** It would fix the download cost for any app (including your existing app's *website* visitors, who currently download the SDK too), but not the simpler-surface or less-risk goals. It also needs care: Discord's proxy rewrites URLs inside served JS, and wasm-bindgen snippets are imported statically, so the bridge would have to locate and import the bundle itself. It's worth a look only if the SDK download for website visitors of mixed apps turns out to matter. It stacks on top of option A.

### Recommendation

Do option A as one change, ready for `/change`:

1. Add a `discord-activity` feature (default off) to `webapp-common`, `webapp-server` and `webapp-client`, mirroring `ws`. The server and client features enable `webapp-common/discord-activity`.
2. Put the activity-only items listed above behind it. Keep the Discord OAuth core (config, code exchange, `users/@me`, `find_or_create_by_discord`, `FD`, `Account.discord_id`) unconditional so web login can build on it. Moving the activity handler into its own module (`discord.rs` = shared core, `activity.rs` = activity login) makes the boundary clear and keeps the `cfg` noise to one `mod` line.
3. Rename the activity-only identifiers to *activity* (`SessionKind::Activity`, `ACTIVITY_SESSION_TTL_SECS`, `is_discord_activity` can stay), keeping the `dc_` token value.
4. Enable the feature in `example/server` and `example/client`, so the example still shows both modes.
5. Docs: the implementation guide gets a "Discord activity (optional)" section (feature flags, `Trunk.toml` settings only needed with it, Developer Portal setup); `CLAUDE.md`'s build commands and API conventions and `deploy.md` follow. The auth API contract marks `/api/auth/discord` as feature-gated.
6. Release as a new minor version (`v0.2.0`, breaking), then add `features = ["discord-activity"]` to both dependencies in the existing app.

Open questions to settle in that change, not here:
- Should `AuthConfig.discord_client_id` be gated, or left alone until the web-login research reshapes `AuthConfig` (it will likely need a "Discord web login enabled" flag)?
- Exact feature name: `discord-activity` (explicit) vs. `activity` (shorter).

## Follow-Up

- The user reviewed the findings and agreed with the recommendation (option A).
- Should the web "Log in with Discord" option be a change right away, or research first?
  **Decision:** Research first. It has open design questions (OAuth `state` handling, redirect URI config, scopes, account linking, the `AuthConfig` / `<AuthGate>` UI) that should be settled before a change is planned.

## Next Steps

- Change: `docs/backlog-changes/20261108-change-discord-activity-cargo-feature.md`
- Research: `docs/backlog-research/20261108-research-discord-web-login.md`
- Change in the dependent app (Deep Space): `C:\Data\Dropbox\dev\rust\ds\docs\backlog-changes\20261108-change-enable-webapp-discord-activity-feature.md` (enable the feature and update to `v0.2.0`, after the webapp change and release)
