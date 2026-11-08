# Discord Web Login

**Status:** done

Add a web-based "Log in with Discord" option for website mode (a normal browser, not a Discord activity), alongside email magic-link login. It should be a standard OAuth2 authorization-code redirect flow that ends in the normal session cookie, and build on the server-side Discord OAuth core that stays unconditional after the `discord-activity` feature change (see `docs/completed-research/20261108-separate-discord-activity-support.md`).

Questions to research:
- Flow and endpoints: e.g. `GET /api/auth/discord/start` (redirect to Discord) and a callback route. How is the OAuth `state` generated, stored (cookie vs. Redis `F?` record) and checked (CSRF)?
- Redirect URI configuration for local dev, tunnels and production (derive from `PUBLIC_BASE_URL`?), and the Developer Portal setup it needs.
- Scopes: `identify` only, or also `email`? If `email`, should a verified Discord email match or link to an existing email account?
- Account linking: a person who signs in by email and by Discord gets two accounts today. Is linking in scope, and how would it work safely?
- Client: how `AuthConfig` and `<AuthGate>` show the Discord button next to the email form; how errors from the callback reach the SPA.
- Runtime enablement: on whenever `DISCORD_CLIENT_ID` / `DISCORD_CLIENT_SECRET` are set, or a separate switch? Does it need a Cargo feature (probably not: no SDK, little code)?
- Interaction with an app that also enables `discord-activity` (same Discord app and credentials; same `FD` identity).

## Clarifications

- Account linking: a person who signs in by email and by Discord gets two accounts today. How should Discord web login handle that?
  **Decision:** Research the options (keep separate, auto-link by verified email, explicit linking from a signed-in account) and recommend one.
- Which sign-in combinations should a website-mode app be able to offer?
  **Decision:** Email always, Discord optional. Email stays the base method; Discord adds a button next to the email form when configured.
- Runtime enablement: when should a website app offer Discord web login?
  **Decision:** Credentials alone. On whenever `DISCORD_CLIENT_ID` and `DISCORD_CLIENT_SECRET` are set, the same way email is enabled by `RESEND_API_KEY` / `DEV_MODE`. No separate switch or Cargo feature.

## Findings

### What Discord's OAuth2 docs say

