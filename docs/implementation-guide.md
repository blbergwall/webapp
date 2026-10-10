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
| Git host, pinned | `webapp-server = { git = "https://github.com/<owner>/webapp", tag = "v0.1.0" }` | Required for deployment: Fly's remote builder only receives the app's folder, so path and `file://` dependencies don't exist there. |

To deploy from a git host but develop against your local copy, keep the git-host dependency and add a **git-ignored** `.cargo/config.toml` to the app:

```toml
[patch."https://github.com/<owner>/webapp"]
webapp-server = { path = "../webapp/crates/webapp-server" }
webapp-client = { path = "../webapp/crates/webapp-client" }
```

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

## 4. Database keys

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

Each app should keep its own prefixes in one documented module (like the framework's `keys.rs`), as the single place to check before choosing a new one.

**Accessing Redis from an app:** use `webapp.redis()` (`Redis`). Its helpers (`get`, `getex`, `getdel`, `set`, `set_ex`, `set_nx_ex`, `del`) take **unprefixed** keys and add the optional `REDIS_KEY_PREFIX` namespace themselves. For any other command, build each key with `redis.key("AW123")` and pass it to `redis.command(&[...])`. The namespace prefix sits in front of the whole key (e.g. `myapp:AW123`) and is separate from the 2-character record prefix.

## 5. Configuration and deployment

All configuration comes from environment variables: `.env` locally (copy `.env.example`), and Fly secrets plus `fly.toml` `[env]` in production. See [`deploy.md`](deploy.md) for the variable reference, the Discord Developer Portal setup (including the required placeholder redirect URI), and how to adapt the `Dockerfile` / `fly.toml` for a new app.
