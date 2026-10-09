# Change 4: Discord Activity Login (Server)

Order: 4 of 6. Depends on change 2. Architecture source: `docs/completed-research/20261009-web-app-framework.md`.

Port `full-rust`'s Discord code exchange to an HTTP endpoint in `webapp-server`:

- `POST /api/auth/discord {code}`:
  - exchanges the code at `discord.com/api/oauth2/token` using `DISCORD_CLIENT_ID` and `DISCORD_CLIENT_SECRET`
  - fetches `users/@me` (`identify` scope only)
  - finds the account via `ID<discord_id>`, or creates it with `display_name` set to `global_name`, falling back to `username`
  - creates a session
  - returns `{token, discord_access_token, account}`; no cookie is set, because activity mode uses a bearer token
- `GET /api/auth/config` includes `discord_client_id`. Discord is optional: if the env vars are unset, the Discord endpoint returns 404 and the config omits the client ID.
- Never trust identity data that comes from the client. Only the server-side exchange establishes who the user is.
