# Remove app on Fly.io

**Status:** done

Please document how to delete/remove the example app from Fly.io in `docs/deploy.md`, and do that. I don't need this app taking up space on Fly.io.

## Clarifications

Current Fly state (checked 2026-10-20): `webapp-example` is suspended, with one stopped machine (`2870544f4dd5d8`, `iad`), no volumes and no certificates.

Should the deploy files (`Dockerfile`, `.dockerignore`, `fly.toml`) be removed too?
**Decision:** No. They are the template that `docs/deploy.md` tells new apps to copy, and they let the example be redeployed later via **First deploy**.

Does the Discord app need changing?
**Decision:** No. The "webapp example" Discord app's Root Mapping points at the dev tunnel (`webapp-dev.fadetovoid.win`), not the Fly host. The docs will mention repointing the mapping for apps whose mapping targets the removed host.

Does Redis data need cleaning up?
**Decision:** No. Production shares the dev Upstash DB; any leftover production sessions expire on their own TTL.

## Implementation Plan

**Files to modify:**
- `docs/deploy.md` — add a **Removing an app** section (`fly apps destroy`, what it deletes, that it's irreversible and frees the `*.fly.dev` name, and the follow-ups outside Fly: Discord URL mapping, custom-domain DNS records, Redis keys). Note at the top that the example is not currently deployed.
- `CLAUDE.md` — **Deployment** section: say the example was deployed as `webapp-example` and has been removed; the files and steps remain for redeploying.

**Approach:**
Update the docs first, then, after the user confirms, run `fly apps destroy webapp-example --yes` and check with `fly apps list` that it's gone.

**Risks / things to watch for:**
- Destroying is irreversible and changes Fly, so it runs only on the user's explicit confirmation.
- Fly app names are global: once freed, someone else could claim `webapp-example`, so a redeploy might need a new name (in `fly.toml` and `PUBLIC_BASE_URL`).

## Result

The user confirmed, and `fly apps destroy webapp-example --yes` reported `Destroyed app webapp-example` (2026-10-20). `fly apps list` no longer shows it.
