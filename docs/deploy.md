# Deploying to Fly.io

How the example app (`webapp-example`) is deployed, and the pattern for apps built on the framework. The example was deployed this way and has since been removed (see **Removing an app**); the files and steps here still deploy it again. One Fly app runs one server that serves both the trunk-built client and `/api/*` from a single origin. The same origin works for the plain website and, through Discord's proxy, for the Discord activity.

## Files

| File | Purpose |
|---|---|
| `Dockerfile` | Multi-stage build. The builder installs a pinned, checksum-verified trunk, runs `trunk build --release` (outputs `build/dist`), then `cargo build --release -p example-server`. The runtime image (`debian:bookworm-slim`) holds only the server binary and the client files in `/app/dist`. |
| `.dockerignore` | Keeps `.env*` (secrets), `.git`, `build`, `target`, `docs` and `.cargo` (a local path patch) out of the build context. |
| `fly.toml` | App name, region, non-secret env vars (`[env]`), HTTP service with auto-stop, and a `/health` check. |

`BIND_ADDR=0.0.0.0:8080` and `STATIC_DIR=/app/dist` are set in the image. The email rate limit keys on Fly's `Fly-Client-IP` header automatically.

## First deploy

Prerequisites: `fly` CLI logged in (`fly auth whoami`). Docker isn't needed, because `fly deploy` builds on Fly's remote builder.

```sh
# 1. Create the app (name must match fly.toml)
fly apps create webapp-example

# 2. Import secrets from .env without printing them (bash). Staged secrets apply on the next deploy.
grep -E '^(UPSTASH_REDIS_REST_URL|UPSTASH_REDIS_REST_TOKEN|REDIS_KEY_PREFIX|RESEND_API_KEY|RESEND_FROM_EMAIL|DISCORD_CLIENT_ID|DISCORD_CLIENT_SECRET|GOOGLE_CLIENT_ID|GOOGLE_CLIENT_SECRET)=.+' .env \
  | fly secrets import --stage --app webapp-example

# 3. Build and deploy. --ha=false: one machine instead of Fly's default two
#    (fine for small apps; drop the flag when you want the redundancy).
fly deploy --ha=false
```

**Then check that the app has public IPs:** `fly ips list --app webapp-example`. On the example's first deploy the list was empty even though the deploy said "Provisioning ips", and the hostname didn't resolve (`Could not resolve host`). Allocating fixed it immediately; both IPs are free:

```sh
fly ips allocate-v4 --shared --app webapp-example
fly ips allocate-v6 --app webapp-example
```

Check afterwards:
- `https://webapp-example.fly.dev/health` returns `ok`.
- `/api/auth/config` returns the Discord client ID.
- Email sign-in works end to end: the emailed link points at `PUBLIC_BASE_URL`.

## Redeploys and secrets

- `fly deploy --ha=false`: rebuild and roll out (from the repo root). Redeploys keep the existing machine count; `--ha=false` only matters when no machine exists yet, and keeps it as a habit.
- `fly scale count 1 --app webapp-example`: go back to one machine if an app ended up with two. `fly scale show` shows the count and size. `shared-cpu-1x` / 256 MB is the smallest size, pinned in `fly.toml`.
- `fly secrets import --app webapp-example` (or `fly secrets set NAME=value`): change secrets; this restarts the machines.
- `fly logs --app webapp-example`: server output, including logged `ApiError::Internal` details.
- `fly secrets list --app webapp-example`: names and digests only, never values.

## Removing an app

```sh
fly apps destroy webapp-example
```

It asks for confirmation (`--yes` skips it) and **can't be undone**. It deletes everything Fly holds for the app: machines, volumes, IPs, certificates, secrets and images. A suspended app with stopped machines costs little, but destroying it frees it up entirely. Check afterwards with `fly apps list`.

Things outside Fly to tidy up:
- **App name:** Fly app names are global, so the freed name (and its `*.fly.dev` host) can be claimed by someone else. To redeploy later, follow **First deploy**; if the name is taken, change `app` and `PUBLIC_BASE_URL` in `fly.toml`.
- **Discord:** if the Discord app's Root Mapping points at the removed host, repoint it (e.g. to the dev tunnel) or the activity won't load.
- **Custom domain:** remove the DNS records created for `fly certs add`.
- **Redis:** the Upstash data isn't touched. Sessions and tickets expire on their own TTLs; app records stay until deleted (use a distinct `REDIS_KEY_PREFIX` per app to make them easy to find).

## Custom domain (website mode)

```sh
fly certs add example.com --app webapp-example
```

Create the DNS records `fly certs add` prints (A/AAAA, or a CNAME to `webapp-example.fly.dev`), wait for `fly certs show example.com` to report the certificate as issued, then update `PUBLIC_BASE_URL` in `fly.toml` and redeploy. Login links are built from it.

## Discord app setup

