//! Client-side (wasm32) auth framework: mode detection, Discord SDK bridge,
//! auth API calls and the Leptos `<AuthGate>` component.
//!
//! Wrap the app's signed-in UI in `<AuthGate>`, read the account with
//! [`use_auth`], and call the app's own endpoints with [`api_get`] /
//! [`api_post`] (which attach the Discord-mode bearer token automatically).
//! With the `ws` feature, [`connect_ws`] opens an authenticated WebSocket.

mod api;
mod discord;
mod gate;
#[cfg(feature = "ws")]
mod ws;

pub use api::{FetchError, api_get, api_post};
pub use discord::{authenticate_discord_sdk, is_discord_activity};
pub use gate::{AuthContext, AuthGate, use_auth};
pub use webapp_common;
#[cfg(feature = "ws")]
pub use ws::{WsConnection, WsStatus, connect_ws};
