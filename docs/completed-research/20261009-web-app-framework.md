# Web App Framework

**Status:** done

I have done a few projects using a Rust server and a website front end. I would like to create something reusable so I don't need to redo this work.

Recently I did the project here: C:\Data\Dropbox\dev\rust\neverending-contrivance. It used an email login system that worked very well, and I would like that as part of this framework.

Some time ago I did the project here: C:\Data\Dropbox\gopath\mod\github.com\blbrust\full-rust. It was created to run as a Discord activity, so login used the Discord user. That worked well, so I would like this framework to also support running as a Discord activity.

So the goal here is to provide a framework that can handle authentication/login running as a plain website or a Discord activity. Decide on the overall architecture, create change tracking file(s) to do the implementation, and set up this project folder (create a CLAUDE.md, initialize a git repo here, ...).

Effort should be made to limit dependencies, and when they are needed, stick to the most minimalistic trusted options.

## Clarifications

Reference projects reviewed: `neverending-contrivance` (axum + askama server-rendered pages, passwordless email magic-link login via Resend, sessions in Upstash Redis REST, Fly.io) and `full-rust` (Cargo workspace: axum server + Leptos/trunk WASM client + `common` crate, Discord Embedded App SDK `authorize` → server code exchange over WebSocket, dev-mode placeholder auth outside Discord).

- **What form should the reusable framework take?**
  **Decision:** A library crate in this repo that apps depend on (git dependency), handling auth/sessions/storage, plus a small example/starter app that exercises it.
- **What front-end style should apps built on the framework use?**
  **Decision:** Rust/WASM SPA with Leptos (as in `full-rust`). All client-side code in all projects is to be Rust + WASM.
- **What should back storage (accounts, sessions, login codes)?**
  **Decision:** Upstash Redis over its REST API (as in `neverending-contrivance`).
- **Should a Discord login and an email login map to the same account?** If Discord's `email` scope were required, would users without a verified email (or unwilling to share it) be rejected?
  **Decision:** No one is rejected. Accounts get an internal account ID; each identity (`email`, `discord_id`) maps to that ID. Discord login requests only the `identify` scope (no email consent prompt). A later explicit "link email" flow (emailed short code, typed in) can attach an email identity to a Discord-created account or vice versa.

## Research Findings

### What the reference projects proved

- **`neverending-contrivance` email login:** `POST /login` checks an IP rate-limit gate (`RL<ip>`, `SET NX EX`, 2 min, IP from `Fly-Client-IP`). It creates a one-time code `LC<code>` (24h TTL) plus a "pending" value, emails a magic link via Resend's REST API, and sets an HttpOnly `pending_login` cookie. `GET /login/confirm` auto-completes only if the pending cookie matches the code's record (same browser). Otherwise it shows a "finish signing in" button, so email link scanners and prefetchers can't burn the code. The code is consumed with `GETDEL`. Sessions are `SS<token>` with a 90-day sliding TTL in an HttpOnly cookie. Accounts are keyed by email (`AC<email>`).
- **`neverending-contrivance` Redis access:** a ~200-line `redis.rs` that POSTs commands as a JSON array to Upstash REST with a bearer token, and handles Upstash's "HTTP 200 with `{error}`" case. It needs only `reqwest` + `serde_json`, which makes it a good candidate to reuse almost as-is.
- **`full-rust` Discord activity:** the Discord Embedded App SDK `authorize` call returns an OAuth2 code that goes to the server. The server exchanges it at `discord.com/api/oauth2/token` and then calls `users/@me`. The SDK is JS-only, so an inline `<script type="module">` exposes `window.initDiscord` and Rust calls it via `js_sys::Reflect`. Trunk needs `no_sri = true` because the proxy rewrites URLs, and `no_autoreload = true` because CSP blocks trunk's reload socket.

### Discord activity constraints (checked against current Discord docs)