From [docs.discord.com/developers/topics/oauth2](https://docs.discord.com/developers/topics/oauth2) and [the User resource](https://docs.discord.com/developers/resources/user):

- **Authorize URL:** `https://discord.com/oauth2/authorize?response_type=code&client_id=…&scope=identify&state=…&redirect_uri=…&prompt=…`. The `redirect_uri` must be one registered in the Developer Portal (a Discord application can register several).
- **`prompt`:** `consent` asks the user to approve again every time. `none` skips the approval screen for a user who has already authorized the app. The activity already uses `prompt: "none"`.
- **`state`:** "Discord does not require the use of the `state` parameter, [but] we support it and highly recommend that you implement it." It is returned unchanged on the redirect, and the client checks it against the stored value.
- **Token exchange:** `POST https://discord.com/api/oauth2/token`, form-encoded only, with `client_id`/`client_secret` in the body (what `discord::exchange_code` already does) or Basic auth. For a redirect flow it must include the same `redirect_uri` as the authorize request. The activity flow sends none, and must keep not sending one.
- **PKCE:** not documented for the authorization-code grant (only mentioned in passing). Not needed here anyway: the server is a confidential client that holds the secret.
- **Scopes:** `identify` gives `users/@me` without the email. `email` adds `email` and `verified` ("whether the email on this account has been verified"), and the consent screen then asks the user for access to their email address.

### Flow: redirect through the server, or through the SPA?

**The classic way: the server redirects both ways.** `GET /api/auth/discord/start` sets a state cookie and answers 303 to Discord. Discord sends the browser to `GET /api/auth/discord/callback?code&state`, which checks the state, signs in, sets the session cookie and answers 303 to `/`. The client is just a link.
- **Problem in local dev:** trunk's dev proxy **follows redirects by default**. Its per-proxy `no_redirect` option "defaults to `false`, i.e. yes, follow redirects automatically" ([trunk `proxy.rs`](https://docs.rs/crate/trunk/latest/source/src/config/models/proxy.rs)). So in `trunk serve`, `/api/auth/discord/start` would fetch Discord's HTML page server-side instead of sending the browser there, and the callback's redirect to `/` would hit the backend's `ServeDir`. Every app's `Trunk.toml` would need `no_redirect = true` on its `/api` proxy. It's fixable, but it's a trap each app has to know about.
- Errors (the user cancelled, a bad state) also have to travel back to the SPA somehow, e.g. as `/?login_error=…`.

**Recommended: the SPA drives the flow, mirroring email login.** No server redirects at all:
1. "Sign in with Discord" calls `POST /api/auth/discord/web/start` (JSON `{}`). The server returns `{url}`, the Discord authorize URL with a fresh random `state`, and sets a `discord_state` cookie holding that state. The client then navigates with `window.location.assign(url)`.
2. Discord sends the browser to `{PUBLIC_BASE_URL}/?code=…&state=…`, or `/?error=access_denied&state=…` if the user cancels. That page is just the SPA, served by trunk or `ServeDir`.
3. On load, `<AuthGate>`'s website start sees `code` + `state`, removes the query string (as it does with `login_code`), and calls `POST /api/auth/discord/web/confirm {code, state}`.
4. The server checks `state` against the cookie and clears the cookie. It then exchanges the code (with the same `redirect_uri`), looks up `users/@me`, runs `find_or_create_by_discord`, creates a normal **website session** (`SessionKind::Website`, 90-day cookie), sets the session cookie, and returns `Account`.

This is the same shape as `email/start` → link → `email/confirm`. Errors are ordinary HTTP statuses the SPA turns into messages on the sign-in view, the trunk proxy needs no changes, and JSON POSTs satisfy the CSRF rule.

### `state`: a cookie, not Redis

The `state` has to be bound to **this browser**, or an attacker could complete a sign-in with their own Discord code in a victim's browser (login CSRF). That requires a cookie either way. A Redis record would only add a write per click and a new `F?` prefix, so use the cookie alone:
- `discord_state=<32 random hex>; HttpOnly; Secure; SameSite=Lax; Path=/api/auth/discord/web; Max-Age=600`, like `pending_login`.
- `SameSite=Lax` is fine: the cookie is read by a same-origin `fetch` from the SPA, not on the cross-site redirect.
- `confirm` with a missing or mismatched state: 400. The cookie is cleared on every `confirm`, so a state works once.

### Redirect URI and configuration

- **`redirect_uri = {PUBLIC_BASE_URL}/`** (the SPA root). `PUBLIC_BASE_URL` already means "the origin users reach the site at" (it builds the email links): `http://localhost:8080` in dev, the real host in production.
- **Developer Portal (OAuth2 → Redirects):** register each environment's root, e.g. `http://localhost:8080/` and `https://<production host>/`. Discord treats `http://localhost` as an allowed exception to HTTPS (to confirm when implementing). Once a real redirect is registered, the activity's placeholder `https://127.0.0.1` is no longer needed, since any registered redirect satisfies the activity's `authorize`. Keeping it is harmless.
- **Enablement (decided: credentials alone):** Discord web login is on whenever `DISCORD_CLIENT_ID`/`SECRET` are set. `Config::from_env` should then require `PUBLIC_BASE_URL` (as it already does when email is enabled), so a missing redirect origin fails at startup, not at click time.
- **`AuthConfig` needs no change.** `discord_client_id: Some(..)` already means "Discord is configured", so in website mode it means "show the Discord button". The website start only has to fetch `/api/auth/config` when it shows the sign-in form.
- **Query names:** the SPA treats `code` + `state` on `/` as a Discord return. Apps shouldn't use those two parameter names on their root page.

### Scopes and account linking

The account options, given that each `FE`/`FD` identity points at one account and apps key their data by `account.id`:

| Option | How | Pros | Cons |
|---|---|---|---|
| **A. Keep separate** (today) | `identify` only. A Discord sign-in is the `FD` account. | Nothing new, safe. **Web Discord login and activity sign-in share `FD`, so they already reach the same account.** | Someone who signed up by email and then uses Discord gets a second, empty account. |
| **B. Auto-link by verified email** | `identify email`; if Discord reports `verified: true` and the email matches an `FE` identity, sign into that account (and claim `FE` for new Discord accounts). | Seamless for users with the same email on both. | Hands the email account to whoever controls a Discord account that once verified that address (recycled or shared addresses). Auth libraries ship this disabled for that reason (e.g. NextAuth's `allowDangerousEmailAccountLinking`). It also adds an email-access line to Discord's consent screen, and the activity, which only asks for `identify`, would behave differently. |
| **C. Explicit linking** | A signed-in user starts "link Discord" (or "link email") and completes that method's flow. The server attaches the new identity to the **current** account if it's unclaimed. | The user proves control of both identities at the same moment. No surprise merges. | New endpoints and UI hooks in both directions. It can't merge two accounts that already exist: the framework can't move the app's data, so a claimed identity gives a conflict. |

**Recommendation: A now, C as its own follow-up if users ask for it. Reject B.** Option A delivers web Discord login with no new account rules, and the shared `FD` identity is already the important link for apps like Deep Space: the same player on the website and in the activity. Explicit linking is independent of the sign-in flow (it reuses the same start/confirm steps with a "link" mode plus a session) and can be added later without changing anything decided here. So: **`identify` scope only**, `prompt=none`.

### Code shape (for the change)

- `webapp-server/src/discord.rs`: add `authorize_url(config, state, redirect_uri)`. Give `exchange_code` an optional `redirect_uri` (`None` for the activity). The core then always has a caller, so the `allow(dead_code)` on `discord.rs` and `find_or_create_by_discord` goes away.
- New `webapp-server/src/discord_web.rs`: the `start` and `confirm` handlers, routed in `auth_router()` (always compiled; 404 without Discord credentials).
- `session.rs`: `DISCORD_STATE_COOKIE` and its set/clear helpers.
- `webapp-common`: `DiscordWebStartResponse {url}`, `DiscordWebConfirmRequest {code, state}`.
- `webapp-client/src/gate.rs`: the email form gets a "Sign in with Discord" button when `discord_client_id` is set. The website start handles `code`/`state` and `error`/`state` returns, the busy state, and error messages ("cancelled", "expired, try again").
- `config.rs`: require `PUBLIC_BASE_URL` when Discord credentials are set.
- Docs: auth API contract rows, implementation guide (website sign-in methods, portal redirects), `deploy.md` portal setup, `.env.example`.
- Tests: authorize-URL building, state-cookie check (missing, mismatch, match), query parsing. Discord's side needs a manual end-to-end test with a real Discord account.

### Consequences and open points

- **Apps that already set Discord credentials for the activity (Deep Space) get the web button automatically** when they upgrade (decided: credentials alone). Until they register their redirect URIs in the portal, clicking it lands on Discord's "Invalid OAuth2 redirect_uri" error. The release notes and the ds upgrade must say to register `{PUBLIC_BASE_URL}/` for each environment.
- **Discord-call abuse (existing, not new):** anyone can call `start` and then `confirm` with junk codes, so every `confirm` costs one Discord token call. The activity's `/api/auth/discord` has the same exposure today. If Discord starts rate-limiting the app, add a per-IP limit like email's `FR` gate. Not proposed now.
- **Return path:** sign-in always lands on `/`. Returning to a deep link is out of scope (the SPAs are single-page today).
- **Old session on re-sign-in:** signing in while already holding a session cookie replaces the cookie, and the old `FS` record expires on its TTL. That's the same as email login today.

## Follow-Up

- The user reviewed the findings and accepted the recommended approach: the SPA-driven flow, state in a cookie, `identify` only with accounts kept separate, and enablement by credentials alone. That includes the consequence that apps with Discord credentials (Deep Space) get the button on upgrade and must register their redirect URIs.

## Next Steps

- Change: `docs/backlog-changes/20261108-change-discord-web-login.md`
