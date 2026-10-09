use std::net::SocketAddr;

use axum::{Json, Router, routing::get};
use tower_http::services::ServeDir;
use webapp_server::{Auth, WebApp};

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    let webapp = WebApp::from_env().unwrap_or_else(|err| panic!("config error: {err:#}"));
    let addr = std::env::var("BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:3000".to_string());
    // The trunk-built client. In dev, `trunk serve` serves it instead.
    let static_dir =
        std::env::var("STATIC_DIR").unwrap_or_else(|_| "build/dist".to_string());

    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/api/hello", get(hello))
        .merge(webapp_server::auth_router())
        .fallback_service(ServeDir::new(&static_dir))
        .with_state(webapp);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|err| panic!("failed to bind {addr}: {err}"));
    println!("Listening on http://{addr} (static files from {static_dir})");
    // Connect info gives the email rate limit a real client IP in local dev
    // (on Fly.io it uses the `Fly-Client-IP` header instead).
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .await
        .expect("server error");
}

/// Example protected route: only reachable with a valid session.
async fn hello(Auth(account): Auth) -> Json<String> {
    Json(format!("Hello, {}!", account.display_name))
}
