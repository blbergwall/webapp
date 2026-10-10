# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

**webapp** is a reusable framework for Rust web apps that run either as a **plain website** or as a **Discord activity**, with authentication handled for both:

- **Website mode:** passwordless email (magic-link) login, with the session in an HttpOnly cookie.
- **Discord activity mode:** Discord Embedded App SDK `authorize` → server-side code exchange, with the session token held in WASM memory and sent as `Authorization: Bearer`.

Apps consume the library crates as git dependencies pinned to a tag, and follow the shape of `example/`.

The authoritative "why" behind the architecture is `docs/completed-research/20261009-web-app-framework.md`. The ordered implementation plan is the set of change files in `docs/backlog-changes/` (changes 1–6).

## Related Projects

- `neverending-contrivance` (`C:\Data\Dropbox\dev\rust\neverending-contrivance`): source of the email login flow, the Upstash Redis REST client (`server/src/redis.rs`), key-prefix conventions (`server/src/keys.rs`), and the Fly.io deploy pattern.
- `full-rust` (`C:\Data\Dropbox\gopath\mod\github.com\blbrust\full-rust`): source of the Leptos/trunk WASM client, the Discord activity auth flow, and the Discord proxy/CSP workarounds.

Check these for prior art before designing something from scratch.

## Development Process

All code changes must follow the structured development process. Use the `/change` skill to start any change. Don't make code changes without an active tracking file in `docs/active-change/` at step `planned` or later. Use the `/research` skill for open questions that need investigation before a change is planned.

## Design Principles

- **Minimize dependencies.** Prefer a few lines of code over a crate (e.g. cookies are parsed and built by hand, with no `axum-extra`). When a crate is needed, choose the most minimal, widely trusted option. See the dependency table in the research doc for what was considered and rejected.
- **Minimize abstractions.** Don't add layers, traits, or generics beyond what the current feature needs.
- **All client-side code is Rust + WASM** (Leptos). The only JS is the vendored Discord Embedded App SDK and its small bridge.

## Tech Stack

| Layer | Choice |
|---|---|
| Server framework | `axum` 0.8 + `tokio` |
| Outbound HTTP | `reqwest` (rustls, json): Upstash, Resend, Discord |
| Storage | Upstash Redis over its REST API |
| Email | Resend REST API |
| Client | Leptos 0.8 (`csr`), built with `trunk` |
| Serialization | `serde` / `serde_json` |
| Hosting | Fly.io: one server serves the trunk `dist/` and `/api/*` from a single origin |

## Crate Layout

```
crates/
  webapp-common/   shared API types (serde only; must stay WASM-compatible)
  webapp-server/   library: axum auth router, Auth extractor, Upstash REST, Resend, Discord exchange
  webapp-client/   library (wasm32): mode detection, Discord SDK bridge, auth API calls, <AuthGate>
example/
  server/          bin: merges the webapp-server routes, adds app routes, serves dist/
  client/          Leptos app built with trunk
```

`default-members` excludes `webapp-client` and `example-client` (wasm32-only), so `cargo build` / `cargo test` at the root build only the host crates.

## Build & Run

```sh
# Build/test host crates (workspace root)
cargo build
cargo test

# Type-check a client crate
cargo check -p example-client --target wasm32-unknown-unknown

# Run the example server (workspace root); binds BIND_ADDR, default 127.0.0.1:3000
cargo run -p example-server

# Run the example client dev server (from example/client); serves :8080 and proxies /api and /health to :3000
trunk serve

# Production client build (from example/client), outputs <repo>/build/dist/
trunk build --release
```

Prerequisites: `rustup target add wasm32-unknown-unknown` and `cargo install trunk`.

**Configuration:** copy `.env.example` to `.env` (gitignored) at the repo root and fill it in. The example server loads it with `dotenvy`; the framework itself only reads environment variables (`webapp_server::Config::from_env`). Local dev uses its own Upstash database, separate from other projects.

**Live Redis tests:** `cargo test -p webapp-server -- --ignored` runs the tests in `crates/webapp-server/src/live_tests.rs` against the Upstash DB in `.env`. Each run uses a random key prefix (`webapp-test:<hex>:`) and deletes its keys afterwards. Plain `cargo test` skips them.

**Local dev runs two processes. Open `http://localhost:8080`:**
- `cargo run -p example-server` runs the backend (axum, native Rust) on `:3000`: `/health` and `/api/*`. Restart it after server code changes.
- `trunk serve` runs the frontend dev server on `:8080`. It compiles the client to WASM, serves the page, rebuilds when client code changes (refresh the browser yourself, since auto-reload is disabled for Discord), and proxies `/api` and `/health` to `:3000`, so the browser sees one origin.

