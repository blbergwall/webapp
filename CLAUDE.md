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

# Production client build (from example/client), outputs example/client/dist/
trunk build --release
```

Prerequisites: `rustup target add wasm32-unknown-unknown` and `cargo install trunk`.

**Local dev runs two processes. Open `http://localhost:8080`:**
- `cargo run -p example-server` runs the backend (axum, native Rust) on `:3000`: `/health` and `/api/*`. Restart it after server code changes.
- `trunk serve` runs the frontend dev server on `:8080`. It compiles the client to WASM, serves the page, rebuilds when client code changes (refresh the browser yourself, since auto-reload is disabled for Discord), and proxies `/api` and `/health` to `:3000`, so the browser sees one origin.

**Production runs only the server.** `trunk build --release` produces `dist/` once, and the axum server serves both `dist/` and `/api/*`. Serving `dist/` is added in change 5.

**Discord** just loads a URL. In dev, point the Cloudflare tunnel at `:8080`. In production, point the URL mapping at the deployed server.

Sanity check after a build: `GET /health` returns `ok`.

## Discord Activity: Known Quirks

The Discord activity proxy (`{clientId}.discordsays.com`) rewrites URLs in served files and enforces a strict CSP. These choices exist because of that:

- **`no_sri = true`** in `Trunk.toml`: the proxy rewrites URLs inside JS, which breaks trunk's SRI hashes.
- **`no_autoreload = true`** in `Trunk.toml`: trunk's reload WebSocket is blocked by the CSP.
- **Vendored Discord SDK:** CSP blocks external URLs that aren't configured URL mappings, so the SDK is a pinned, pre-bundled ESM file inlined into `index.html` by trunk, never loaded from a CDN.
- **Bearer token, not cookies, in activity mode:** cookies through the proxy require `SameSite=None; Partitioned` and are unreliable. The activity re-authorizes on each launch (`prompt: "none"`) and keeps its token in memory.
- **Mode detection:** a `frame_id` query param (added by Discord at launch) means activity mode.
- **Server-side verification:** never trust identity data from the Discord client; the server does the code exchange and the `users/@me` lookup itself.
