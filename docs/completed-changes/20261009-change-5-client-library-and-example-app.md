# Change 5: Client Library (WASM) and Example App

**Status:** done

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

## Clarifications

The change is large but cohesive: one library crate plus the example that exercises it. Website and Discord mode share the same `AuthGate`, API helpers and token store, so it isn't split.

- **How is the Discord SDK delivered to apps that use `webapp-client` as a git dependency?** (refines the research)
  **Decision:** As a **wasm-bindgen JS snippet** (`#[wasm_bindgen(module = "/js/discord.js")]`) instead of a trunk inline `<link>` in each app's `index.html`. wasm-bindgen copies the file into `dist/snippets/…` and the generated glue imports it with a **relative** path. So any app gets it automatically through the dependency, with no `index.html` edits and no file paths into the crate.
  - `full-rust`'s "a separate JS file can't be used" note dates from the old `/.proxy/` path-prefix era. Current Discord docs map the root directly, and relative imports aren't affected anyway.
  - **Fallback:** if the snippet fails inside Discord, switch to trunk inlining.
  - `discord.js` is the SDK 2.5.0 plus a tiny bridge, bundled and minified once with esbuild into one committed file. It carries a header with the version and license (MIT), and `crates/webapp-client/js/README.md` gives the regeneration steps. Node/esbuild are only used when vendoring, not in the build.