**Production runs only the server.** `trunk build --release` produces `build/dist/` once, and the axum server serves both the client (`STATIC_DIR`, default `build/dist`, via `tower-http` `ServeDir` as the fallback route) and `/api/*`. To check this locally, run `trunk build --release`, then `cargo run -p example-server`, and open `http://localhost:3000`.

**Dropbox:** this repo lives in Dropbox, which locks freshly built files and makes trunk fail intermittently (`os error 32/5`). So trunk outputs to `<repo>/build/dist` (gitignored), and `build/` is marked Dropbox-ignored. That's a per-machine flag; set it once after cloning (it persists because nothing deletes `build/` itself):
`New-Item -ItemType Directory -Force build | Out-Null; Set-Content -Path build -Stream com.dropbox.ignored -Value 1` (PowerShell, at the repo root). Cargo's target dir is `CARGO_TARGET_DIR`, outside Dropbox on this machine. If `os error 32` locks still occur with `build/` ignored, Windows Defender's real-time scanning of new `.js`/`.wasm` files is the likely cause (same fix as in `full-rust`): run `Add-MpPreference -ExclusionPath "<repo>uild"` in an admin PowerShell. To recover from a failed build, stop trunk, delete `build/dist`, and rerun.

**Discord app setup** (once per app, in the Developer Portal): enable Activities; set the Activity URL Mapping Root `/` → the tunnel or production hostname (no `https://`); and under **OAuth2 → Redirects** add the placeholder `https://127.0.0.1`. Discord requires a registered redirect URI even though the Embedded App SDK never uses it. Without one, `authorize` fails with `invalid_request: Missing "redirect_uri"`. Copy the Client ID and Secret into `.env`.

**Discord activity dev loop** (Discord app "webapp example", Root Mapping `/` → `webapp-dev.fadetovoid.win`):
1. Run `cargo run -p example-server` and `trunk serve` (as above).
2. Run `cloudflared tunnel run --url http://localhost:8080 webapp-dev`. It's a named tunnel, so the hostname is fixed and the URL mapping never changes. It stays in the foreground; Ctrl+C stops it.
3. Launch the activity in Discord (it's visible to the app's owner and testers).
4. Optional: to open dev tools in Discord desktop (Ctrl+Shift+I), add `"DANGEROUS_ENABLE_DEVTOOLS_ONLY_ENABLE_IF_YOU_KNOW_WHAT_YOURE_DOING": true` to `%APPDATA%\Discord\settings.json`.

In production, the URL mapping points at the deployed server (change 6).

Sanity check after a build: `GET /health` returns `ok`; `GET /api/auth/config` returns JSON; `GET /api/auth/me` and `GET /api/hello` return 401 without a session.

## Implementation Guide

`docs/implementation-guide.md` is the document **applications** read: how to add the framework, wire the server and client, the Redis key ownership rules, and the conventions apps must follow. Keep it in sync whenever the public API, conventions, or framework record prefixes change.

## Deployment

**Never deploy unless the user explicitly asks for a deploy in that moment.** Do not run `fly deploy`, `fly scale`, `fly secrets` or any other command that changes the Fly app as part of a change, a plan, or a fix, even when production would otherwise be out of date. Say that a deploy is needed, and let the user decide. Approval for one deploy doesn't carry over to later ones.

The example app is deployed to Fly.io as `webapp-example` (`https://webapp-example.fly.dev`), using the repo-root `Dockerfile` (trunk + server, multi-stage), `.dockerignore` and `fly.toml`. Deploy with `fly deploy --ha=false` from the repo root. The app runs one `shared-cpu-1x` / 256 MB machine (the smallest size) that auto-stops when idle. The deploy builds on Fly's remote builder, so Docker isn't needed locally. Secrets come from `.env` via `fly secrets import`; only non-secret settings live in `fly.toml`. Production shares the dev Upstash DB (no key prefix). Full steps, Discord portal setup, env var reference and how to adapt this for a new app: `docs/deploy.md`.

## Framework API Conventions (`webapp-server`)

