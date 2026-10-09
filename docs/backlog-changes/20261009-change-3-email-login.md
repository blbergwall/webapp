# Change 3: Email (Magic-Link) Login

Order: 3 of 6. Depends on change 2. Architecture source: `docs/completed-research/20261009-web-app-framework.md`.

Port `neverending-contrivance`'s passwordless email login to the JSON API in `webapp-server`:

- `POST /api/auth/email/start {email}`:
  - normalizes the email (trim, lowercase)
  - checks the IP rate limit `RL<ip>` (`Fly-Client-IP`, falling back to the peer address)
  - creates `LC<code>` holding `{email, pending}` with a 24h TTL
  - sends `{PUBLIC_BASE_URL}/?login_code=<code>` via the Resend REST API
  - sets the HttpOnly `pending_login` cookie
- `POST /api/auth/email/confirm {code, auto}`:
  - when `auto` is true, requires the `pending_login` cookie to match the code's record (protection against link scanners)
  - consumes the code with `GETDEL`
  - finds the account via `IE<email>`, or creates it
  - creates a session, sets the session cookie, and clears the pending cookie
- **Dev mode:** when `DEV_MODE=1` and `RESEND_API_KEY` is unset, print the link to the console instead of sending email.
- **Env vars:** `RESEND_API_KEY`, `RESEND_FROM_EMAIL`, `PUBLIC_BASE_URL`, `APP_NAME` (used in the email subject).
