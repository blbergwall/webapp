# webapp

A small framework for Rust web apps that run either as a **plain website** or as a **Discord activity**, with sign-in handled for both:

- **Website:** passwordless email (magic-link) login, with the session in an HttpOnly cookie.
- **Discord activity:** Discord Embedded App SDK sign-in with a server-side code exchange, and a bearer token held in memory.

The server is `axum` with accounts and sessions in Upstash Redis (REST API), and email goes through Resend. The client is Leptos compiled to WASM with `trunk`. There's also optional authenticated WebSocket support (the `ws` feature). One server serves the client and `/api/*` from a single origin, deployed to Fly.io.

## Crates

| Crate | |
|---|---|
| `webapp-server` | axum auth router, `Auth` extractor, Upstash, Resend, Discord exchange |
| `webapp-client` | wasm32: `<AuthGate>`, Discord SDK bridge, API and WebSocket helpers |
| `webapp-common` | shared API types |

`example/` is a complete app (server and client) to copy from.

## Using it

Depend on the crates by git tag:

```toml
webapp-server = { git = "https://github.com/blbergwall/webapp", tag = "v0.1.0" }
webapp-client = { git = "https://github.com/blbergwall/webapp", tag = "v0.1.0" }
```

Read [`docs/implementation-guide.md`](docs/implementation-guide.md) for wiring the server and client, the conventions apps follow, and the Redis key rules. Deployment is in [`docs/deploy.md`](docs/deploy.md).

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT), at your option. The bundled Discord Embedded App SDK and its libraries are under their own licenses: see [`crates/webapp-client/js/THIRD-PARTY-NOTICES`](crates/webapp-client/js/THIRD-PARTY-NOTICES).
