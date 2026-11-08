# Discord Web Login

Add a web-based "Log in with Discord" option for website mode (a normal browser, not a Discord activity), alongside email magic-link login. It should be a standard OAuth2 authorization-code redirect flow that ends in the normal session cookie, and build on the server-side Discord OAuth core that stays unconditional after the `discord-activity` feature change (see `docs/completed-research/20261108-separate-discord-activity-support.md`).

Questions to research:
- Flow and endpoints: e.g. `GET /api/auth/discord/start` (redirect to Discord) and a callback route. How is the OAuth `state` generated, stored (cookie vs. Redis `F?` record) and checked (CSRF)?
- Redirect URI configuration for local dev, tunnels and production (derive from `PUBLIC_BASE_URL`?), and the Developer Portal setup it needs.
- Scopes: `identify` only, or also `email`? If `email`, should a verified Discord email match or link to an existing email account?
- Account linking: a person who signs in by email and by Discord gets two accounts today. Is linking in scope, and how would it work safely?
- Client: how `AuthConfig` and `<AuthGate>` show the Discord button next to the email form; how errors from the callback reach the SPA.
- Runtime enablement: on whenever `DISCORD_CLIENT_ID` / `DISCORD_CLIENT_SECRET` are set, or a separate switch? Does it need a Cargo feature (probably not: no SDK, little code)?
- Interaction with an app that also enables `discord-activity` (same Discord app and credentials; same `FD` identity).
