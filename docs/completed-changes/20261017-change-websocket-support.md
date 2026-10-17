# Change: WebSocket Support

**Status:** done

Source: `C:\Data\Dropbox\dev\rust\ds\docs\completed-research\20261017-initial-planing.md` (Deep Space initial planning: section 2 of the Findings, and the Follow-Up on WebSocket vs. long-poll). Prior art: `full-rust` (`server/src/main.rs` axum `ws` handler, `client/src/main.rs` `web_sys::WebSocket` client).

Add authenticated WebSocket support to the framework so apps (Deep Space first) get realtime push in both website and Discord activity mode.

- **Opt-in via a Cargo feature, `ws`, off by default** on both `webapp-server` and `webapp-client`. Apps that don't enable it build exactly as before: no extra dependencies, no extra endpoints. Everything below sits behind `#[cfg(feature = "ws")]`.
  - `webapp-server/ws` enables axum's `ws` feature (which pulls in `tokio-tungstenite`, `tungstenite`, `sha1`, `base64`) and compiles `WsAuth` and `ws_router()`.
  - `webapp-client/ws` compiles the connect helper and enables the extra `web-sys` features (`WebSocket`, `MessageEvent`, `CloseEvent`, …).
  - Apps opt in with `features = ["ws"]` on both dependencies.
- **Ticket-based auth:** a browser `WebSocket` can't send an `Authorization` header, and a cookie-authenticated upgrade is open to cross-site WebSocket hijacking. So add `POST /api/auth/ws-ticket` in a **separate `ws_router()`** that apps merge explicitly (not in `auth_router()`, so the route is visible in the app's own code). It uses normal `Auth`, so the CSRF rule applies. It returns a random single-use ticket with a ~30 s TTL, stored under a new `F?` prefix (add it to `FRAMEWORK_PREFIXES` and the implementation guide table).
- **Ticket rate limit:** cap how often one session can request tickets (e.g. a few per minute, via a `SET NX EX` gate keyed by account or session, like the email rate limit), so a signed-in user can't spam Redis writes. Return 429 when over the cap. Reconnect backoff in the client helper must stay within the cap.
- **`WsAuth` extractor (server):** reads `?ticket=…`, consumes it with `GETDEL` and loads the `Account` **before** the upgrade (401 otherwise). Enable axum's `ws` feature.
- **Client helper (`webapp-client`):** builds the same-origin `ws(s)://{location.host}/…` URL (works through the Discord proxy), fetches a ticket via `api_post`, connects, exposes incoming text messages and a `send`, reconnects with backoff (a new ticket each time), and keeps the connection alive (ping) under Fly's ~60 s idle timeout.
- **Example app:** enables `ws` and adds a small demo (e.g. echo or a server clock).
- **Docs:** update `docs/implementation-guide.md` and `CLAUDE.md` (the `ws` feature, `ws_router()`, API contract, conventions).
- **Tests:** ticket single use and expiry, ticket rate limit, `WsAuth` rejection paths. Confirm the workspace builds and tests pass both with and without the `ws` feature (e.g. `cargo check -p webapp-server --no-default-features` and with `--features ws`, and the same for the client on `wasm32-unknown-unknown`).

## Clarifications

- **Keepalive:** browsers can't send ping frames, and the framework doesn't own the app's server-side socket loop. How should the connection stay under Fly's idle timeout?
  **Decision:** the client helper sends an **empty text message every 30 s**. App socket handlers must ignore empty text messages (documented). No server code or new dependency.
- **Rate limit mechanism:** a single `SET NX EX` gate allows only one ticket per window, which would make a quick reload or the first reconnect fail.
  **Decision:** a per-account counter: one atomic `EVAL` (`INCR`, plus `EXPIRE` on the first hit) under `FT<account_id>`, **20 tickets per 60 s**. Still one Redis command per check.
