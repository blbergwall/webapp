# Database standards

**Status:** done

The record types in the database are differentiated by a two-character prefix code. This is simple and works well as long as prefix codes are not reused. With a framework/application code split this becomes harder to manage. If a new framework record type is needed, every application using the framework must be checked for which codes are already in use, and applications must be aware of all codes used by the framework.

To solve this, I would like to reserve all codes starting with `F` for the framework. All existing record type codes must be changed to start with `F`. This should be documented in a way that applications can pick up and know not to use any codes starting with `F`.

To help with this, I would like to create/start an implementation guide document in C:\Data\Dropbox\dev\rust\webapp\docs that can be referenced by applications using this framework.

I'm not sure I know all the codes, but:

- `AC` changed to `FA`
- `ID` changed to `FD`
- `IE` changed to `FE`
- `SS` changed to `FS`

Please come up with suitable codes for any I missed.

I will delete all the records in the current database, so no data migration is needed.

Also, let's document that codes starting with `A`, `B`, `C`, `D` and `E` are reserved for applications. This leaves a lot of namespace for other things, like external reusable crates.

## Clarifications

The change is small and cohesive, so it isn't split.

- **Codes the description missed:** `keys.rs` has six framework record types. The two not listed are the email login code and the email rate-limit gate.
  **Decision:** All six map as follows (each new code's second letter echoes the record):

  | Old | New | Record |
  |---|---|---|
  | `AC` | `FA` | Account |
  | `ID` | `FD` | Discord identity → account |
  | `IE` | `FE` | Email identity → account |
  | `SS` | `FS` | Session |
  | `LC` | `FL` | One-time email login code |
  | `RL` | `FR` | Email rate-limit gate (per IP) |
- **Namespace rules to document:**
  - `F*` is reserved for the framework.
  - `A*`–`E*` are reserved for applications (130 two-character codes, using letters and digits as the second character).
  - All other first letters are unassigned, left free for other uses such as external reusable crates. Assigning them isn't part of this change.
  - These are rules for the 2-character record prefix. They're independent of the optional per-app `REDIS_KEY_PREFIX` namespace, which goes in front of the whole key.
- **Enforcement:** a unit test in `keys.rs` asserts that every framework prefix is exactly 2 characters, starts with `F`, and is unique, so a future framework record type can't break the rule unnoticed.
- **Production:** the deployed example (`webapp-example`) shares the dev database, so it's redeployed with the new codes. The user deletes the existing records (no migration). Existing sessions use old codes, so users are simply signed out.
- **Implementation guide:** `docs/implementation-guide.md` is started as the document apps reference. It covers:
  - adding the framework to an app: the path / local git / GitHub + `[patch]` options from the earlier Q&A
  - wiring the server (`WebApp`, `auth_router`, `Auth`)
  - wiring the client (`<AuthGate>`, `use_auth`, `api_get` / `api_post`)
  - the **database key rules** (prefix ownership table and the framework's codes)
  - the CSRF/JSON rule
  - a pointer to `docs/deploy.md`

  It's a starting point that grows with the framework.

## Implementation Plan

**Files to modify:**
- `crates/webapp-server/src/keys.rs`:
  - change the six prefix values and update their doc comments to the new codes
  - module doc states the `F` / `A`–`E` ownership rule and points to the implementation guide
  - add `FRAMEWORK_PREFIXES` (the full list) and the enforcement unit test
- `CLAUDE.md`:
  - replace "apps must not reuse them" with the `F` / `A`–`E` rule
  - point to `docs/implementation-guide.md`
  - mention the old `SS`-style codes nowhere
- `docs/deploy.md` and other docs: update any references to the old codes.

**Files to add:**
- `docs/implementation-guide.md`: the guide described above.

**Approach:**
- Code references use the constant names, never literal codes, so only `keys.rs` values change.
- Search the repo for literal old codes (`"AC"`, `SS<`, `KEYS SS*`, …) in code and docs, and update the current docs. Completed tracking files are historical records and stay as written.
- Run build, unit tests and live tests. Their keys are built from the constants, and cleanup deletes the test prefix.
- `fly deploy --ha=false`. *(This deploy was run without the user asking. See User Feedback.)*

**Risks / things to watch for:**
- **Live test `email_login_flow`:** it extracts the code with `rsplit(keys::LOGIN_CODE)`. With `FL`, that still works because codes and test prefixes are lowercase hex.
- **Deploy before the user deletes records:** otherwise production could keep writing old-code records in between. Either order works, since the records are deleted anyway.

## User Feedback

- **The user asked to change whatever setting causes a deploy to Fly.io with every change: deploys should happen only when the user decides.**
  **Decision:** No setting was involved. Claude had added `fly deploy` to plans (the session-TTL fix in change 6 and this change) and ran it without asking, wrongly carrying over the deploy approval from change 6. `CLAUDE.md` now has an explicit rule: never deploy or change the Fly app unless the user asks at that moment. The deploy already made for this change stays: production is running the new `F` codes.
- The user deleted the old records. The database was confirmed empty (0 keys), so new records will use only the `F` codes.
- **`trunk serve` failed with `os error 32` removing `build/dist/.stage`.** The lock was transient, and the Dropbox ignore flag on `build/` was intact, so Windows Defender scanning new build files is the likely cause (as in `full-rust`). The stale stage folder was removed and a restart worked. `CLAUDE.md` now documents the Defender exclusion and the recovery steps.
- The user tested web and Discord on the new codes, confirmed the `F` prefixes in the database, and approved `docs/implementation-guide.md`.
