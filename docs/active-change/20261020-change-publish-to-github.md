# Change: Publish to GitHub

**Status:** developed

Source: `docs/completed-research/20261020-add-to-github.md` (findings, decisions and the proposed order). Needed by ds's change 5 (deploy to Fly), which switches ds to a pinned git dependency on this repo.

Publish webapp as the **public** repo `https://github.com/blbergwall/webapp`, tagged `v0.1.0`, so Fly's remote builder can fetch it.

- **License:** `MIT OR Apache-2.0`. Add `LICENSE-MIT` and `LICENSE-APACHE`, set `license` in `[workspace.package]` with `license.workspace = true` in the five crate manifests.
- **Third-party notice:** add `crates/webapp-client/js/THIRD-PARTY-NOTICES` with the MIT text of `@discord/embedded-app-sdk` (and its bundled dependencies' notices). Update the esbuild command in `js/README.md` to `--legal-comments=external`, so later regenerations produce the notices file.
- **Template `.dockerignore`:** add `.cargo` with a comment (an app's git-ignored `.cargo/config.toml` path patch breaks the Fly build if it reaches the build context).
- **Docs:**
  - `docs/implementation-guide.md` §1: replace `<owner>` with `blbergwall`, and add the `.dockerignore` → `.cargo` rule next to the patch snippet. Note that a patched `Cargo.lock` records path packages and that the Dockerfile must not use `--locked`.
  - `docs/deploy.md` ("adapt for a new app"): switch to the git dependency, exclude `.cargo`.
  - `CLAUDE.md`: the GitHub URL, the HTTPS remote, and the release convention (annotated `vX.Y.Z` tags matching `workspace.package.version`, never moved or deleted once pushed, a fix is a new tag).
- **History rewrite before the first push:** set repo-local `user.email` to `727986+blbergwall@users.noreply.github.com`, re-author every commit (author and committer email) with `git filter-branch --env-filter`, delete `refs/original/`, and verify `git log --all --format='%ae %ce' | sort -u` shows only the noreply address. Dates and trees stay the same.
- **Branch:** rename `master` to `main`.
- **Tag:** annotated `v0.1.0` on the commit to publish.
- **Push (user runs it, since it needs the token prompt):** create the empty public repo in the GitHub web UI (no README, license or .gitignore), `git remote add origin https://github.com/blbergwall/webapp.git`, `git push -u origin main`, `git push origin v0.1.0`. Use a fine-grained PAT (only `webapp`, Contents read and write) as the password. `wincred` stores it.
- **Verify:** a scratch project with `webapp-common = { git = "https://github.com/blbergwall/webapp", tag = "v0.1.0" }` resolves it (`cargo metadata` writes `?tag=v0.1.0#<commit>` into its lock).

The history rewrite and the push are outward-facing or hard to undo: run them only when the user says go.

## Clarifications

- The process commits once at the end, but the tag and push need the edits committed first. How do they fit together?
  **Decision:** Publish during testing (step 6). After the edits build, commit them, rewrite the emails, rename to `main`, and tag `v0.1.0`. The user pushes, then the tag is verified from GitHub and the results are recorded here. The final "done" commit lands on `main` after the tag, and the user pushes it too.
- Copyright holder for the licenses?
  **Decision:** "Copyright (c) 2026 Brent Bergwall".
- Add a README for the GitHub front page?
  **Decision:** Yes, a short one: what it is, how apps depend on it, a link to the implementation guide, and the license.
- Found while clarifying: esbuild `--legal-comments=external` produces **no** notices file for this bundle, because the SDK's sources carry no legal comments. A scratch rebuild was byte-identical to the committed `discord.js` (minus the header). The SDK ships its dependencies pre-bundled under `output/lib/`: `eventemitter3`, `zod`, `big-integer`, `uuid`, `decimal.js-light`, `lodash.transform` and `tslib`.
  **Decision:** Write `THIRD-PARTY-NOTICES` from those packages' own license files, and keep `--legal-comments=none` in the README. The README instead says to update the notices when the SDK's bundled libraries change.

## Implementation Plan

**Files to add:**
- `LICENSE-MIT`: MIT text, "Copyright (c) 2026 Brent Bergwall".
- `LICENSE-APACHE`: the standard Apache License 2.0 text.
- `README.md`: short repo front page.
- `crates/webapp-client/js/THIRD-PARTY-NOTICES`: license texts for `@discord/embedded-app-sdk` (MIT) and its bundled libraries: `eventemitter3`, `zod`, `uuid`, `decimal.js-light` and `lodash.transform` (MIT), `big-integer` (Unlicense) and `tslib` (0BSD). Assembled from each package's own license file (npm scratch install).

**Files to modify:**
- `Cargo.toml`: `license = "MIT OR Apache-2.0"` in `[workspace.package]`.
- `crates/*/Cargo.toml`, `example/*/Cargo.toml`: `license.workspace = true`.
- `crates/webapp-client/js/README.md`: point at `THIRD-PARTY-NOTICES` and say when to update it.
- `crates/webapp-client/js/discord.js`: header line says "license texts in THIRD-PARTY-NOTICES". The header is a comment only, so the code is unchanged.
- `.dockerignore`: add `.cargo` with a comment.
- `docs/implementation-guide.md` §1: `<owner>` → `blbergwall`. After the patch snippet: list `.cargo` in `.dockerignore`, the patched `Cargo.lock` holds path packages (the tag is the pin), and don't build with `--locked`. Releases: bump the tag and run `cargo update -p webapp-server -p webapp-client`.
- `docs/deploy.md` "Adapting for a new app": a step to switch to the GitHub git dependency, and the `.cargo` line in `.dockerignore`.
- `CLAUDE.md`: GitHub URL, HTTPS remote (git's bundled ssh is broken on this machine), and a **Releasing** subsection (tag convention, push commands).

**Approach:**
1. Make the file edits. Run `cargo build`, `cargo test`, the wasm `cargo check`s with and without `ws`, and `trunk build --release` (so the README/notices changes don't affect the client build).
2. Step 6 (only on the user's go, per the clarification): commit the edits. Set the repo-local `user.email`. Run `git filter-branch --env-filter` over all refs to set the author and committer email to the noreply address. Delete `refs/original/` and verify. `git branch -m master main`, then `git tag -a v0.1.0`. The user creates the empty repo and pushes. Then verify from a scratch project that `tag = "v0.1.0"` resolves from GitHub. Record the results here.
3. Step 7: final commit on `main` (tracking file done), and the user pushes it.

**Risks / things to watch for:**
- `filter-branch` rewrites every hash. Nothing outside the repo references them yet (ds uses path deps). Do it before anything is pushed. Keep `refs/original/` until the verify passes, then delete it.
- The `git commit` co-author trailer is unaffected by the rewrite (only the email headers change).
- The Apache text must be the unmodified license file (fetched from apache.org), not retyped.
- The license versions in `node_modules` may be newer than the copies the SDK bundled, but the license terms and copyright lines are the same in practice. The notices name the packages without pinning versions.
- An old git (2.29) plus the `wincred` helper: the first push prompts for username/password in the user's terminal, using the PAT as the password.

## Implementation Notes

- All file edits from the plan are done. `LICENSE-APACHE` is the official apache.org text (SHA-256 `cfc7749b…3d30`, the canonical file). `THIRD-PARTY-NOTICES` was assembled from each package's license file in a scratch npm install (tslib via `npm pack`). The `discord.js` header line was also corrected: it said "dependencies MIT/Apache-2.0", but `big-integer` is Unlicense and `tslib` is 0BSD.
- Git Bash `sed -i` turned three CRLF manifests (`Cargo.toml`, `example/*/Cargo.toml`) into LF. CRLF was restored, so the diff is one line per manifest.
- Build check: `cargo build`, `cargo test` (28 passed, 5 ignored), `cargo test -p webapp-server --features ws`, the wasm `cargo check` of `webapp-client` with and without `ws` and of `example-client`, and `trunk build --release` all pass. `cargo metadata` reports `MIT OR Apache-2.0` for all five packages.
