# Implementation Guide

For applications built on the **webapp** framework: a Rust/axum server and a Leptos/WASM client that run as a plain website (email magic-link login) or as a Discord activity (Discord login), with accounts and sessions in Upstash Redis.

This guide covers what an app needs to know. The framework's own design decisions are in `docs/completed-research/20261009-web-app-framework.md`; deployment is in [`deploy.md`](deploy.md). `example/` is a complete, working app to copy from.

## 1. Adding the framework to an app

An app has the same shape as `example/`: a server crate (native) and a client crate (wasm32, built with trunk). The server depends on `webapp-server`, and the client depends on `webapp-client`. Both re-export the shared types via `webapp-common` (the client as `webapp_client::webapp_common`).

Pick a dependency style:

| Style | `Cargo.toml` | When |
|---|---|---|
| Local path | `webapp-server = { path = "../webapp/crates/webapp-server" }` | Developing the app and the framework together; always the current working copy, no version pinning. |
| Local git, pinned | `webapp-server = { git = "file:///C:/Data/Dropbox/dev/rust/webapp", tag = "v0.1.0" }` | Local builds pinned to a tagged, committed version. |
| Git host, pinned | `webapp-server = { git = "https://github.com/blbergwall/webapp", tag = "v0.1.0" }` | Required for deployment: Fly's remote builder only receives the app's folder, so path and `file://` dependencies don't exist there. |

The framework is published at `https://github.com/blbergwall/webapp` (public, so builds fetch it without credentials). Tags are `vX.Y.Z` and never move once pushed. To pick up a new release, change `tag` on both dependencies and run `cargo update -p webapp-server -p webapp-client`.

To deploy from a git host but develop against your local copy, keep the git-host dependency and add a **git-ignored** `.cargo/config.toml` to the app:

```toml
[patch."https://github.com/blbergwall/webapp"]
webapp-server = { path = "../webapp/crates/webapp-server" }
webapp-client = { path = "../webapp/crates/webapp-client" }
```

With the patch:
- **Add `.cargo` to the app's `.dockerignore`** (the template already lists it). `fly deploy` uploads the working directory, including git-ignored files, and a patch path outside the build context fails the build.
- `Cargo.lock` records the patched crates as path packages, with no git commit. The `tag` in `Cargo.toml` is the pin. The Docker build, which has no patch, re-resolves them to the tag. So don't add `--locked` to the Dockerfile's `cargo build`, or that re-resolve fails.

Use the same `axum` (0.8) and `leptos` (0.8, `csr`) major versions as the framework.

## 2. Server

```rust
use axum::{Json, Router, routing::get};
use webapp_server::{ApiError, Auth, WebApp};

let webapp = WebApp::from_env()?; // reads the env vars in deploy.md
let app = Router::new()
    .route("/api/things", get(list_things))
    .merge(webapp_server::auth_router()) // /api/auth/*
    .fallback_service(tower_http::services::ServeDir::new(static_dir))
    .with_state(webapp);
// Serve with connect info so the email rate limit sees real client IPs locally:
axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await?;

// `Auth` on a handler = sign-in required (401 otherwise).
async fn list_things(Auth(account): Auth) -> Result<Json<Vec<Thing>>, ApiError> { ... }
```

- **App state:** an app with its own state type keeps a `WebApp` inside it and implements `FromRef<AppState> for WebApp`. `auth_router()` and `Auth` then work unchanged.
- **Errors:** return `ApiError`. Any `anyhow::Error` converts with `?` into a logged, generic 500, so details never reach the client. The other variants map to 400/401/403/404/409/410/429.
- **CSRF rule:** a cookie-authenticated request with a method other than GET/HEAD/OPTIONS must send `Content-Type: application/json`, or `Auth` rejects it with 403. Design write endpoints to take JSON. The client helpers always send it.
- **Sessions:** website sessions are a 90-day sliding cookie. Discord sessions are a 24-hour sliding bearer token. `Auth` accepts either.
- **Identity:** an `Account` (`id`, `display_name`, `email?`, `discord_id?`) is keyed by its internal `id`. Use `account.id` as the key for the app's own data, never the email or Discord ID. A website user and a Discord user are separate accounts unless linked (linking is planned, not built).

## 3. Client

```rust
use leptos::prelude::*;
use webapp_client::{AuthGate, api_get, api_post, use_auth};

view! { <AuthGate><Home /></AuthGate> }

#[component]
fn Home() -> impl IntoView {
    let auth = use_auth(); // AuthContext { account: Signal<Account>, logout: Callback<()> }
    let things = LocalResource::new(|| api_get::<Vec<Thing>>("/api/things"));
    view! { /* ... */ <button on:click=move |_| auth.logout.run(())>"Sign out"</button> }
}
```