- All traffic goes through the Discord proxy at `https://{clientId}.discordsays.com/...`, which runs on Cloudflare Workers. **CSP blocks any external URL that isn't a configured URL mapping.** `full-rust` imported the SDK from `esm.sh`. That only works if a URL mapping or a lenient CSP allows it, so it's fragile.
  → **Decision:** vendor a pinned, pre-bundled build of `@discord/embedded-app-sdk` (currently 2.5.0) as one ESM file in the client-support crate. Trunk inlines it into `index.html` (`<link data-trunk rel="inline" ...>`), so there are no external requests and no path-prefix problems.
- Only WebSockets and HTTP go through the proxy. WebRTC and WebTransport don't.
- Cookies *can* work through the proxy, but only with `SameSite=None; Partitioned` on the `discordsays.com` domain.
  → **Decision:** don't rely on cookies in activity mode. The activity re-runs `authorize` (`prompt: "none"`) on every launch anyway, so the session token stays in WASM memory and is sent as `Authorization: Bearer`. Website mode uses an HttpOnly cookie, which survives reloads and can't be read by JS.
- Don't trust identity claims from the Discord client. The server does the code exchange and the `users/@me` lookup itself, as `full-rust` did.
- **Activity detection:** Discord launches activities with `frame_id`, `instance_id` and `platform` query params. Checking for `frame_id` is more reliable than `full-rust`'s `window.self !== window.top`.

### Proposed architecture

**Cargo workspace (this repo):**

```
crates/
  webapp-common/   shared API types (serde only; WASM-compatible)
  webapp-server/   library: axum router + Auth extractor + Upstash REST + Resend + Discord exchange
  webapp-client/   library (wasm32): mode detection, Discord SDK bridge, auth API calls,
                   bearer-token handling, Leptos <AuthGate> component, vendored Discord SDK JS
example/
  server/          bin: merges webapp-server routes + one protected route, serves dist/
  client/          Leptos app (trunk) using webapp-client; login → "hello <name>"
```

Apps consume the libraries as git dependencies pinned to a tag, and are expected to keep the same shape as `example/`.

**One server, one origin.** The axum server serves both the trunk `dist/` (via `tower-http` `ServeDir`) and the `/api/...` routes. In website mode it runs on a custom domain. For the Discord activity, the Developer Portal URL mapping `/` points to the same Fly.io host. Everything is same-origin, so CORS never comes up.

**Accounts and identities.** Redis keys use 2-letter prefixes, plus an optional app-level `REDIS_KEY_PREFIX` so several apps can share one Upstash DB.

| Key | Value | TTL |
|---|---|---|
| `AC<account_id>` | JSON account `{id, created, display_name, email?, discord_id?}` | none |
| `IE<email>` | `account_id` (email identity), `SET NX` | none |
| `ID<discord_id>` | `account_id` (Discord identity), `SET NX` | none |
| `SS<token>` | `account_id` | 90d sliding |
| `LC<code>` | `{email, pending}` one-time email login code, `GETDEL` | 24h |
| `RL<ip>` | email-send rate-limit gate | 2 min |

`account_id` and session tokens are hex-encoded random bytes from the OS RNG (`getrandom`). The Redis client is exported so apps can store their own data under their own prefixes, keyed by `account_id`.

**Server API.** `webapp-server` exposes `auth_router()` for apps to merge:

- `GET  /api/auth/config` → `{discord_client_id}`. Config is fetched at runtime so builds aren't tied to an environment.
- `POST /api/auth/email/start {email}` → applies the rate limit, creates a code, sends the link `{PUBLIC_BASE_URL}/?login_code=...`, and sets the `pending_login` cookie.
- `POST /api/auth/email/confirm {code, auto}` → consumes the code, finds or creates the account via `IE<email>`, creates a session, and sets the session cookie. When `auto: true`, it refuses unless the `pending_login` cookie matches (the same-browser check from `neverending-contrivance`).
- `POST /api/auth/discord {code}` → exchanges the code, looks up `users/@me` (`identify` scope only), finds or creates the account via `ID<discord_id>`, creates a session, and returns `{token, discord_access_token, account}`. The access token is returned so the client can call SDK `authenticate` if the app needs other SDK commands.
- `GET  /api/auth/me` → returns the current account, or 401.
- `POST /api/auth/logout` → deletes the session and clears the cookie.
- **`Auth` extractor** (`FromRequestParts`) → accepts `Authorization: Bearer <token>` *or* the session cookie, refreshes the sliding TTL, and yields the `Account`. Apps put it on any protected handler.
- **CSRF:** cookie-authenticated state-changing endpoints only accept `application/json` bodies. A cross-site form can't send that without a CORS preflight, which is never granted. `SameSite=Lax` is a second layer.

