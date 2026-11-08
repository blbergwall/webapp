//! Discord activity mode (`discord-activity` feature): the Embedded App SDK,
//! via the bundled `js/discord.js` (a wasm-bindgen snippet: copied into
//! `dist/snippets/` and imported with a relative path, so apps get it through
//! this crate with no setup), and activity sign-in.

use std::cell::RefCell;

use wasm_bindgen::prelude::*;
use web_sys::UrlSearchParams;
use webapp_common::{Account, AuthConfig, DiscordLoginRequest, DiscordLoginResponse};

use crate::api::{api_get, api_post, set_token};

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

/// Signs in with Discord: the SDK's `authorize` code, exchanged by the server
/// for a bearer session. Keeps both tokens in memory and returns the account,
/// or a message for the sign-in view.
pub(crate) async fn sign_in() -> Result<Account, String> {
    let config: AuthConfig = api_get("/api/auth/config")
        .await
        .map_err(|err| format!("Couldn't reach the server ({err})."))?;
    let client_id = config
        .discord_client_id
        .ok_or("Discord sign-in isn't configured on this server.")?;
    let code = authorize(&client_id)
        .await
        .map_err(|err| format!("Discord sign-in was cancelled or failed ({err})."))?;
    let response: DiscordLoginResponse = api_post("/api/auth/discord", &DiscordLoginRequest { code })
        .await
        .map_err(|err| format!("Sign-in failed ({err})."))?;
    set_token(Some(response.token));
    ACCESS_TOKEN.with_borrow_mut(|slot| *slot = Some(response.discord_access_token));
    Ok(response.account)
}

/// Forgets both tokens (local half of signing out).
pub(crate) fn sign_out() {
    set_token(None);
    ACCESS_TOKEN.with_borrow_mut(|slot| *slot = None);
}

/// Runs the SDK handshake and `authorize` (`identify` scope), returning the
/// OAuth2 code for the server to exchange.
async fn authorize(client_id: &str) -> Result<String, String> {
    let code = discord_authorize(client_id).await.map_err(describe)?;
    code.as_string()
        .ok_or_else(|| "Discord returned no authorization code".to_string())
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
