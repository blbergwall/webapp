# Change: Publish to GitHub

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