**Email confirm inside the SPA.** The emailed link opens the SPA with `?login_code=`. Scanners that don't run JS never consume the code. The client first tries `confirm` with `auto: true`, which succeeds in the same browser. If that's refused, it shows a "Finish signing in" button. This keeps `neverending-contrivance`'s protection against scanners that do run JS.

**Client flow (`webapp-client`).** It first detects the mode (`frame_id` present → Discord).
- **Discord:** fetch the config, run SDK `ready` + `authorize({scope:["identify"], prompt:"none"})`, POST the code, and keep the token in memory.
- **Website:** call `/api/auth/me`. On 401, show the email form, or handle `?login_code=` if present.

`<AuthGate>` renders its children with the account in context once signed in. The `api_get` / `api_post` helpers attach the bearer token when there is one.

**Dev mode.** With `DEV_MODE=1` and no `RESEND_API_KEY`, the login link is printed to the console instead of emailed, so the website flow works locally without a mail provider. The Discord flow is tested through a Cloudflare tunnel, as in `full-rust`. The old `dev-placeholder` Discord bypass isn't needed, because outside Discord the app simply runs in website mode.

### Dependencies (minimal set; current versions checked 2026-10-09)

| Crate | Where | Why |
|---|---|---|
| `axum` 0.8 | server | web framework (both reference projects) |
| `tokio` 1 | server | runtime |
| `reqwest` 0.13 (rustls, json) | server | Upstash REST, Resend, Discord API |
| `serde` / `serde_json` | all | serialization |
| `tower-http` 0.7 (`fs` only) | server | serving `dist/` |
| `getrandom` 0.4 | server | tokens and IDs from the OS RNG |
| `anyhow` 1 | server | error plumbing (as in `neverending-contrivance`) |
| `leptos` 0.8 (`csr`) | client | UI (user decision) |
| `wasm-bindgen`, `wasm-bindgen-futures`, `js-sys`, `web-sys` | client | browser APIs; `fetch` called directly through `web-sys` |

**Considered and rejected:**
- **`axum-extra` cookies** (pulls in `cookie` + `time`): setting one `Set-Cookie` header and parsing one `Cookie` header is a few lines of code.
- **`rand` + `base64` + `uuid` + `chrono`:** `getrandom` plus hex encoding covers tokens and IDs, and timestamps are plain unix seconds.
- **`askama`:** an SPA has no server-rendered pages.
- **`gloo-net`:** `web-sys` fetch is enough.
- **`dotenvy`:** left to each app (the example may use it).
- **Hashing session tokens at rest (`sha2`):** would limit the damage if the DB leaked, but isn't worth a dependency now. Noted for later.
- **Loading the Discord SDK from a CDN:** fragile under Discord's CSP (see above).

### Open questions / later work (outside the first implementation)

- "Link email" flow for Discord-created accounts: an emailed short code the user types in, because a magic link can't return into the activity iframe. Plus the reverse direction.
- WebSocket support: an authenticated upgrade helper. Website mode can use the cookie. Activity mode authenticates with the first message, because browsers can't set headers on a WebSocket.
- Deployment template: a multi-stage Dockerfile that builds trunk + server, and `fly.toml`, following the `neverending-contrivance` pattern.
- Admin role / account management.

## Follow-Up

- **Are we ready to create change tracking file(s) to implement?**
  **Decision:** Yes. Research accepted as written. Implementation split into six ordered change files in `docs/backlog-changes/` (project setup, server core, email login, Discord login, client library + example app, deployment template).
