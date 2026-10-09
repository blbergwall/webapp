//! Discord Embedded App SDK access, via the bundled `js/discord.js` (a
//! wasm-bindgen snippet: copied into `dist/snippets/` and imported with a
//! relative path, so apps get it through this crate with no setup).

use std::cell::RefCell;

use wasm_bindgen::prelude::*;
use web_sys::UrlSearchParams;

#[wasm_bindgen(module = "/js/discord.js")]
extern "C" {
    #[wasm_bindgen(catch, js_name = discordAuthorize)]
    async fn discord_authorize(client_id: &str) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(catch, js_name = discordAuthenticate)]
    async fn discord_authenticate(access_token: &str) -> Result<JsValue, JsValue>;
}

thread_local! {
    /// Discord OAuth2 access token from the last sign-in, for
    /// [`authenticate_discord_sdk`].
    static ACCESS_TOKEN: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Whether the page is running as a Discord activity. Discord launches
/// activities with `frame_id`, `instance_id` and `platform` query params.
pub fn is_discord_activity() -> bool {
    let Some(window) = web_sys::window() else {
        return false;
    };
    let search = window.location().search().unwrap_or_default();
    UrlSearchParams::new_with_str(&search).is_ok_and(|params| params.has("frame_id"))
}

/// Runs the SDK handshake and `authorize` (`identify` scope), returning the
/// OAuth2 code for the server to exchange.
pub(crate) async fn authorize(client_id: &str) -> Result<String, String> {
    let code = discord_authorize(client_id).await.map_err(describe)?;
    code.as_string()
        .ok_or_else(|| "Discord returned no authorization code".to_string())
}

pub(crate) fn set_access_token(token: Option<String>) {
    ACCESS_TOKEN.with_borrow_mut(|slot| *slot = token);
}

/// Authenticates the Discord SDK with the signed-in user's access token. Only
/// needed by apps that use SDK commands beyond sign-in; call it after
/// `AuthGate` has signed in.
pub async fn authenticate_discord_sdk() -> Result<(), String> {
    let token = ACCESS_TOKEN
        .with_borrow(Clone::clone)
        .ok_or_else(|| "not signed in with Discord".to_string())?;
    discord_authenticate(&token).await.map_err(describe)?;
    Ok(())
}

/// A readable message from a rejected JS promise. The SDK rejects with plain
/// `{code, message}` objects as well as `Error`s, so read `message` either way.
fn describe(error: JsValue) -> String {
    js_sys::Reflect::get(&error, &"message".into())
        .ok()
        .and_then(|message| message.as_string())
        .or_else(|| error.as_string())
        .unwrap_or_else(|| format!("{error:?}"))
}