- Apps hold a `WebApp` (config + Redis client, cheap to clone) in their axum state and `merge(webapp_server::auth_router())`. Both the router and the `Auth` extractor work for any state `S` where `WebApp: FromRef<S>`.
- `Auth(account)` on a handler means a session is required. The bearer header wins over the cookie, and an invalid bearer is a 401 with no cookie fallback.
- **Session TTLs (sliding; the `GETEX` read also refreshes, in one command):** website (cookie) sessions last 90 days; Discord sessions last 24 hours. Discord tokens are prefixed `dc_`, which selects the TTL without an extra lookup. Every activity launch signs in afresh and its in-memory token dies with the activity, so the short TTL clears abandoned sessions.
- **CSRF rule (enforced in `Auth`):** cookie-authenticated requests with a non-GET/HEAD/OPTIONS method must send `Content-Type: application/json`, otherwise 403. Clients must send JSON (even `{}`) on every write.
- **Redis record prefixes (2 characters):** `F*` is reserved for the framework (all of them live in `keys.rs`, listed in `FRAMEWORK_PREFIXES`; a unit test enforces 2 chars, `F`-first, unique). `A*`–`E*` are reserved for applications. Other first letters are unassigned (e.g. for reusable crates). Any new framework record type must use an unused `F?` code and be added to `FRAMEWORK_PREFIXES` and to the table in `docs/implementation-guide.md`. `Redis` helpers apply the `REDIS_KEY_PREFIX` namespace automatically; raw `command()` calls must build keys with `redis.key(..)`.
- `ApiError::Internal` logs the error and returns a generic 500. Never put error details in responses.

## Auth API Contract

| Endpoint | Success | Errors |
|---|---|---|
| `GET /api/auth/config` | 200 `AuthConfig` | — |
| `GET /api/auth/me` | 200 `Account` | 401 |
| `POST /api/auth/logout` | 204, clears session cookie | 403 (CSRF) |
| `POST /api/auth/discord {code}` | 200 `{token, discord_access_token, account}` (bearer token, no cookie) | 400 empty code, 401 Discord rejected the code (`invalid_grant`), 404 Discord not configured, 500 other Discord failures incl. bad credentials (logged) |
| `POST /api/auth/email/start {email}` | 204, sets `pending_login` cookie | 400 bad email, 429 rate-limited (2 min per IP), 404 email login disabled |
| `POST /api/auth/email/confirm {code, auto}` | 200 `Account`, sets session cookie, clears `pending_login` | 410 code expired/used; 409 `auto` refused (not this browser, so the code is **not** consumed: show a "Finish signing in" button and retry with `auto: false`) |

The emailed link is `{PUBLIC_BASE_URL}/?login_code=<code>`. The SPA reads the code and calls `confirm`. Email delivery is set by env: Resend if `RESEND_API_KEY` is set, else console output if `DEV_MODE=1`, else disabled. The example server serves with connect info so the rate limit sees the real peer IP locally (`Fly-Client-IP` on Fly).

## Framework API Conventions (`webapp-client`)

- Wrap the signed-in part of the app in `<AuthGate>`. It detects the mode (`frame_id` query param → Discord), runs the matching sign-in flow, and renders its children only once signed in.
- Inside it, `use_auth()` returns `AuthContext {account: Signal<Account>, logout: Callback<()>}`.
- Call the app's own endpoints with `api_get::<T>(path)` / `api_post::<B, T>(path, &body)`. They attach the Discord-mode bearer token, always send JSON (satisfying the CSRF rule), and map an empty/204 body to `null`, so `T = ()` works. Errors are `FetchError::Status(u16)` or `Network(String)`.
- `authenticate_discord_sdk()` is for apps that need Discord SDK commands beyond sign-in.
- The sign-in views are unstyled semantic HTML with `webapp-*` classes (`webapp-login`, `webapp-error`, `webapp-status`); apps style them.

## Discord Activity: Known Quirks

The Discord activity proxy (`{clientId}.discordsays.com`) rewrites URLs in served files and enforces a strict CSP. These choices exist because of that:

- **`no_sri = true`** in `Trunk.toml`: the proxy rewrites URLs inside JS, which breaks trunk's SRI hashes.
- **`no_autoreload = true`** in `Trunk.toml`: trunk's reload WebSocket is blocked by the CSP.
- **Vendored Discord SDK:** CSP blocks external URLs that aren't configured URL mappings, so the SDK is never loaded from a CDN. `crates/webapp-client/js/discord.js` is `bridge.src.js` + `@discord/embedded-app-sdk@2.5.0`, bundled into one committed file (regeneration steps in `js/README.md`). It's loaded as a **wasm-bindgen snippet** (`#[wasm_bindgen(module = "/js/discord.js")]`): copied into `dist/snippets/…` and imported with a relative path, so apps get it through the crate dependency with no `index.html` changes.
- **Bearer token, not cookies, in activity mode:** cookies through the proxy require `SameSite=None; Partitioned` and are unreliable. The activity re-authorizes on each launch (`prompt: "none"`) and keeps its token in memory.
- **Mode detection:** a `frame_id` query param (added by Discord at launch) means activity mode.
- **Server-side verification:** never trust identity data from the Discord client; the server does the code exchange and the `users/@me` lookup itself.
