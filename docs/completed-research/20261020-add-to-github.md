# Add to GitHub

**Status:** done

I have a project (`C:\Data\Dropbox\dev\rust\ds`) that is built on this framework, and I'm to the point where I need to deploy it to Fly.io. To do that, if I understand correctly, this project must be pushed to GitHub so that project can include it. This research is to work through how to do this.

## Clarifications

- Public or private repo? (A private repo means Fly's remote builder needs a GitHub token to fetch the dependency.)
  **Decision:** Public.
- Which GitHub account owns it?
  **Decision:** The main account `blbergwall`, so the repo is `https://github.com/blbergwall/webapp` (the name is free; checked 2026-10-20).
- Should `ds` also go on GitHub?
  **Decision:** No, webapp only. `fly deploy` uploads the local ds folder, so ds doesn't need a git host.
- The 9 commits are authored as `kothx@yahoo.com`, which a public repo would publish. Rewrite?
  **Decision:** Yes. Re-author to the GitHub noreply address `727986+blbergwall@users.noreply.github.com` (727986 is the account's user ID) before the first push.
- Add a license?
  **Decision:** `MIT OR Apache-2.0` (the usual Rust dual license).
- Rename `master` to `main`?
  **Decision:** Yes.

## Findings

### The premise is right

Fly's remote builder receives a tarball of the app's working directory (filtered by `.dockerignore`) and nothing else. ds's current `path = "../webapp/crates/..."` dependencies point outside that folder, so they don't exist in the build. A `file://` git URL fails the same way. The dependency has to be fetchable over the network, as the implementation guide already says (§1, "Git host, pinned"). For a public repo that's an anonymous HTTPS clone, which Cargo does with its built-in libgit2, so the `rust:1-slim-bookworm` builder needs no git CLI and no credentials.

Alternatives considered and rejected:
- **Private repo plus a build-secret token:** works, but adds a token to create, rotate and keep out of image layers. Not needed once the repo is public.
- **Building from the parent folder** (`C:\Data\Dropbox\dev\rust` as the build context, so the Dockerfile can `COPY` both `ds` and `webapp`): no git host needed, but the context is the whole dev folder (it needs a careful allow-list `.dockerignore`), and the deploy depends on whatever is in the webapp working copy, uncommitted changes included. That's exactly what tag pinning avoids.
- **`cargo vendor` / git submodule / copying webapp into ds:** more moving parts than a public repo, and copies drift.

### Pre-publish audit of webapp (all 9 commits, every file ever committed)

- **No secrets anywhere in history.** `.env` was never committed. `.env.example` holds only empty values. No Upstash URLs or tokens, Resend keys or Discord secrets were found by pattern search over all revisions.
- **Author email:** every commit is `Brent Bergwall <kothx@yahoo.com>` (author and committer). Rewriting is decided above. It was tested on a scratch clone with `git filter-branch --env-filter` (works with the installed git 2.29.2): author and committer dates are kept and every tree is byte-identical, so only commit hashes change. Nothing references webapp commit hashes yet (ds uses path dependencies), so nothing breaks.
  - If `kothx@yahoo.com` is a *private* email on the GitHub account and "Block command line pushes that expose my email" is on, GitHub would reject the push with `GH007`. The rewrite avoids that too.
- **Mildly personal, left as is:** your own Discord user ID in `docs/completed-changes/20261009-change-5-...md` (Discord IDs are visible to anyone in Discord's developer mode), the dev tunnel hostname `webapp-dev.fadetovoid.win` and `webapp-example.fly.dev` in `CLAUDE.md`, and local paths like `C:\Data\Dropbox\...`. None of these grant access to anything. The tunnel hostname only reaches your machine while `cloudflared` is running, and it serves the same app that's public on Fly anyway.
- **Third-party license notice:** `crates/webapp-client/js/discord.js` bundles `@discord/embedded-app-sdk` (MIT) and its dependencies. It was built with `--legal-comments=none`, so only a two-line attribution header remains. Redistributing MIT code publicly requires including its license text. Fix: add a notices file next to it. esbuild's `--legal-comments=external` writes one (`out.js.LEGAL.txt`) on the next regeneration, and until then a hand-written `THIRD-PARTY-NOTICES` with the SDK's MIT text is enough. Update `js/README.md`'s regeneration command to match.
- **Tracked `.claude/commands/` (the `/change` and `/research` skills):** harmless to publish, and useful to anyone working in the repo.

### Local tooling on this machine

- **`gh` CLI is not installed.** Create the repo in the GitHub web UI (New repository → `webapp`, Public, **no** README, license or .gitignore, so the first push goes into an empty repo). Installing `gh` (`winget install GitHub.cli`) is optional and isn't needed for a one-time setup.
- **Git is 2.29.2 (2020), installed under `C:\Data\Dropbox\git`.** Its bundled `ssh.exe` is broken (`msys-gssapi-3.dll` missing), so SSH remotes won't work as is. **Use an HTTPS remote.**
- **Credentials:** `credential.helper` is `wincred`. GitHub doesn't accept account passwords over HTTPS, so on the first push enter username `blbergwall` and a **personal access token** as the password. Use a fine-grained token with repository access limited to `webapp`, permission *Contents: Read and write*, and an expiry. `wincred` saves it in Windows Credential Manager for later pushes. Do the first push from your own terminal, since the prompt needs a TTY. Git Credential Manager Core 2.0.280-beta is also installed and could do a browser sign-in (`git -c credential.helper= -c credential.helper=manager-core push ...`), but that build is old and its GitHub OAuth flow is unverified. Upgrading Git for Windows (optional) would bring a current GCM and fix the bundled ssh.

### Tag and release convention

- Tag `v0.1.0` (matching `workspace.package.version = "0.1.0"`) as an **annotated** tag on the commit to publish, and push it explicitly (`git push origin v0.1.0`).
- **Tags are immutable once pushed.** Never move or delete one. A fix is a new tag (`v0.1.1`). Bump `workspace.package.version` in the same commit as the tag, so the version and the tag agree.
- Releasing later: commit → bump version → `git tag -a vX.Y.Z -m "..."` → `git push origin main vX.Y.Z` → in ds, change `tag = "vX.Y.Z"` and run `cargo update -p webapp-server -p webapp-client`.

### How ds consumes it (for ds's change 5, not done here)

```toml
# ds/Cargo.toml [workspace.dependencies]
webapp-server = { git = "https://github.com/blbergwall/webapp", tag = "v0.1.0", features = ["ws"] }
webapp-client = { git = "https://github.com/blbergwall/webapp", tag = "v0.1.0", features = ["ws"] }
```

Local development keeps using the working copy through a git-ignored `ds/.cargo/config.toml` (implementation guide §1):

```toml
[patch."https://github.com/blbergwall/webapp"]
webapp-server = { path = "../webapp/crates/webapp-server" }
webapp-client = { path = "../webapp/crates/webapp-client" }
```

Verified with a scratch project (webapp's local repo standing in for GitHub):
- **With the patch active, `Cargo.lock` records the crates as path packages (no `source` line).** So the lock committed in ds never pins a webapp commit. The `tag` in `Cargo.toml` is the pin, which is fine because tags are immutable.
- **Without the patch, Cargo re-resolves to the git tag on the next build** and writes `source = "git+https://...?tag=v0.1.0#<commit>"`. The Fly build doesn't pass `--locked`, so it just does this. Other locked versions are unaffected. (A `--locked` build fails here, so don't add `--locked` to the Dockerfile.)
- **If `.cargo/config.toml` reaches the Fly build context, the build fails** (`The system cannot find the path specified`, since `../webapp` doesn't exist there). `.gitignore` doesn't help: flyctl filters the context with `.dockerignore` only. A `.gitignore` is only used to generate one during `fly launch`. **ds's `.dockerignore` must list `.cargo`.** The webapp template's `.dockerignore` should list it too, with a comment, so new apps get it by copying.

Two other checks:
- The `ws` feature passes through git dependencies unchanged.
- The vendored SDK snippet (`#[wasm_bindgen(module = "/js/discord.js")]`) resolves against the crate's own directory, so it works from Cargo's git checkout.

### Docs to update in webapp when publishing

- `docs/implementation-guide.md` §1: replace `<owner>` with `blbergwall` (3 places), and add the `.dockerignore` → `.cargo` rule next to the patch snippet.
- `docs/deploy.md` ("adapt for a new app"): mention switching to the git dependency and excluding `.cargo`.
- `CLAUDE.md`: the GitHub URL, the HTTPS remote, and the release/tag steps above.
- License files plus `license = "MIT OR Apache-2.0"` in the five crate manifests (or once in `[workspace.package]`, with `license.workspace = true` in each).

### Proposed order (for a change file)

1. Docs/license/notice edits above, committed.
2. `git config user.email 727986+blbergwall@users.noreply.github.com` (repo-local), then rewrite history with `filter-branch --env-filter` (author and committer email), delete `refs/original/`, and verify (`git log --format='%ae %ce' | sort -u`).
3. `git branch -m master main`.
4. `git tag -a v0.1.0 -m "v0.1.0"`.
5. Create the empty public repo in the web UI. Then `git remote add origin https://github.com/blbergwall/webapp.git`, `git push -u origin main`, `git push origin v0.1.0` (first push from your terminal with the PAT).
6. Check: a scratch project with the GitHub git dependency resolves it (`cargo metadata` writes `?tag=v0.1.0#<commit>` into its lock).

Steps 2–5 publish code and rewrite history, so they run only when you say go.

Sources: [Fly community: remote builds respect .dockerignore](https://community.fly.io/t/dockerignore-being-ignored/4342), [Improving flyctl reliability (.dockerignore generated from .gitignore only at launch)](https://community.fly.io/t/improving-flyctl-reliability/8057), [Cargo: overriding dependencies / `[patch]` in config](https://doc.rust-lang.org/cargo/reference/overriding-dependencies.html), [GitHub: blocking pushes that expose your email (GH007)](https://docs.github.com/en/github/setting-up-and-managing-your-github-user-account/managing-email-preferences/blocking-command-line-pushes-that-expose-your-personal-email-address).

## Follow-Up

- The user reviewed the findings and had no questions or corrections. Next step: a change file to do the publishing.
