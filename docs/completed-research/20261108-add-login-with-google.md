# Add Login with Google

**Status:** done

Now that login with Discord is implemented, next I would like to implement login with Google. Research the code changes needed, and also what needs to be set up with Google.

## Clarifications

- How should a Google sign-in relate to an existing email account with the same address? (Discord sign-ins are kept separate from email accounts, but Google reports a verified email address and is the authority for `@gmail.com`.)
  **Decision:** Match by email. A Google sign-in with a verified email signs into the email account for that address, and vice versa (an email sign-in reaches an account created through Google). The research works out when Google's email can be trusted for this, and how to do it safely.
- When should a website app offer Google sign-in?
  **Decision:** Credentials alone, like Discord: on whenever `GOOGLE_CLIENT_ID` and `GOOGLE_CLIENT_SECRET` are set. No Cargo feature.

## Findings

### Google's protocol (OpenID Connect, authorization-code flow)

From [Google's OpenID Connect guide](https://developers.google.com/identity/openid-connect/openid-connect) and its [discovery document](https://accounts.google.com/.well-known/openid-configuration):

- **Authorize:** `https://accounts.google.com/o/oauth2/v2/auth` with `response_type=code`, `client_id`, `redirect_uri`, `scope` (must start with `openid`; we'd use `openid email profile`), `state` (recommended), and `nonce` (listed as required). `prompt` may be `none`, `consent` or `select_account`.
- **Token:** `POST https://oauth2.googleapis.com/token`, form-encoded `code`, `client_id`, `client_secret`, `redirect_uri`, `grant_type=authorization_code`. The response includes `access_token` and an **`id_token`** (a JWT with the user's claims).
- **ID token trust:** "since you are communicating directly with Google over an intermediary-free HTTPS channel and using your client secret … you can be confident that the token you receive really comes from Google and is valid." So a server that gets the ID token straight from the token endpoint can read its claims **without verifying the JWT signature** (no JWKS, no crypto crate).
- **Claims:** `sub` (the stable, never-reused user ID: "Use `sub` … as the unique-identifier key"), `email`, `email_verified`, `hd` (the Google Workspace domain, absent for consumer accounts), `name` (with `profile`; "never guaranteed to be present").
- **Userinfo** (`https://openidconnect.googleapis.com/v1/userinfo`) returns `sub`, `email`, `email_verified`, `name` etc., but **`hd` is not in its `claims_supported`**. The ID token is the reliable source for `hd`.
- **Redirect URIs** "must exactly match one of the authorized redirect URIs," including "scheme, case, and trailing '/'". `http://` is allowed only for localhost ([Google Cloud help](https://support.google.com/cloud/answer/15549257)). Changes "may take 5 minutes to a few hours" to take effect.
- **PKCE** is supported (`S256`) but not needed: the server is a confidential client that holds the secret.

### When Google's email can be trusted ("match by email")

From [Google's ID-token guide](https://developers.google.com/identity/gsi/web/guides/verify-google-id-token), Google is **authoritative** for an address only when:
- the `email` ends in **`@gmail.com`** (a Gmail account), or
- **`email_verified` is true and `hd` is set** (a Google Workspace account, where the domain admin controls the mailbox).

Otherwise (a Google account registered with some other address, no `hd`), "Google is not authoritative", and `email_verified` can be true even though "ownership of the third party email account may have since changed." That is the same stale-ownership risk that made us reject email auto-linking for Discord.

**Recommendation:** match by email **only when Google is authoritative** (`email_verified` and (`@gmail.com` or `hd` present)). For any other Google account, keep it separate: its account is keyed by `sub` alone, with `email: None`, so the framework never claims that address. Users with an @gmail.com or company Workspace address, by far the common case, get the matching that was decided. An attacker can't take over an email account through a stale third-party address.

Edge case, accepted: Gmail ignores dots and `+tags`, but email login treats `j.ane@gmail.com` and `jane@gmail.com` as different addresses. Google returns the account's canonical address, so a user who typed a variant for email login gets a separate account. Canonicalizing Gmail addresses would change email login's behaviour, so it's not proposed.

### Accounts and identities

- New identity record **`FG<sub>`** → `account_id` (a new framework prefix: add it to `FRAMEWORK_PREFIXES` and the implementation guide's table).
- `Account` gains **`google_id: Option<String>`**, like `discord_id`. Old `FA` records without the field deserialize as `None`.
- Google sign-in, given `sub`, `email`, `trusted` and `name`:
  1. `FG<sub>` exists → that account.
  2. Else if `trusted` and `FE<email>` exists → **link**: claim `FG<sub>` for that account (`SET NX`; on a lost race, follow the winner) and set its `google_id`.
  3. Else create an account (`google_id: sub`, `email: trusted.then(email)`, display name from `name`, else the email's local part, else "Google user"), claim `FG<sub>` as `find_or_create` does today, and if `trusted`, also claim `FE<email>` (`SET NX`). If that claim loses a race to a concurrent email sign-up, the accounts stay separate.
- **"Vice versa" comes for free:** once `FE<email>` points at the Google-created account, email sign-in for that address reaches it.
- Discord is unaffected: it stays `identify`-only and separate.

### Flow and code: reuse the Discord web pattern

The flow is the same SPA-driven start/confirm pair as Discord web sign-in, so trunk's proxy needs nothing and errors stay ordinary statuses:
- `POST /api/auth/google/start` → `{url}` and a `google_state` cookie (`Path=/api/auth/google`, 10 min).
- Google redirects to `{PUBLIC_BASE_URL}/?code=…&state=…` (plus extra params such as `scope` and `authuser`), or `?error=access_denied&state=…`.
- `POST /api/auth/google/confirm {code, state}`: check the state cookie, exchange the code (with `redirect_uri`), read the ID token's claims, run the account logic above, set the website session cookie, return `Account`.

**Telling the two providers apart on return:** both land on `/` with `code` + `state`. Use a **provider prefix in the state** (`discord.<hex>`, `google.<hex>`). The server generates it, the client routes `confirm` by the prefix, and each provider's cookie still holds the full value. Discord's state format changes, but a state only lives 10 minutes, so there's no compatibility concern. (Rejected: telling them apart by Google's extra query params, which is fragile; and a distinct redirect URI per provider, which needs a path or query that the SPA and `ServeDir` serve.)

**Nonce:** send `nonce` = the state value and check the ID token's `nonce` equals it, along with `aud` = our client ID. That's cheap, satisfies Google's "required" nonce, and catches mix-ups.

**Code shape:**
- `webapp-server`:
  - New `google.rs`: authorize URL, code exchange, ID-token claims (split the JWT, base64url-decode the payload, `serde_json`; a ~20-line hand-written base64url decoder with tests, per the minimize-dependencies principle), the `trusted` rule, and the `start` / `confirm` handlers.
  - New small shared `oauth.rs` with the bits both providers now repeat: prefixed state generation, the state cookie set/clear (by name and path), `state_matches`. Plain functions, no trait.
  - `discord_web.rs` moves onto those helpers.
  - `account.rs`: Google find/link/create. `keys.rs`: `FG`.
  - `config.rs`: `GoogleConfig` from `GOOGLE_CLIENT_ID` / `GOOGLE_CLIENT_SECRET` (both or neither; requires `PUBLIC_BASE_URL`).
- `webapp-common`:
  - `Account.google_id`.
  - `AuthConfig` gains `google_sign_in: bool` (the client needs no client ID).
  - `GoogleStartResponse` / `GoogleConfirmRequest`: or reuse the Discord web request/response types under neutral names (`OAuthStartResponse`, `OAuthConfirmRequest`), which is simpler. Settle in the change.
- `webapp-client/gate.rs`:
  - A "Sign in with Google" button (`webapp-google`) next to the Discord one.
  - Route returns by state prefix (shared return parsing; `error=access_denied` → "cancelled").
- Docs: API contract, implementation guide (methods, matching rule, `FG`), `deploy.md` (Google setup below), `.env.example`.
- Tests: base64url decoding, claims parsing, the trusted-email rule (gmail, hd, unverified, other domain), the state prefix, and a live Redis test of the find/link/create paths with fake claims (no Google needed).

### Setting up Google (per app)

In the [Google Cloud Console](https://console.cloud.google.com/) → **Google Auth Platform**:

1. **Create or select a project** (e.g. one per app).
2. **Branding:** app name, user support email, developer contact email.
3. **Audience:** user type **External**. A new app starts in **Testing**: only listed **test users** (up to 100) can sign in, so add your own Google account. **Publish** to production when the site goes live.
4. **Data Access:** add the scopes `openid`, `.../auth/userinfo.email`, `.../auth/userinfo.profile`. These are **non-sensitive**, so there is no scope verification or security review.
5. **Clients → Create client → Web application:** under **Authorized redirect URIs** add `{PUBLIC_BASE_URL}/` for each environment, exactly (scheme, host, port, trailing `/`, no spaces): `http://localhost:8080/` and `https://<production host>/`. "Authorized JavaScript origins" isn't needed (the exchange is server-side). One client can hold both URIs; unlike Discord's single URL mapping, there's no need for separate dev and production apps.
6. **Copy the Client ID and Client Secret immediately.** Google shows the full secret **only once**, at creation; afterwards only its last four characters (lost secret → rotate). Put them in `.env` (`GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET`) and the Fly secrets.
7. Allow a few minutes (up to hours) for redirect-URI changes to take effect.

**Brand verification** ([Google](https://developers.google.com/identity/protocols/oauth2/production-readiness/brand-verification)) is needed only to show the app's **name/logo** on the consent screen for an External, published app. It requires domains verified in Google Search Console, a public home page and a privacy policy on that domain. It's usually automated (minutes), sometimes 2–3 business days. Without it, sign-in still works with these non-sensitive scopes, but the consent screen shows less branding. To confirm when publishing; it doesn't block development.

**Button branding:** Google's sign-in branding guidelines govern how a "Sign in with Google" button looks (logo, wording). The framework's views are unstyled, so this falls to each app's CSS. Document it as an app responsibility.

### Open points for the change
- Shared request/response types (`OAuth*`) vs. per-provider types.
- `prompt`: Google's default (the account chooser only when needed) vs. `select_account` (always choose, which helps users with several Google accounts). Leaning default.
- Refresh tokens, Google API access and `login_hint` are out of scope (sign-in only).

## Follow-Up

- How hard is it to move the Google app from Testing (only listed users) to production (all users)? Will it be a problem for small projects?
  **Answer:** It's easy, and not a problem for small projects. From [Google's publishing-status help](https://support.google.com/cloud/answer/15549945):
  - **Moving to production is one button:** "A project's publishing status is considered In production after selecting the Publish app button," and then any Google account can sign in.
  - **No review for these scopes.** The "unverified app" warning and the 100-user lifetime cap apply only to apps that request **sensitive or restricted** scopes before verification. Sign-in only asks for `openid`, `email` and `profile`, which are non-sensitive, so neither applies.
  - **Testing is fine during development.** Up to 100 listed test users. For apps requesting only `openid`/`email`/`profile`, Google exempts test users' grants from the usual 7-day expiry and warning.
  - **The only catch is branding:** the app's **name and logo may not appear on the consent screen until brand verification**. That needs domains verified in Google Search Console, a public home page and a privacy policy on that domain. It's usually automated (minutes), occasionally 2–3 business days, and it's optional: sign-in works without it. For a small project, publish without it and add it later if the plain consent screen bothers you.
  - Adding any other Google API scope later (e.g. Drive or Calendar) would change this: sensitive scopes bring review, the warning and the user cap.
- The user accepted the findings and asked for a change file.

## Next Steps

- Change: `docs/backlog-changes/20261108-change-google-login.md`
