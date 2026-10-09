use std::net::SocketAddr;

use axum::{Router, routing::get};
use webapp_server::{Auth, WebApp};

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    let webapp = WebApp::from_env().unwrap_or_else(|err| panic!("config error: {err:#}"));
    let addr = std::env::var("BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:3000".to_string());

    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/api/hello", get(hello))
        .merge(webapp_server::auth_router())
        .with_state(webapp);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|err| panic!("failed to bind {addr}: {err}"));
    println!("Listening on http://{addr}");
    // Connect info gives the email rate limit a real client IP in local dev
    // (on Fly.io it uses the `Fly-Client-IP` header instead).
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .await
        .expect("server error");
}

/// Example protected route: only reachable with a valid session.
async fn hello(Auth(account): Auth) -> String {
    format!("Hello, {}!", account.display_name)
}
