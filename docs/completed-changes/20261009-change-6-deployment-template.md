# Change 6: Deployment Template (Fly.io)

**Status:** done

Order: 6 of 6. Depends on change 5. Architecture source: `docs/completed-research/20261009-web-app-framework.md`.

Make the example app deployable, and document the pattern for apps built on the framework:

- **Multi-stage Dockerfile:**
  - builds the client with trunk (`wasm32-unknown-unknown` target)
  - builds the server in release mode
  - produces a slim runtime image containing the server binary and `dist/`
- **`fly.toml`:** follows the `neverending-contrivance` / `tfa` conventions. Secrets are set with `fly secrets set` and never baked into the image.
- **`docs/deploy.md` covers:**
  - Fly deploy steps
  - custom-domain website setup
  - the Discord Developer Portal setup: an Activity with the URL mapping `/` pointing at the Fly host, and the OAuth2 client ID and secret
  - the env var reference

## Clarifications

The change is one deliverable, so it isn't split.

- **Should this change actually deploy the example app, or just add files and docs?**
  **Decision:** Deploy it as Fly app `webapp-example`, region `iad`, with auto-stop so it costs almost nothing while idle, and verify at `https://webapp-example.fly.dev`. Fly's remote builder is used, since Docker isn't installed locally.
- **Which Upstash data should production use?**
  **Decision:** The same dev database with no key prefix, so prod and local dev share accounts and sessions.
- **How is Discord activity mode tested against production, given one Root Mapping per app?**
  **Decision:** Temporarily repoint the "webapp example" Root Mapping to `webapp-example.fly.dev`, test, then point it back to `webapp-dev.fadetovoid.win`. `docs/deploy.md` documents the general pattern: the production mapping in real apps, plus a separate dev Discord app or temporary repointing.
- **Where do the deploy files live?** (decided in planning)
  **Decision:** At the repo root (`Dockerfile`, `.dockerignore`, `fly.toml`), because the build needs the whole workspace. Apps built on the framework copy them, changing the package and app names.
- **Configuration split:** non-secret settings go in `fly.toml` `[env]`: `APP_NAME`, `PUBLIC_BASE_URL=https://webapp-example.fly.dev`. Secrets are imported from `.env` with `fly secrets import`, so values are never printed: the `UPSTASH_*`, `RESEND_*` and `DISCORD_*` variables. `BIND_ADDR` and `STATIC_DIR` are fixed in the image. `DEV_MODE` is never set in production.
- **Email sender:** keep neverending's verified Resend domain for the example (from the change-3 decision). Real apps use their own domain.

## Implementation Plan

**Files to add:**
- `Dockerfile`: multi-stage.
  - **Builder** (`rust:1-slim-bookworm`, pinned to bookworm to match the runtime's glibc):
    - add the `wasm32-unknown-unknown` target
    - install trunk 0.21.14 from its GitHub release, **SHA-256-verified** (`f2b4680c…036b`, the published checksum matched an independent download)
    - `trunk build --release` in `example/client`; trunk fetches the `wasm-bindgen` CLI version matching `Cargo.lock`
    - `cargo build --release -p example-server`
  - **Runtime** (`debian:bookworm-slim` + `ca-certificates`): the server binary plus `build/dist` → `/app/dist`, with `ENV STATIC_DIR=/app/dist BIND_ADDR=0.0.0.0:8080`, `EXPOSE 8080`.
- `.dockerignore`: `.git`, `build`, `target`, `.env`, `.env.*` (but not `.env.example`), `docs`.
- `fly.toml`: `app = "webapp-example"`, `primary_region = "iad"`, `[env]` as above, `[http_service]` (`internal_port = 8080`, `force_https`, `auto_stop_machines = "stop"`, `auto_start_machines`, `min_machines_running = 0`), and a `/health` check; `shared-cpu-1x` VM.
- `docs/deploy.md`:
  - first-time Fly setup (`fly apps create`, `fly secrets import`) and redeploys (`fly deploy`)
  - custom domain (`fly certs add` + DNS)
  - full Discord app setup: Activities, Root Mapping, OAuth2 redirect placeholder, client ID/secret
  - the dev-vs-prod mapping pattern
  - env var reference: required/optional, secret or not, used for what
  - checklist for adapting the files to a new app

**Files to modify:**
- `CLAUDE.md`: a Deployment section pointing to `docs/deploy.md`.

**Changes made during implementation:**
- The first `fly deploy` created the machines and passed health checks, but allocated no public IPs, so `webapp-example.fly.dev` didn't resolve. Fixed with `fly ips allocate-v4 --shared` + `fly ips allocate-v6`; `docs/deploy.md` now says to check `fly ips list` after the first deploy.

**Approach:**
1. Write the files.
2. `fly apps create webapp-example`; `fly secrets import --stage` from the filtered `.env` lines.
3. `fly deploy` (remote builder).
4. Verify `https://webapp-example.fly.dev`: `/health`; `/api/auth/config`; website email login with the real Resend link pointing at fly.dev; the session cookie over HTTPS; `/api/hello`; logout. Rate limiting is keyed by `Fly-Client-IP`.
5. User repoints the Discord mapping to `webapp-example.fly.dev`, tests the activity, then repoints it back.

**Risks / things to watch for:**
- **Remote build time:** the first build compiles leptos and the server from scratch. There's no layer-caching optimization, to keep the Dockerfile simple; it can be added later if deploys get slow.
- **`COPY . .`:** relies on `.dockerignore` to keep `.env` out of the build context. It must never end up in an image.
- **Shared prod/dev DB:** a local `cargo test -- --ignored` run uses random prefixes and cleans up, so it can't disturb prod data.
- **Fly `[env]` is visible in the repo:** only non-secret values go there.

## User Feedback

- **The user asked for one machine only, on the smallest size, since this is a simple example app.**
  **Decision:** Scaled to 1 with `fly scale count 1` (the extra machine was destroyed). It was already the smallest size Fly offers (`shared-cpu-1x`, 256 MB), now pinned as `memory = "256mb"` in `fly.toml`. The deploy command is now `fly deploy --ha=false` (in `docs/deploy.md` and `CLAUDE.md`). A redeploy confirmed it stays at 1 machine.
- Production tests passed: website email sign-in at `https://webapp-example.fly.dev`, and the Discord activity with the Root Mapping temporarily on `webapp-example.fly.dev`.
- **The database had two `SS` records with 3-month TTLs for the Discord user. Should Discord sessions live that long, and will more keep being created?**
  **Decision:** Yes, one is created per activity launch: the token lives only in activity memory, so every launch signs in afresh and the old session is abandoned. Discord sessions now have a **24-hour sliding TTL** (website sessions keep 90 days). Discord tokens carry a `dc_` prefix, so the TTL is chosen from the token without an extra lookup. The user was concerned that refreshing on every API call adds writes. It doesn't: the lookup is already a single `GETEX` that reads and refreshes in one command (Upstash bills per command), so the user chose sliding over fixed. Covered by unit and live tests, documented in `CLAUDE.md`, and redeployed.
- The 3 pre-existing 90-day Discord sessions are left to expire on their own (user decision).