For apps with Discord sign-in: setting `DISCORD_CLIENT_ID` / `DISCORD_CLIENT_SECRET` turns on "Sign in with Discord" on the website, and (with the `discord-activity` Cargo feature, which the example enables) activity sign-in. Once per Discord application, in the [Developer Portal](https://discord.com/developers/applications):

1. **OAuth2 → Redirects:** add `{PUBLIC_BASE_URL}/` for every environment whose website uses this Discord app, exactly as `PUBLIC_BASE_URL` is set plus a trailing `/`: e.g. `http://localhost:8080/` for local dev and `https://webapp-example.fly.dev/` (or the custom domain) in production. Discord web sign-in redirects back there; an unregistered one makes Discord show "Invalid OAuth2 redirect_uri". Discord matches the string exactly, and the portal will save a pasted leading or trailing space as part of the URL, so check for those. (The activity's SDK `authorize` never uses a redirect, but fails with `invalid_request: Missing "redirect_uri"` unless at least one is registered. An activity-only Discord app can use the placeholder `https://127.0.0.1`.)
2. **OAuth2:** copy the Client ID and Client Secret into `.env` (`DISCORD_CLIENT_ID`, `DISCORD_CLIENT_SECRET`) and into the Fly secrets.
3. **Activities → Settings** (activity apps only): enable Activities.
4. **Activities → URL Mappings** (activity apps only): Root Mapping prefix `/` → target host **without** `https://`: the Fly host (`webapp-example.fly.dev`) or the custom domain in production, and the dev tunnel host (`webapp-dev.fadetovoid.win`) in development. No proxy path mappings are needed, because everything is served from one origin and the Discord SDK is bundled.

**Dev vs. production mapping:** a Discord app has a single Root Mapping. For the example, repoint it temporarily to test production, then point it back to the tunnel. For a real app, keep the production app's mapping on production, and create a second Discord app (with its own client ID and secret) for development.

## Google app setup

For apps with Google sign-in: setting `GOOGLE_CLIENT_ID` / `GOOGLE_CLIENT_SECRET` turns on "Sign in with Google" on the website. Once per app, in the [Google Cloud Console](https://console.cloud.google.com/) → **Google Auth Platform**:

1. **Project:** create or select one (e.g. one per app).
2. **Branding:** app name, user support email, developer contact email.
3. **Audience:** user type **External**. A new app is in **Testing**: only the listed **test users** (up to 100) can sign in, so add your own Google account. When the site goes live, click **Publish app**: with only the scopes below there's no review, no "unverified app" warning and no user cap.
4. **Data Access:** add the scopes `openid`, `.../auth/userinfo.email` and `.../auth/userinfo.profile` (all non-sensitive).
5. **Clients → Create client → Web application:** under **Authorized redirect URIs** add `{PUBLIC_BASE_URL}/` for every environment, exactly (scheme, host, port, trailing `/`, no spaces), e.g. `http://localhost:8080/` and `https://webapp-example.fly.dev/`. One client can hold all of them. "Authorized JavaScript origins" isn't needed. Changes can take a few minutes to a few hours to apply; until then Google shows `redirect_uri_mismatch`.
6. **Copy the Client ID and Client Secret right away.** Google shows the full secret only once, at creation (lost secret → add a new one). Put them in `.env` (`GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET`) and into the Fly secrets.

**Brand verification** is optional: without it, sign-in works, but the consent screen may not show the app's name and logo. It needs the app's domain verified in Google Search Console, a public home page and a privacy policy on that domain, and is usually automated (minutes), occasionally 2–3 business days. The Google button's look must follow [Google's branding guidelines](https://developers.google.com/identity/branding-guidelines) (the app's CSS).

## Environment variables

| Variable | Required | Secret | Where | Purpose |
|---|---|---|---|---|
| `UPSTASH_REDIS_REST_URL` | yes | yes | secret | Upstash REST endpoint |
| `UPSTASH_REDIS_REST_TOKEN` | yes | yes | secret | Upstash REST token |
| `REDIS_KEY_PREFIX` | no | no | secret or `[env]` | Namespace for every key, to share one DB between apps or environments (the example uses none) |
| `RESEND_API_KEY` | for email login | yes | secret | Send login emails via Resend |
| `RESEND_FROM_EMAIL` | with `RESEND_API_KEY` | no | secret | Sender address on a Resend-verified domain |
| `PUBLIC_BASE_URL` | for email, Discord or Google login | no | `[env]` | Origin users reach the site at: the emailed login link and the Discord / Google sign-in redirect (`{PUBLIC_BASE_URL}/`) are built from it |
| `APP_NAME` | no (default `webapp`) | no | `[env]` | Shown in the login email subject and body |
| `DISCORD_CLIENT_ID` | for Discord login, web and activity (set both or neither) | no | secret | Discord application ID |
| `DISCORD_CLIENT_SECRET` | with `DISCORD_CLIENT_ID` | yes | secret | Discord OAuth2 secret |
| `GOOGLE_CLIENT_ID` | for Google login (set both or neither) | no | secret | Google OAuth client ID |
| `GOOGLE_CLIENT_SECRET` | with `GOOGLE_CLIENT_ID` | yes | secret | Google OAuth client secret |
| `DEV_MODE` | no | no | local only | `1` with no Resend key prints login links to the console. **Never set in production.** |
| `BIND_ADDR` | no | no | image | Listen address (`0.0.0.0:8080` in the image; `127.0.0.1:3000` default locally) |
| `STATIC_DIR` | no | no | image | Client files (`/app/dist` in the image; `build/dist` default locally) |

## Adapting for a new app

1. Copy `Dockerfile`, `.dockerignore` and `fly.toml` into the app repo's root.
2. In `Dockerfile`, replace `example/client` and `example-server` (both the build and `COPY` lines) with the app's client directory and server package name. Keep its `Trunk.toml` outputting to `build/dist`, or change the `COPY --from=builder` path to match.
3. In `fly.toml`, set `app`, `primary_region`, `APP_NAME` and `PUBLIC_BASE_URL`.
4. Depend on the framework through its GitHub git dependency pinned to a tag, not a path (implementation guide §1). The build only sees the app's folder. If you develop against a local copy via a `.cargo/config.toml` patch, keep `.cargo` in `.dockerignore`.
5. Follow **First deploy** (and, for Discord or Google sign-in, **Discord app setup** / **Google app setup**) above with the new app's names.
