# Change 6: Deployment Template (Fly.io)

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