- `<AuthGate>` detects the mode (a `frame_id` query param means Discord), runs the sign-in flow, and renders its children only when signed in.
- Call the app's API with `api_get` / `api_post` only. They attach the Discord bearer token and send JSON. An empty/204 response parses as `()`. Errors are `FetchError::Status(u16)` or `FetchError::Network(String)`.
- The sign-in views are unstyled HTML with `webapp-login`, `webapp-error` and `webapp-status` classes for the app's CSS to style.
- `Trunk.toml` must keep `no_sri = true` and `no_autoreload = true` (Discord's proxy and CSP require them). It should output to `build/dist` and proxy `/api` to the local server. Copy `example/client/Trunk.toml`.
- The Discord SDK ships inside `webapp-client` (no `index.html` changes, no CDN). Apps that need SDK commands beyond sign-in call `authenticate_discord_sdk()` after sign-in.

## 4. WebSockets (optional `ws` feature)

Realtime push in both modes, behind a Cargo feature that is off by default. Without it, nothing below is compiled and there is no extra endpoint or dependency. Enable it on both crates (and on the app's `axum`, for `axum::extract::ws`):

```toml
# server
webapp-server = { ..., features = ["ws"] }
axum = { version = "0.8", features = ["ws"] }
# client
webapp-client = { ..., features = ["ws"] }
```

**How auth works:** a browser `WebSocket` can't send an `Authorization` header, and a cookie-authenticated upgrade is open to cross-site WebSocket hijacking. So the client first calls `POST /api/auth/ws-ticket` (normal `Auth`, either mode, CSRF rule included). That returns a random single-use ticket, valid for 30 s, which the client passes as `?ticket=` on the upgrade. Tickets are limited to **20 per account per minute** (429 beyond that).

**Server:**

```rust
use axum::{extract::ws::{Message, WebSocket, WebSocketUpgrade}, response::Response};
use webapp_server::{WsAuth, webapp_common::Account};

let app = Router::new()
    .route("/api/ws", get(ws))
    .merge(webapp_server::auth_router())
    .merge(webapp_server::ws_router()) // POST /api/auth/ws-ticket
    /* ... */;

// `WsAuth` goes BEFORE `WebSocketUpgrade`: no valid ticket = 401, no upgrade.
async fn ws(WsAuth(account): WsAuth, upgrade: WebSocketUpgrade) -> Response {
    upgrade.on_upgrade(move |socket| handle(socket, account))
}
```

- **Ignore empty text messages.** They are the client helper's keepalive (sent every 30 s, because Fly.io closes connections that are idle for about 60 s). Don't use `""` as a real message.
- A socket is authenticated **once, at connect**. Signing out doesn't close sockets that are already open. If that matters, close them in the app.
- Use the account from `WsAuth`. Don't trust identity data sent over the socket.

**Client:**

```rust
use webapp_client::{WsStatus, connect_ws};

// Inside <AuthGate> (a session is needed to get a ticket):
let ws = connect_ws("/api/ws", move |text: String| { /* handle a message */ });
ws.send("hello");       // false (and dropped) unless the socket is open
ws.status.get();        // WsStatus::Connecting | Open | Closed
```

- The URL is same-origin (`ws(s)://{location.host}{path}`), so it works through the Discord proxy unchanged.
- It reconnects automatically with a new ticket each time. The backoff runs from 1 s up to 30 s with jitter, resets once a connection has stayed open for 10 s, and waits 30 s after a 429. Messages aren't queued while disconnected, so after reconnecting the server should send whatever state the client needs.
- A 401 on the ticket request (session gone) stops it (`WsStatus::Closed`). It also stops for good when the calling component is cleaned up, e.g. on sign-out.
- `WsConnection` is `Copy`. Messages arrive through the callback (not a signal), so none are lost when several arrive at once.
- **Local dev:** trunk's plain `/api` proxy doesn't forward WebSocket upgrades. Add a `ws = true` entry per WebSocket route to `Trunk.toml` (see `example/client/Trunk.toml`):

  ```toml
  [[proxy]]
  backend = "ws://127.0.0.1:3000/api/ws"
  ws = true
  ```

## 5. Database keys

Every record in Redis starts with a **2-character type prefix**, e.g. `FA<account_id>`. Prefixes are owned by first letter, so the framework and apps can add record types independently without colliding:

| First letter | Owner |
|---|---|
| `F` | **The framework.** Apps must never create keys starting with `F`. |
| `A`, `B`, `C`, `D`, `E` | **Applications.** Pick any unused code in this range, e.g. `AW` for worlds or `B1`. |
| All others | Unassigned; kept free for other uses, such as reusable crates. Don't use them in an app. |

Codes the framework uses today (from `crates/webapp-server/src/keys.rs`; new framework codes are always `F?`, so apps don't need to track this list):

| Prefix | Record |
|---|---|
| `FA<account_id>` | Account (JSON `Account`) |
| `FE<email>` | Email identity → `account_id` |
| `FD<discord_id>` | Discord identity → `account_id` |
| `FS<token>` | Session → `account_id` (sliding TTL) |
| `FL<code>` | One-time email login code |
| `FR<ip>` | Email rate-limit gate |
| `FW<ticket>` | Single-use WebSocket ticket → `Account` (30 s TTL; `ws` feature) |
| `FT<account_id>` | WebSocket ticket rate-limit counter (`ws` feature) |

Each app should keep its own prefixes in one documented module (like the framework's `keys.rs`), as the single place to check before choosing a new one.

**Accessing Redis from an app:** use `webapp.redis()` (`Redis`). Its helpers (`get`, `getex`, `getdel`, `set`, `set_ex`, `set_nx_ex`, `del`) take **unprefixed** keys and add the optional `REDIS_KEY_PREFIX` namespace themselves. For any other command, build each key with `redis.key("AW123")` and pass it to `redis.command(&[...])`. The namespace prefix sits in front of the whole key (e.g. `myapp:AW123`) and is separate from the 2-character record prefix.

## 6. Configuration and deployment

All configuration comes from environment variables: `.env` locally (copy `.env.example`), and Fly secrets plus `fly.toml` `[env]` in production. See [`deploy.md`](deploy.md) for the variable reference, the Discord Developer Portal setup (including the required placeholder redirect URI), and how to adapt the `Dockerfile` / `fly.toml` for a new app.
