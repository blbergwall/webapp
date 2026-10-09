# Change 2: Server Core: Redis, Accounts, Sessions

Order: 2 of 6. Architecture source: `docs/completed-research/20261009-web-app-framework.md`.

Implement the foundation of `webapp-server` (and its shared types in `webapp-common`):

- **Upstash Redis REST client:** port `neverending-contrivance`'s `server/src/redis.rs`, with an optional `REDIS_KEY_PREFIX` namespace. Export it publicly so apps can store their own data.
- **Key module:** the 2-letter prefixes and TTLs (`AC`, `IE`, `ID`, `SS`, `LC`, `RL`), documented the same way as `neverending-contrivance`'s `keys.rs`.
- **IDs and tokens:** generated with `getrandom` and hex-encoded.
- **Accounts:** `AC<account_id>` records holding `{id, created, display_name, email?, discord_id?}`, plus identity maps `IE<email>` and `ID<discord_id>` that point to `account_id`. Each identity is claimed with `SET NX` so two concurrent creates can't make two accounts.
- **Sessions:** `SS<token>` maps to `account_id`, with a 90-day sliding TTL.
- **`Auth` extractor:** accepts `Authorization: Bearer` or the session cookie. Cookie parsing and building are hand-written (no `axum-extra`).
- **`auth_router()` routes:**
  - `GET /api/auth/config`
  - `GET /api/auth/me`
  - `POST /api/auth/logout`
- **CSRF:** state-changing endpoints accept JSON bodies only.
- **Tests:** unit tests for token generation, cookie parsing, and the extractor's header/cookie precedence.
