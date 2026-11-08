//! Client-side (wasm32) auth framework: the Leptos `<AuthGate>` component,
//! auth API calls and, optionally, Discord activity mode and WebSockets.
//!
//! Wrap the app's signed-in UI in `<AuthGate>`, read the account with
//! [`use_auth`], and call the app's own endpoints with [`api_get`] /
//! [`api_post`].
//!
//! Optional Cargo features (both off by default):
//! - `discord-activity`: Discord activity mode. `<AuthGate>` signs in with
//!   Discord when launched as an activity, using the bundled Embedded App
//!   SDK, and the API helpers attach the resulting bearer token.
//! - `ws`: [`connect_ws`] opens an authenticated WebSocket.

mod api;
#[cfg(feature = "discord-activity")]
mod discord;
mod gate;
#[cfg(feature = "ws")]
mod ws;

pub use api::{FetchError, api_get, api_post};
#[cfg(feature = "discord-activity")]
pub use discord::{authenticate_discord_sdk, is_discord_activity};
pub use gate::{AuthContext, AuthGate, use_auth};
pub use webapp_common;
#[cfg(feature = "ws")]
pub use ws::{WsConnection, WsStatus, connect_ws};
