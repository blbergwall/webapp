# Change: Sign in with Google

**Status:** done

Source: `docs/completed-research/20261108-add-login-with-google.md` (Findings, Follow-Up).

Add "Sign in with Google" to website mode, next to the email form and the Discord button. It's enabled at runtime whenever `GOOGLE_CLIENT_ID` / `GOOGLE_CLIENT_SECRET` are set (both or neither; requires `PUBLIC_BASE_URL`), with no Cargo feature. It uses Google's OpenID Connect authorization-code flow and ends in the normal 90-day session cookie.

- **Same SPA-driven flow as Discord web sign-in:**
  - `POST /api/auth/google/start` → `{url}` (`https://accounts.google.com/o/oauth2/v2/auth`, `response_type=code`, `scope=openid email profile`, `state`, `nonce` = the state, `redirect_uri={PUBLIC_BASE_URL}/`). It also sets a `google_state` cookie (`HttpOnly; Secure; SameSite=Lax; Path=/api/auth/google; Max-Age=600`).
  - Google returns to `{PUBLIC_BASE_URL}/?code=…&state=…` (or `?error=…&state=…`), and `<AuthGate>` calls `POST /api/auth/google/confirm {code, state}`.
  - `confirm` checks the state cookie (400) and clears it on every response. It exchanges the code at `https://oauth2.googleapis.com/token` (form, with `redirect_uri`) and reads the **ID token's** claims. Its signature isn't checked: Google says a server that got the token straight from the token endpoint can trust it. It checks `aud` = our client ID and `nonce` = the state, runs the account logic below, sets the session cookie, and returns `Account`.
- **Provider-prefixed state:** `discord.<hex>` and `google.<hex>`, so `<AuthGate>` routes a return on `/` to the right `confirm`. Discord's state changes to the prefixed form.
- **Match by email, only when Google is authoritative.** The email is trusted when `email_verified` and (`email` ends in `@gmail.com` or the `hd` claim is present).
  - `FG<sub>` exists → that account.
  - Else trusted and `FE<email>` exists → link: claim `FG<sub>` for it (`SET NX`; follow the winner on a lost race) and set its `google_id`.
  - Else create an account: `google_id: sub`, `email: trusted.then(email)`, display name from `name`, else the email's local part, else "Google user". Claim `FG<sub>`, and if trusted also `FE<email>` (`SET NX`; a lost race leaves the accounts separate).
  - Untrusted Google accounts are keyed by `sub` only, with `email: None`. Discord is unchanged (separate, `identify` only).
- **Server:**
  - New `google.rs` (authorize URL, code exchange, ID-token claims via a small hand-written base64url decoder, the trusted rule, `start` / `confirm`).
  - New shared `oauth.rs` (prefixed state generation, state cookie set/clear by name and path, `state_matches`; plain functions), and `discord_web.rs` moves onto it.
  - `account.rs`: Google find/link/create. `keys.rs`: new `FG` prefix in `FRAMEWORK_PREFIXES`.
  - `config.rs`: `GoogleConfig`.
