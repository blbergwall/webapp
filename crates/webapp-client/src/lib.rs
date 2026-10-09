//! Client-side (wasm32) auth framework: mode detection, Discord SDK bridge,
//! auth API calls and the Leptos `<AuthGate>` component.
//!
//! Wrap the app's signed-in UI in `<AuthGate>`, read the account with
//! [`use_auth`], and call the app's own endpoints with [`api_get`] /
//! [`api_post`] (which attach the Discord-mode bearer token automatically).

mod api;
mod discord;
mod gate;

pub use api::{FetchError, api_get, api_post};
pub use discord::{authenticate_discord_sdk, is_discord_activity};
pub use gate::{AuthContext, AuthGate, use_auth};
pub use webapp_common;
