# Change 5: Client Library (WASM) and Example App

Order: 5 of 6. Depends on changes 2–4. Architecture source: `docs/completed-research/20261009-web-app-framework.md`.

Implement `webapp-client` (Leptos 0.8, `csr`) and the end-to-end example app:

- **Vendored Discord SDK:** a pinned, pre-bundled ESM build of `@discord/embedded-app-sdk` (2.5.0), plus a small bridge script exposing `ready`, `authorize`, and `authenticate`. Trunk inlines both into `index.html`; no CDN loads.
- **Mode detection:** a `frame_id` query param means Discord activity mode; otherwise website mode.
- **Discord mode:**
  - fetch `/api/auth/config`
  - run SDK `ready` and `authorize({scope:["identify"], prompt:"none"})`
  - `POST /api/auth/discord`
  - keep the token in memory
- **Website mode:**
  - call `GET /api/auth/me`
  - on 401, show the email form
  - handle `?login_code=`: try `auto` confirm first, and if that is refused, show a "Finish signing in" button
- **API helpers:** `api_get` / `api_post` built directly on `web-sys` fetch, attaching the bearer token when one is held.
- **`<AuthGate>` component:** shows login UI until signed in, then renders its children with the account in context. Logout is included.
- **Example app:**
  - `example/server` merges `auth_router()`, adds one protected route using the `Auth` extractor, and serves `dist/` with `tower-http` `ServeDir`.
  - `example/client` displays "Hello, <display_name>".
  - `Trunk.toml` sets `no_sri = true`, `no_autoreload = true`, and proxies `/api` to the server.
- **Verification:** test website mode locally (dev-mode console link) and Discord mode through a Cloudflare tunnel.