- **Ticket contents:** **Decision:** the ticket record (`FW<ticket>`, 30 s TTL) stores the `Account` JSON, so `WsAuth` costs a single `GETDEL`. A socket is authenticated once at connect. Signing out doesn't close an already-open socket (documented).
- **Client helper shape:** **Decision:** `connect_ws(path, on_message: impl Fn(String))` returns a `Copy` `WsConnection { status: Signal<WsStatus>, send(..) }`. Messages arrive through a callback rather than a signal, because a "last message" signal can drop messages that arrive in the same tick. `send` returns `false` when the socket isn't open (no queueing). The connection closes and stops reconnecting when the calling component's owner is cleaned up (e.g. on sign-out, when `<AuthGate>` unmounts its children). Backoff starts at 1 s and doubles to 30 s (plus up to 25% jitter), resets when a socket that stayed open for at least 10 s closes (so a server that keeps dropping connections isn't hammered), and jumps to 30 s on a 429. A 401 from the ticket endpoint stops reconnecting (`WsStatus::Closed`).
- **Shared types:** `WsTicketResponse {ticket}` goes in `webapp-common` behind its own `ws` feature, which the server and client `ws` features enable.
- **Example demo:** **Decision:** echo. On connect the server greets with the account's display name (which shows that auth worked), then echoes each non-empty text message.

## Implementation Plan

**Files to modify:**
- `crates/webapp-common/Cargo.toml`, `src/lib.rs`: add the `ws` feature and `WsTicketResponse`.
- `crates/webapp-server/Cargo.toml`: add the `ws = ["axum/ws", "webapp-common/ws"]` feature.
- `crates/webapp-server/src/keys.rs`: `WS_TICKET` (`FW`) + `WS_TICKET_TTL_SECS` (30), `WS_TICKET_RATE` (`FT`) + window/limit constants. These are ungated, so the prefixes stay reserved either way. Add them to `FRAMEWORK_PREFIXES`.
- `crates/webapp-server/src/lib.rs`: `#[cfg(feature = "ws")]` `mod ws`, `pub use ws::WsAuth`, and `pub fn ws_router()` (`POST /api/auth/ws-ticket`). Also `pub use webapp_common`: the implementation guide already says the server re-exports it, but it didn't, and WS handlers need `Account`.
- `crates/webapp-server/src/live_tests.rs`: live tests for ticket single use, expiry/TTL, the rate limit, and `WsAuth` with unknown and consumed tickets (feature-gated).
- `crates/webapp-client/Cargo.toml`: add the `ws` feature (`webapp-common/ws`, and web-sys `WebSocket`, `MessageEvent`, `CloseEvent`, `Event` as optional features via `web-sys/…`).
- `crates/webapp-client/src/lib.rs`: `#[cfg(feature = "ws")]` `mod ws` and re-exports.
- `example/server/Cargo.toml`, `src/main.rs`: enable `ws`, merge `ws_router()`, and add the `GET /api/ws` echo handler.
- `example/client/Cargo.toml`, `src/main.rs`, `style.css`: enable `ws` and add an echo demo component.
- `example/client/Trunk.toml`: a `ws = true` proxy entry for `/api/ws` (trunk's plain `/api` proxy doesn't upgrade).
- `docs/implementation-guide.md`: a WebSocket section (feature flags, `ws_router`, `WsAuth`, `connect_ws`, the empty-message rule, the trunk proxy entry) and the new prefixes in the keys table.
- `CLAUDE.md`: the API contract row, conventions for both crates, and the build/test commands with and without `ws`.

**Files to add:**
- `crates/webapp-server/src/ws.rs`: the ticket handler (`Auth` → rate check → `SET EX` ticket), the `WsAuth` extractor (`?ticket=` parsed by hand, length check before Redis, `GETDEL` → `Account`, 401 otherwise), and unit tests for query parsing and pre-Redis rejections.
- `crates/webapp-client/src/ws.rs`: `connect_ws`, `WsConnection`, `WsStatus`; callback-driven reconnect state, the 30 s ping interval, and cleanup on owner disposal.

**Approach:**
`WsAuth` is a `FromRequestParts` extractor. Apps put it before `WebSocketUpgrade` in the handler arguments, so a request without a valid ticket is rejected before any upgrade. The client keeps one `Rc` of shared state (socket + its JS closures, backoff, stopped flag). Replacing or closing a socket detaches its handlers first, so a manual close never triggers a reconnect, and the closures are dropped instead of leaked.

**Risks / things to watch for:**
- Trunk proxy: overlapping `/api` (HTTP) and `/api/ws` (`ws = true`) entries must coexist in trunk's router. Verify; if they don't, move the example route outside `/api`.
- Leptos `on_cleanup` needs `Send + Sync` closures; hold the `Rc` state through a `Send`-safe wrapper (`StoredValue` local storage or `SendWrapper`) that is still valid at cleanup time.
- Feature unification: an app's `axum::extract::ws` only exists because `webapp-server/ws` turns on `axum/ws`. The example enables `axum`'s `ws` explicitly too, for clarity.
- Builds both with and without `ws` (server natively, client on wasm32).
