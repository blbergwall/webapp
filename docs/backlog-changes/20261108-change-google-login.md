# Change: Sign in with Google

Source: `docs/completed-research/20261108-add-login-with-google.md` (Findings, Follow-Up).

Add "Sign in with Google" to website mode, next to the email form and the Discord button. It's enabled at runtime whenever `GOOGLE_CLIENT_ID` / `GOOGLE_CLIENT_SECRET` are set (both or neither; requires `PUBLIC_BASE_URL`), with no Cargo feature. It uses Google's OpenID Connect authorization-code flow and ends in the normal 90-day session cookie.

- **Same SPA-driven flow as Discord web sign-in:**
  - `POST /api/auth/google/start` → `{url}` (`https://accounts.google.com/o/oauth2/v2/auth`, `response_type=code`, `scope=openid email profile`, `state`, `nonce` = the state, `redirect_uri={PUBLIC_BASE_URL}/`). It also sets a `google_state` cookie (`HttpOnly; Secure; SameSite=Lax; Path=/api/auth/google; Max-Age=600`).
  - Google returns to `{PUBLIC_BASE_URL}/?code=…&state=…` (or `?error=…&state=…`), and `<AuthGate>` calls `POST /api/auth/google/confirm {code, state}`.
  - `confirm` checks the state cookie (400) and clears it on every response. It exchanges the code at `https://oauth2.googleapis.com/token` (form, with `redirect_uri`) and reads the **ID token's** claims. Its signature isn't checked: Google says a server that got the token straight from the token endpoint can trust it. It checks `aud` = our client ID and `nonce` = the state, runs the account logic below, sets the session cookie, and returns `Account`.
- **Provider-prefixed state:** `discord.<hex>` and `google.<hex>`, so `<AuthGate>` routes a return on `/` to the right `confirm`. Discord's state changes to the prefixed form.
- **Match by email, only when Google is authoritative.** The email is trusted when `email_verified` and (`email` ends in `@gmail.com` or the `hd` claim is present).
  - `FG<sub>` exists → that account.
  - Else trusted and `FE<email>` exists → link: claim `FG<sub>` for it (`SET NX`; follow the winner on a lost race) and set its `google_id`.
  - Else create an account: `google_id: sub`, `email: trusted.then(email)`, display name from `name`, else the email's local part, else "Google user". Claim `FG<sub>`, and if trusted also `FE<email>` (`SET NX`; a lost race leaves the accounts separate).
  - Untrusted Google accounts are keyed by `sub` only, with `email: None`. Discord is unchanged (separate, `identify` only).
- **Server:**
  - New `google.rs` (authorize URL, code exchange, ID-token claims via a small hand-written base64url decoder, the trusted rule, `start` / `confirm`).
  - New shared `oauth.rs` (prefixed state generation, state cookie set/clear by name and path, `state_matches`; plain functions), and `discord_web.rs` moves onto it.
  - `account.rs`: Google find/link/create. `keys.rs`: new `FG` prefix in `FRAMEWORK_PREFIXES`.
  - `config.rs`: `GoogleConfig`.
- **Common:** `Account.google_id: Option<String>` (old records → `None`); `AuthConfig.google_sign_in: bool`; request/response types for start/confirm.
- **Client (`gate.rs`):** a "Sign in with Google" button (`webapp-google`) when `google_sign_in`; route returns by state prefix; messages for cancelled / expired / rejected.
- **Docs:**
  - API contract and conventions in `CLAUDE.md`.
  - Implementation guide: sign-in methods, the email-matching rule, `FG` in the key table, `Account.google_id`, and that the Google button's look must follow Google's branding guidelines (the app's CSS).
  - `deploy.md`: Google setup steps (project, Branding, Audience External + test users, Data Access scopes, Web client with exact redirect URIs `{PUBLIC_BASE_URL}/`, copy the secret at creation, Publish when live, optional brand verification) and the env var table.
  - `.env.example`, README.
- **Tests:**
  - Unit: base64url decoding, ID-token claims parsing, `aud`/`nonce` checks, the trusted-email rule (gmail, `hd`, unverified, other domain), state prefixes and routing.
  - Live Redis: the Google find / link / create paths with fake claims, including email sign-in reaching a Google-created account.
  - Manual: an end-to-end test with a real Google account (a test user) on `http://localhost:8080`.
- **Release:** new minor version (`v0.4.0`). Tag and push only when the user asks.

Open points to settle in the change:
- Shared `OAuthStartResponse` / `OAuthConfirmRequest` types for both providers vs. per-provider types.
- `prompt`: Google's default vs. `select_account`.