- **Common:** `Account.google_id: Option<String>` (old records → `None`); `AuthConfig.google_sign_in: bool`; request/response types for start/confirm.
- **Client (`gate.rs`):** a "Sign in with Google" button (`webapp-google`) when `google_sign_in`; route returns by state prefix; messages for cancelled / expired / rejected.
- **Docs:**
  - API contract and conventions in `CLAUDE.md`.
  - Implementation guide: sign-in methods, the email-matching rule, `FG` in the key table, `Account.google_id`, and that the Google button's look must follow Google's branding guidelines (the app's CSS).
  - `deploy.md`: Google setup steps (project, Branding, Audience External + test users, Data Access scopes, Web client with exact redirect URIs `{PUBLIC_BASE_URL}/`, copy the secret at creation, Publish when live, optional brand verification) and the env var table.
  - `.env.example`, README.
- **Tests:**
  - Unit: base64url decoding, ID-token claims parsing, `aud`/`nonce` checks, the trusted-email rule (gmail, `hd`, unverified, other domain), state prefixes and routing.
  - Live Redis: the Google find / link / create paths with fake claims, including email sign-in reaching a Google-created account.
  - Manual: an end-to-end test with a real Google account (a test user) on `http://localhost:8080`.
- **Release:** new minor version (`v0.4.0`). Tag and push only when the user asks.

Open points to settle in the change:
- Shared `OAuthStartResponse` / `OAuthConfirmRequest` types for both providers vs. per-provider types.
  **Decision:** Shared. Both providers send identical shapes; `DiscordWebStartResponse` / `DiscordWebConfirmRequest` are renamed (fine in a minor bump, since only the framework's own client uses them).
- `prompt`: Google's default vs. `select_account`.
  **Decision:** Google's default. It already shows the account chooser when the browser has several Google accounts.

## Clarifications

- Nothing else ambiguous; the research and its Follow-Up settled the design. The change is cohesive (one feature plus the small shared helper Discord moves onto), so it stays one change.
- The user creates the Google Cloud project and Web client (per the research's setup steps) for the manual test; nothing in the code depends on it.

## Implementation Plan

**Files to modify:**
- `Cargo.toml` — `version = "0.4.0"`.
- `crates/webapp-common/src/lib.rs` — `Account.google_id: Option<String>`; `AuthConfig.google_sign_in: bool`; rename the Discord web types to `OAuthStartResponse {url}` / `OAuthConfirmRequest {code, state}`.
- `crates/webapp-server/src/config.rs` — `GoogleConfig {client_id, client_secret}` from `GOOGLE_CLIENT_ID` / `GOOGLE_CLIENT_SECRET` (both or neither); `PUBLIC_BASE_URL` required when set.
- `crates/webapp-server/src/discord_web.rs` — use `oauth.rs` (prefixed state, cookie helpers, state check, redirect URI, confirm response); keep only Discord-specific code.
- `crates/webapp-server/src/session.rs` — move the Discord state-cookie helpers out to `oauth.rs` (session.rs keeps session/pending cookies).
- `crates/webapp-server/src/keys.rs` — `IDENTITY_GOOGLE = "FG"` (in `FRAMEWORK_PREFIXES`); `DISCORD_STATE_TTL_SECS` → `OAUTH_STATE_TTL_SECS`.
- `crates/webapp-server/src/account.rs` — `find_or_create_by_google(redis, sub, trusted_email, name)`: follow `FG`; else link to the `FE` account (claim `FG` with `SET NX`, set `google_id`); else create (claim `FG`, then `FE` if trusted). Existing constructors set `google_id: None`.
- `crates/webapp-server/src/email.rs` — `normalize_email` becomes `pub(crate)` (Google's trusted email is normalized the same way before matching `FE`).
- `crates/webapp-server/src/auth.rs` — `AuthConfig.google_sign_in`.
- `crates/webapp-server/src/lib.rs` — `mod google; mod oauth;`, routes `POST /api/auth/google/start` and `/confirm`; docs.
- `crates/webapp-server/src/ws.rs` — test `Config` literal gets `google: None`.
- `crates/webapp-server/src/live_tests.rs` — `google_id: None` in the helper; new live test for the Google find/link/create paths (trusted → claims `FE` and email sign-in reaches it; existing email account → linked; untrusted → separate with `email: None`; repeat → same account).
- `crates/webapp-client/src/gate.rs` — generalize the Discord web pieces: a `Providers {discord, google}` signal from `/api/auth/config`; one `start_oauth` for both buttons (`webapp-discord`, `webapp-google`); a Discord/Google return is routed by the state prefix (`discord.` / `google.`); per-provider messages.
- `CLAUDE.md`, `README.md`, `docs/implementation-guide.md`, `docs/deploy.md`, `.env.example` — Google sign-in, the email-matching rule, `FG`, `Account.google_id`, API contract rows, Google setup steps, env vars, Google button branding is the app's job.

**Files to add:**
- `crates/webapp-server/src/oauth.rs` — shared by both providers: `Provider` constants (name, state cookie name and path), `new_state` (`<name>.<32 hex>`), state cookie set/clear, `state_matches`, `redirect_uri` (`{PUBLIC_BASE_URL}/`), and the `confirm` response (session cookie + state cookie cleared on every outcome). Plain functions and a data struct; no trait.
- `crates/webapp-server/src/google.rs` — authorize URL (`openid email profile`, `nonce` = state), code exchange (`invalid_grant` → 401), ID-token claims (hand-written base64url decode of the JWT payload + `serde_json`), `aud`/`nonce` checks, the trusted-email rule, `start` / `confirm` handlers. Unit tests for all of the pure parts.

**Approach:**
Factor the Discord web flow's provider-neutral parts into `oauth.rs`, then add Google as a second user of it, so both share one state/cookie/confirm pattern and the client routes returns by the state's provider prefix. Read Google's identity from the ID token returned by the token endpoint (Google-sanctioned without signature checks in this flow), which also gives `hd`. Match to email accounts only when Google is authoritative for the address.

**Risks / things to watch for:**
- **Email trust is the security-sensitive part:** only `email_verified && (@gmail.com || hd)`; compare on the normalized (lowercased) address; never claim `FE` for untrusted emails. Unit-test the rule thoroughly.
- **`aud`** in Google ID tokens is a string for this flow; parse it as a string (fail closed if it isn't).
- **Linking writes the `FA` record** (read-modify-write to set `google_id`); nothing else writes `FA` concurrently today.
- **Discord's state format changes** to `discord.<hex>`. In-flight Discord sign-ins at deploy time (10-minute window) fail with "expired, try again"; acceptable.
- **Google redirect-URI changes can take minutes to hours** to apply, and the app must be in Testing with the tester listed (or published) for the manual test.
- Keep all four `ws` × `discord-activity` combinations warning-free.

**Implementation notes:**
- As planned. `oauth.rs` holds a `Provider` data struct (`DISCORD`, `GOOGLE` constants: state prefix, cookie name, cookie path) with methods for state, cookies, the state check and the `confirm` response; both providers' handlers are now a few lines around it. The client mirrors it with a small `Provider` enum (paths, labels, classes, `from_state`) and an `Offered` flag pair from `/api/auth/config`.
- A nonce mismatch is a 400 (a mix-up the user can retry); a wrong `aud` is a logged 500 (misconfiguration).
- `Account.google_id` and `AuthConfig.google_sign_in` carry `#[serde(default)]`, so old `FA` records and an older server's config response still parse.
- Verified: no warnings (`clippy`, forced re-check) in all four feature combinations for both library crates; server tests pass in all four (new: base64url, ID-token parsing, trusted-email rule incl. look-alike domains, Google authorize URL, state prefixes, per-provider state cookies); live Redis tests pass, including the new Google create / link / untrusted / name-fallback test.
- Smoke-tested the example server with curl: `/api/auth/config` → `google_sign_in: false` (no Google credentials yet); Discord `start` now issues `discord.<hex>` states and `confirm` with a matching prefixed state + junk code → 401 with the cookie cleared; Google `start`/`confirm` → 404 when unconfigured (`confirm` still clears `google_state`).

## User Feedback

- **Space the provider buttons away from the email form** (hard to tell the email field only goes with "Send sign-in link"). The provider buttons are now wrapped in a `webapp-providers` div, rendered only when at least one provider is offered, so apps can set them apart. The example's CSS gives it a top margin, padding and a faint divider line. Documented in `CLAUDE.md` and the implementation guide.
- Verified by the user: Google client set up, Google sign-in tested end to end on `http://localhost:8080` and working; the provider-button spacing looks good.
