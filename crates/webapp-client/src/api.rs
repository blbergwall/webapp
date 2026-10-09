//! JSON fetch helpers for the app's own `/api/*` routes, built directly on
//! the browser's `fetch`.

use std::{cell::RefCell, fmt};

use serde::{Serialize, de::DeserializeOwned};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{Request, RequestCredentials, RequestInit, Response};

thread_local! {
    /// Session token in Discord activity mode (sent as `Authorization:
    /// Bearer`). `None` in website mode, which uses the session cookie.
    static TOKEN: RefCell<Option<String>> = const { RefCell::new(None) };
}

pub(crate) fn set_token(token: Option<String>) {
    TOKEN.with_borrow_mut(|slot| *slot = token);
}

#[derive(Debug, Clone, PartialEq)]
pub enum FetchError {
    /// The server answered with a non-2xx status.
    Status(u16),
    /// The request never completed, or the response wasn't the expected JSON.
    Network(String),
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Status(status) => write!(f, "server returned {status}"),
            Self::Network(message) => write!(f, "network error: {message}"),
        }
    }
}

/// `GET path`, parsing the JSON response.
pub async fn api_get<T: DeserializeOwned>(path: &str) -> Result<T, FetchError> {
    request("GET", path, None).await
}

/// `POST path` with a JSON body, parsing the JSON response. An empty (e.g.
/// 204) response parses as `null`, so `T = ()` works for endpoints without a
/// body. Always sending JSON also satisfies the server's CSRF rule.
pub async fn api_post<B: Serialize + ?Sized, T: DeserializeOwned>(
    path: &str,
    body: &B,
) -> Result<T, FetchError> {
    let json = serde_json::to_string(body).map_err(|err| FetchError::Network(err.to_string()))?;
    request("POST", path, Some(json)).await
}

async fn request<T: DeserializeOwned>(
    method: &str,
    path: &str,
    body: Option<String>,
) -> Result<T, FetchError> {
    let init = RequestInit::new();
    init.set_method(method);
    init.set_credentials(RequestCredentials::SameOrigin);
    if let Some(body) = &body {
        init.set_body(&JsValue::from_str(body));
    }
    let request = Request::new_with_str_and_init(path, &init).map_err(js_error)?;
    let headers = request.headers();
    if body.is_some() {
        headers.set("Content-Type", "application/json").map_err(js_error)?;
    }
    if let Some(token) = TOKEN.with_borrow(Clone::clone) {
        headers
            .set("Authorization", &format!("Bearer {token}"))
            .map_err(js_error)?;
    }

    let window = web_sys::window().ok_or_else(|| FetchError::Network("no window".to_string()))?;
    let response: Response = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(js_error)?
        .dyn_into()
        .map_err(js_error)?;
    if !response.ok() {
        return Err(FetchError::Status(response.status()));
    }

    let text = JsFuture::from(response.text().map_err(js_error)?)
        .await
        .map_err(js_error)?
        .as_string()
        .unwrap_or_default();
    let text = if text.trim().is_empty() { "null" } else { text.as_str() };
    serde_json::from_str(text).map_err(|err| FetchError::Network(format!("unexpected response: {err}")))
}

fn js_error(value: JsValue) -> FetchError {
    FetchError::Network(format!("{value:?}"))
}