- **Discord mode logout:** `logout()` deletes the session server-side and drops the in-memory token. The signed-out view then shows a "Sign in with Discord" button that re-runs `authorize` (`prompt: "none"`, so it's instant for a user who has already consented).
- **Discord errors** (the user cancels `authorize`, or the server returns 401/500): an error message plus a "Try again" button.
- **Website-mode views:**
  - signed out → email form → "Check your email" (with the address)
  - 429 → "Please wait a couple of minutes"
  - `?login_code=` → auto confirm; on 409 a "Finish signing in" button; on 410 "Link expired or already used", then the email form
  - The query string is removed with `history.replaceState` after handling.
- **UI styling:** the `AuthGate` views are plain semantic HTML with `webapp-` CSS classes and no built-in styles. The example ships a small stylesheet; apps restyle freely.
- **Client API:**
  - `use_auth()` returns `AuthContext {account: Signal<Account>, logout}`.
  - `api_get<T>(path)` / `api_post<B, T>(path, &body)` return `Result<T, FetchError>`. `FetchError` is `Status(u16)` or `Network(String)`. An empty/204 body deserializes as `null`, so `T = ()` works.
  - `api_post` always sends JSON, which keeps the server's CSRF rule satisfied.
- **Serving `dist/`:** the example server serves `STATIC_DIR` (default `example/client/dist`) with `tower-http` `ServeDir` as the fallback route, so `/api/*` and `/health` still take precedence. In dev, trunk serves the page anyway.
- **Dev docs:** `CLAUDE.md` gains the tunnel workflow from change 4's feedback (`webapp-dev.fadetovoid.win`) and the Discord desktop devtools setting from the `ftv` notes.

## Implementation Plan

**Files to modify:**
- `Cargo.toml`: add workspace deps `webapp-client` (path), `wasm-bindgen`, `wasm-bindgen-futures`, `js-sys`, `web-sys`, `tower-http` (`fs` only).
- `crates/webapp-client/Cargo.toml` + `src/lib.rs`: the library, built from the modules listed under **Files to add**.
- `example/client/Cargo.toml`, `src/main.rs`, `index.html`: wrap the app in `<AuthGate>`, call the protected `/api/hello` with `api_get`, show the greeting and a logout button; link the stylesheet.
- `example/server/Cargo.toml` + `src/main.rs`: `ServeDir` fallback for `STATIC_DIR`.
- `.env.example`: add `STATIC_DIR`.
- `CLAUDE.md`: client API, Discord dev workflow (tunnel, URL mapping, desktop devtools), and the SDK-snippet approach replacing the "vendored SDK inlined by trunk" wording.

**Files to add:**
- `crates/webapp-client/js/discord.js`: the bundled SDK + bridge (generated, committed).
- `crates/webapp-client/js/bridge.src.js`: bridge source. It exports `discordAuthorize(clientId)`, which runs `new DiscordSDK` + `ready()` + `authorize({client_id, response_type: "code", state: "", prompt: "none", scope: ["identify"]})` and returns the code, and `discordAuthenticate(accessToken)`.
- `crates/webapp-client/js/README.md`: how to regenerate `discord.js` (pinned SDK version, esbuild command).
- `crates/webapp-client/src/api.rs`: the fetch helpers and the thread-local bearer token.
- `crates/webapp-client/src/discord.rs`: wasm-bindgen externs for the bridge and an `is_discord_activity()` check for the `frame_id` param.
- `crates/webapp-client/src/gate.rs`: `AuthGate`, the views, `use_auth`, and `AuthContext`.
- `example/client/style.css`: minimal styles.

**Changes made during implementation:**
- **Trunk output moved from `example/client/dist` to `<repo>/build/dist`.** Trunk builds failed intermittently with `os error 32/5` while Dropbox indexed the freshly created files in `dist/`. A Dropbox-ignore flag on `dist/` itself doesn't last, because trunk deletes and recreates that folder. `build/` is never deleted, so its ignore flag persists (set once per machine; command in `CLAUDE.md`). `.gitignore` now has `build/` instead of `dist/`, and the `STATIC_DIR` default is `build/dist`. (User decision.)
- The stray `.gitignore` entry `target/` is harmless. This machine's `CARGO_TARGET_DIR` is `C:\Data\rust\target`, outside Dropbox, so change 1's warning about Dropbox syncing `target/` didn't apply.
- Non-ASCII characters were removed from the vendored JS header and comments.

**Approach:**
1. Vendor the SDK. Bundle it in the scratchpad with `npm install @discord/embedded-app-sdk@2.5.0 esbuild` and `esbuild bridge.src.js --bundle --format=esm --minify`, then copy only the output into the crate.
2. Build the client library and the example.
3. Verify website mode in the browser via trunk on `:8080` with real email (Resend), including logout.
4. Verify Discord mode by launching the activity through the `webapp-dev` tunnel.
5. Check that `trunk build --release` works and that the example server serves `dist/` on its own.

**Risks / things to watch for:**
- **Snippet loading in Discord:** if the proxy or CSP mishandles `snippets/…`, fall back to trunk inlining (the plan would be updated).
- **SDK bundle size:** expected to be around 100–200 KB minified. Acceptable.
- **Leptos 0.8 API details:** `ChildrenFn` for re-rendering children, `LocalResource` / `spawn_local` for async in CSR. Follow the compiler.
- **`Secure` cookies on `http://localhost:8080` through the trunk proxy:** browsers accept them on localhost, but this is the first real browser test.
- **Discord authorize with `prompt: "none"` for a first-time user:** Discord still shows its consent modal for activities. Expected.

## User Feedback

- Website mode: all five test steps passed (email form, same-browser auto sign-in, different-browser "Finish signing in", sign out, session survives reload).
- **Discord mode showed "OAuth2 Error: invalid_request: Missing \"redirect_uri\" in request."**
  **Decision:** This is Discord app configuration, not code. The SDK snippet loaded and `ready()`/`authorize` reached Discord. Discord requires at least one registered OAuth2 redirect URI even for activities (its activity guide uses the placeholder `https://127.0.0.1` under OAuth2 → Redirects). The user adds it in the portal. The step is now documented under "Discord app setup" in `CLAUDE.md`.
- After adding the redirect URI, Discord mode worked end to end: the activity signed in automatically, showed "Hello, Koth!" from the protected `/api/hello` (bearer token), and created account `7191e6d4…` for Discord ID `118817581967605763`. This is the first real Discord code exchange, so change 4 is now fully verified too. As designed, this account is separate from the email account `86acc7f5…`; linking is backlog work.
