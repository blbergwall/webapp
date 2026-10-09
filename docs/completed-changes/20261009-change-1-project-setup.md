# Change 1: Project Setup

**Status:** done

Order: 1 of 6. Architecture source: `docs/completed-research/20261009-web-app-framework.md`.

Set up this repo as the home of the reusable web app framework:

- Initialize a git repo and add a `.gitignore` (`target/`, `dist/`, `.env*`, editor files).
- Create the Cargo workspace skeleton described in the research:
  - `crates/webapp-common`, `crates/webapp-server`, `crates/webapp-client` as library crates.
  - `example/server` (bin) and `example/client` (Leptos/trunk app).
  - `default-members` excludes the wasm32-only crates (`webapp-client`, `example/client`) so `cargo build` / `cargo test` at the root work, as in `full-rust`.
- Create `CLAUDE.md` covering:
  - project purpose and the reference projects (`neverending-contrivance`, `full-rust`)
  - the `/research` + `/change` development process
  - design principles: minimize dependencies and abstractions
  - tech stack, crate layout, and build/run commands
  - Discord activity quirks: `no_sri`, `no_autoreload`, the vendored SDK, bearer token instead of cookies
- Placeholder code only: each crate compiles, and the example server answers `GET /health`.

## Clarifications

Nothing in the description was ambiguous, and the change is one coherent deliverable, so it isn't split. The minor choices made while planning:

- **Rust edition:** 2024 (toolchain is 1.94.1).
- **Package names:** `webapp-common`, `webapp-server`, `webapp-client`, `example-server`, `example-client`.
- **Placeholder dependencies:** the library crates get no dependencies yet; each later change adds what it needs. Only the example crates need dependencies now: `axum` + `tokio` for the server and `leptos` (`csr`) for the client.
- **Local ports:** the example server binds `BIND_ADDR`, defaulting to `127.0.0.1:3000`. `trunk serve` runs on `:8080` and proxies `/api` and `/health` to `:3000`, as in `full-rust`. Fly will set `BIND_ADDR=0.0.0.0:8080` in change 6.

## Implementation Plan

**Files to add:**
- `.gitignore`: `target/`, `dist/`, `.env*`, editor/OS files.
- `Cargo.toml`: workspace with all five members; `default-members` limited to the three host crates; `[workspace.package]` sets edition 2024; `[workspace.dependencies]` pins shared versions.
- `crates/webapp-common/{Cargo.toml,src/lib.rs}`: empty library with a crate doc comment.
- `crates/webapp-server/{Cargo.toml,src/lib.rs}`: empty library with a crate doc comment.
- `crates/webapp-client/{Cargo.toml,src/lib.rs}`: empty library with a crate doc comment.
- `example/server/{Cargo.toml,src/main.rs}`: axum app with `GET /health` returning `ok`.
- `example/client/{Cargo.toml,src/main.rs,index.html,Trunk.toml}`: Leptos `csr` app that mounts a placeholder page. `Trunk.toml` sets `dist = "dist"`, `no_sri = true`, `no_autoreload = true`, and proxies `/api` and `/health` to `:3000`.
- `CLAUDE.md`: project guidance as listed in the description.

**Approach:**
1. `git init`.
2. Write the files above.
3. Verify with `cargo build` at the root, `cargo check -p example-client --target wasm32-unknown-unknown`, `trunk build` in `example/client`, and a run of the example server to hit `/health`.

**Risks / things to watch for:**
- The repo lives in Dropbox, so `target/` and `dist/` churn will sync unless they're excluded in Dropbox. Mention this to the user; it's outside the repo's control.
- Trunk on Windows sometimes fails to delete its staging dir (a known `full-rust` gotcha). Retrying usually works.

## User Feedback

- **What is `trunk serve` doing as opposed to `cargo run -p example-server`? Is `trunk serve` for the Discord integration?**
  **Decision:** No. `trunk serve` is the frontend dev server (compiles the client to WASM, serves it on :8080, rebuilds on change, and proxies `/api` to the axum backend on :3000). The two-process dev setup is kept. `CLAUDE.md` Build & Run now explains each process, the production setup, and where Discord points.
