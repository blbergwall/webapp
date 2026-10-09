# Change 3: Email (Magic-Link) Login

**Status:** done

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

## Clarifications

The change is one deliverable, so it isn't split.

- **How should email sending be tested in this change?**
  **Decision:** Reuse `neverending-contrivance`'s `RESEND_API_KEY` / `RESEND_FROM_EMAIL` (its verified sending domain) in this repo's `.env`, and send real emails now. Dev mode (console link) is also verified.
- **Response contract** (decided in planning, for the client in change 5):
  - `start` → `204` on success, `400` for an invalid email, `429` when rate-limited, `404` when email login isn't configured. It reveals nothing about whether an account already exists.
  - `confirm` → `200` + `Account` JSON, with the session cookie set and the pending cookie cleared. `410 Gone` when the code is expired or already used. `409 Conflict` when `auto: true` is refused because the pending cookie doesn't match: the code is **not** consumed, so the client shows the "Finish signing in" button and retries with `auto: false`.
- **Email validation:** minimal. After trim + lowercase it must contain exactly one `@` with non-empty parts on both sides, have no whitespace, and be at most 254 chars. Resend rejects anything else that's undeliverable.
- **Email delivery modes:**
  - `RESEND_API_KEY` set → send via Resend (`RESEND_FROM_EMAIL` is then required).
  - Else `DEV_MODE=1` → print the link to the console.
  - Else → email login is disabled and the email routes return 404.
  - `PUBLIC_BASE_URL` is required whenever email login is enabled. `APP_NAME` is optional and defaults to `webapp`; it's used in the email subject.
- **Client IP for rate limiting:** the `Fly-Client-IP` header, else the TCP peer address when the app serves with `into_make_service_with_connect_info::<SocketAddr>()` (read as `Option<Extension<ConnectInfo<_>>>`, so apps aren't forced to enable it), else the literal `unknown`, which means one shared bucket. Off Fly, a client could spoof `Fly-Client-IP`; that only affects rate limiting, the same as in `neverending-contrivance`.
- **Pending cookie:** `pending_login=<hex>; HttpOnly; Secure; SameSite=Lax; Path=/api/auth/email; Max-Age=86400`. Its path limits it to the email endpoints.
- **CSRF:** both endpoints use axum's `Json` extractor, which already rejects non-JSON content types (415).

## Implementation Plan

**Files to modify:**
- `crates/webapp-common/src/lib.rs`: add `EmailStartRequest {email}` and `EmailConfirmRequest {code, auto}`.
- `crates/webapp-server/src/config.rs`: add `public_base_url: Option<String>`, `app_name: String`, and `email: EmailDelivery` (`Resend {api_key, from}` / `Console` / `Disabled`). `from_env` errors if Resend is configured without `RESEND_FROM_EMAIL`, or email is enabled without `PUBLIC_BASE_URL`.
- `crates/webapp-server/src/account.rs`: add `find_or_create_by_email(email)` (display name = local part) and remove the `#[allow(dead_code)]` on `find_or_create`.
- `crates/webapp-server/src/session.rs`: remove the `#[allow(dead_code)]` marks; add `pending_login_cookie(value)` and `clear_pending_login_cookie()`.
- `crates/webapp-server/src/error.rs`: add the variants `BadRequest`, `NotFound`, `Conflict`, `Gone`, and `TooManyRequests`.
- `crates/webapp-server/src/lib.rs`: register `mod email` and the two routes.
- `crates/webapp-server/src/live_tests.rs`: add the email flow test (with Console delivery): start → confirm with `auto` and a matching cookie; a mismatched cookie → 409 without consuming the code; a reused code → 410; the rate limit → 429.
- `example/server/src/main.rs`: serve with `into_make_service_with_connect_info::<SocketAddr>()`.
- `.env.example`, `CLAUDE.md`: new env vars and the response contract.

**Files to add:**
- `crates/webapp-server/src/email.rs`:
  - Handlers: `start` and `confirm`.
  - Helpers: `normalize_email`, `client_ip`, and `send_login_email`, which uses the shared `reqwest::Client` from `Redis`. The client moves to `WebApp` so `Redis` and the email sender share it.
  - The `LC` record `{email, pending}`.
  - `confirm` flow: when `auto` is true, `GET` the record and compare its pending value to the cookie (409 on mismatch); then `GETDEL` to consume it (410 if it's gone); `find_or_create_by_email`; `session::create`; respond with cookies.

**Approach:**
- Port the `neverending-contrivance` flow onto the JSON API and the change-2 primitives.
- No new dependencies. The Resend call is a single `reqwest` JSON POST, as in the reference.
- Live tests call the handler functions directly with their extractor values (`State(..)`, `Json(..)`, headers) and inspect the responses. *(Changed during implementation: this was planned with `tower::ServiceExt::oneshot`, but calling the handlers directly needs no extra dev-dependency.)*

**Risks / things to watch for:**
- **Peek-then-consume:** the `auto` check `GET`s the record before `GETDEL`. A concurrent consumer between the two just produces a 410, and single use is still guaranteed by `GETDEL`.
- **Email HTML:** the link is built from `PUBLIC_BASE_URL` plus a hex code, so it needs no escaping. `APP_NAME` is HTML-escaped in the body anyway.
- **Real sends use another project's Resend domain:** the "from" address will show neverending's domain until webapp apps get their own.

## User Feedback

- Real Resend send tested with neverending's key, to the user's address: the email arrived and the user pasted back the link. Auto-confirm from the same cookie session returned 200 and created the account (display name `blbergwall`). `/api/auth/me` and the protected `/api/hello` succeeded. Reusing the code returned 410, and logout followed by `/me` returned 401. No code changes were needed.
