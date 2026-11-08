//! `<AuthGate>`: sign-in UI for website mode and (with the
//! `discord-activity` feature) Discord activity mode, rendering its children
//! once signed in.
//!
//! The views are plain semantic HTML with `webapp-*` classes and no built-in
//! styles; apps style them.

use leptos::{ev::SubmitEvent, prelude::*, task::spawn_local};
use webapp_common::{
    Account, AuthConfig, DiscordWebConfirmRequest, DiscordWebStartResponse, EmailConfirmRequest,
    EmailStartRequest,
};

use crate::api::{FetchError, api_get, api_post};
#[cfg(feature = "discord-activity")]
use crate::discord;

/// Available to everything inside `<AuthGate>` via [`use_auth`].
#[derive(Clone, Copy)]
pub struct AuthContext {
    /// The signed-in account.
    pub account: Signal<Account>,
    /// Signs out (server session and local state) and returns to the
    /// sign-in view.
    pub logout: Callback<()>,
}

/// The [`AuthContext`] provided by the enclosing `<AuthGate>`.
///
/// Panics if called outside `<AuthGate>`'s children.
pub fn use_auth() -> AuthContext {
    use_context().expect("use_auth() must be called inside <AuthGate>")
}

#[derive(Clone, PartialEq)]
enum Stage {
    /// Working; shows a status message.
    Busy(&'static str),
    /// Website mode: ask for an email address, optionally with a notice.
    EmailForm(Option<String>),
    /// Website mode: link sent to this address.
    CheckEmail(String),
    /// Website mode: the link was opened in a different browser than the one
    /// that requested it, so ask before consuming the code.
    FinishSignIn(String),
    /// Activity mode: not signed in, optionally after an error.
    #[cfg(feature = "discord-activity")]
    ActivitySignedOut(Option<String>),
    SignedIn,
}

/// Shows sign-in UI until the user is signed in, then renders `children`
/// with an [`AuthContext`] available through [`use_auth`].
///
/// On a plain website it uses email magic links (including handling the
/// `?login_code=` the emailed link opens with). With the `discord-activity`
/// feature, inside a Discord activity it signs in with Discord automatically
/// instead.
#[component]
pub fn AuthGate(children: ChildrenFn) -> impl IntoView {
    #[cfg(feature = "discord-activity")]
    let in_activity = discord::is_discord_activity();
    #[cfg(not(feature = "discord-activity"))]
    let in_activity = false;
    let stage = RwSignal::new(Stage::Busy("Loading…"));
    // Keeps the last signed-in account after logout, so `AuthContext.account`
    // never has to be optional for code that briefly outlives a sign-out.
    let account = RwSignal::new(None::<Account>);
    // Website mode: whether the server offers Discord web sign-in (from
    // `/api/auth/config`), to show the button next to the email form.
    let discord_web = RwSignal::new(false);

    let sign_in = move |signed_in: Account| {
        account.set(Some(signed_in));
        stage.set(Stage::SignedIn);
    };

    let logout = Callback::new(move |()| {
        spawn_local(async move {
            // Best effort: the local sign-out happens regardless.
            let _ = api_post::<_, ()>("/api/auth/logout", &serde_json::json!({})).await;
            if in_activity {
                #[cfg(feature = "discord-activity")]
                {
                    discord::sign_out();
                    stage.set(Stage::ActivitySignedOut(None));
                }
            } else {
                stage.set(Stage::EmailForm(None));
            }
        });
    });

    provide_context(AuthContext {
        account: Signal::derive(move || {
            account
                .get()
                .expect("AuthContext.account read before sign-in")
        }),
        logout,
    });

    if in_activity {
        #[cfg(feature = "discord-activity")]
        spawn_local(activity_sign_in(stage, sign_in));
    } else {
        spawn_local(load_discord_web(discord_web));
        spawn_local(website_start(stage, sign_in));
    }

    move || match stage.get() {
        Stage::Busy(message) => view! { <p class="webapp-status">{message}</p> }.into_any(),
        Stage::EmailForm(notice) => view! { <EmailForm stage notice discord_web /> }.into_any(),
        Stage::CheckEmail(email) => view! {
            <div class="webapp-login">
                <p>"We've sent a sign-in link to " <strong>{email}</strong> ". Open it to finish signing in."</p>
                <button on:click=move |_| stage.set(Stage::EmailForm(None))>"Use a different email"</button>
            </div>
        }
        .into_any(),
        Stage::FinishSignIn(code) => {
            let code = StoredValue::new(code);
            view! {
                <div class="webapp-login">
                    <p>"Finish signing in on this device?"</p>
                    <button on:click=move |_| {
                        spawn_local(confirm_email(stage, sign_in, code.get_value(), false))
                    }>"Finish signing in"</button>
                </div>
            }
            .into_any()
        }
        #[cfg(feature = "discord-activity")]
        Stage::ActivitySignedOut(error) => view! {
            <div class="webapp-login">
                {error.map(|message| view! { <p class="webapp-error">{message}</p> })}
                <button on:click=move |_| spawn_local(activity_sign_in(stage, sign_in))>
                    "Sign in with Discord"
                </button>
            </div>
        }
        .into_any(),
        Stage::SignedIn => children().into_any(),
    }
}

/// The website sign-in form: email magic link, plus a "Sign in with Discord"
/// button when the server offers Discord web sign-in.
#[component]
fn EmailForm(
    stage: RwSignal<Stage>,
    notice: Option<String>,
    discord_web: RwSignal<bool>,
) -> impl IntoView {
    let email = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(notice);

    let start_discord = move |_| {
        busy.set(true);
        spawn_local(async move {
            let started = api_post::<_, DiscordWebStartResponse>(
                "/api/auth/discord/web/start",
                &serde_json::json!({}),
            )
            .await;
            let navigated = match started {
                Ok(response) => web_sys::window()
                    .ok_or_else(|| "no window".to_string())
                    .and_then(|window| {
                        window.location().assign(&response.url).map_err(|err| format!("{err:?}"))
                    }),
                Err(err) => Err(err.to_string()),
            };
            // On success the page is leaving for Discord, so stay busy.
            if let Err(message) = navigated {
                error.try_set(Some(format!("Couldn't start Discord sign-in ({message}).")));
                busy.try_set(false);
            }
        });
    };

    let submit = move |event: SubmitEvent| {
        event.prevent_default();
        let address = email.get_untracked().trim().to_string();
        busy.set(true);
        spawn_local(async move {
            let request = EmailStartRequest {
                email: address.clone(),
            };
            match api_post::<_, ()>("/api/auth/email/start", &request).await {
                Ok(()) => stage.set(Stage::CheckEmail(address)),
                Err(err) => {
                    error.try_set(Some(match err {
                        FetchError::Status(400) => {
                            "That doesn't look like an email address.".to_string()
                        }
                        FetchError::Status(429) => {
                            "Please wait a couple of minutes before requesting another link."
                                .to_string()
                        }
                        err => format!("Couldn't send the sign-in link ({err})."),
                    }));
                    busy.try_set(false);
                }
            }
        });
    };

    view! {
        <form class="webapp-login" on:submit=submit>
            <label for="webapp-email">"Email"</label>
            <input id="webapp-email" type="email" required autocomplete="email" bind:value=email />
            <button type="submit" disabled=move || busy.get()>"Send sign-in link"</button>
            {move || {
                discord_web.get().then(|| view! {
                    <button type="button" class="webapp-discord" on:click=start_discord disabled=move || busy.get()>
                        "Sign in with Discord"
                    </button>
                })
            }}
            {move || error.get().map(|message| view! { <p class="webapp-error">{message}</p> })}
        </form>
    }
}

/// Website mode: asks the server whether Discord web sign-in is available.
/// On failure the button just stays hidden; the email form still works.
async fn load_discord_web(discord_web: RwSignal<bool>) {
    if let Ok(config) = api_get::<AuthConfig>("/api/auth/config").await {
        discord_web.try_set(config.discord_client_id.is_some());
    }
}

/// Website mode on page load: finish a Discord web sign-in or a
/// `?login_code=` link if the URL carries one, otherwise check for an
/// existing session cookie.
async fn website_start(stage: RwSignal<Stage>, sign_in: impl Fn(Account) + Copy + 'static) {
    match take_discord_return() {
        Some(DiscordReturn::Code(request)) => {
            confirm_discord(stage, sign_in, request).await;
            return;
        }
        Some(DiscordReturn::Error(error)) => {
            let message = if error == "access_denied" {
                "Discord sign-in was cancelled."
            } else {
                "Discord sign-in failed. Please try again."
            };
            stage.set(Stage::EmailForm(Some(message.to_string())));
            return;
        }
        None => {}
    }
    if let Some(code) = take_login_code() {
        confirm_email(stage, sign_in, code, true).await;
        return;
    }
    match api_get::<Account>("/api/auth/me").await {
        Ok(account) => sign_in(account),
        Err(FetchError::Status(401)) => stage.set(Stage::EmailForm(None)),
        Err(err) => stage.set(Stage::EmailForm(Some(format!("Couldn't reach the server ({err})."))))
    }
}

async fn confirm_email(
    stage: RwSignal<Stage>,
    sign_in: impl Fn(Account) + Copy + 'static,
    code: String,
    auto: bool,
) {
    stage.set(Stage::Busy("Signing in…"));
    let request = EmailConfirmRequest {
        code: code.clone(),
        auto,
    };
    match api_post::<_, Account>("/api/auth/email/confirm", &request).await {
        Ok(account) => sign_in(account),
        Err(FetchError::Status(409)) => stage.set(Stage::FinishSignIn(code)),
        Err(FetchError::Status(410)) => stage.set(Stage::EmailForm(Some(
            "That sign-in link has expired or was already used. Request a new one below."
                .to_string(),
        ))),
        Err(err) => stage.set(Stage::EmailForm(Some(format!("Couldn't sign in ({err})."))))
    }
}

async fn confirm_discord(
    stage: RwSignal<Stage>,
    sign_in: impl Fn(Account) + Copy + 'static,
    request: DiscordWebConfirmRequest,
) {
    stage.set(Stage::Busy("Signing in with Discord…"));
    let message = match api_post::<_, Account>("/api/auth/discord/web/confirm", &request).await {
        Ok(account) => return sign_in(account),
        Err(FetchError::Status(400)) => {
            "That Discord sign-in expired or was started in another browser. Please try again."
                .to_string()
        }
        Err(FetchError::Status(401)) => {
            "Discord didn't accept that sign-in. Please try again.".to_string()
        }
        Err(err) => format!("Couldn't sign in with Discord ({err})."),
    };
    stage.set(Stage::EmailForm(Some(message)));
}

#[cfg(feature = "discord-activity")]
async fn activity_sign_in(stage: RwSignal<Stage>, sign_in: impl Fn(Account) + Copy + 'static) {
    stage.set(Stage::Busy("Signing in with Discord…"));
    match discord::sign_in().await {
        Ok(account) => sign_in(account),
        Err(message) => stage.set(Stage::ActivitySignedOut(Some(message))),
    }
}

/// What Discord sent the browser back to `/` with after a web sign-in.
enum DiscordReturn {
    /// `?code=…&state=…`: confirm with the server.
    Code(DiscordWebConfirmRequest),
    /// `?error=…&state=…`, e.g. `access_denied` when the user cancelled.
    Error(String),
}

/// Reads a Discord web sign-in return from the URL (a `state` plus either
/// `code` or `error`) and removes the query string, so a reload doesn't
/// replay it.
fn take_discord_return() -> Option<DiscordReturn> {
    let params = query_params()?;
    let state = params.get("state")?;
    let found = match (params.get("code"), params.get("error")) {
        (Some(code), _) => DiscordReturn::Code(DiscordWebConfirmRequest { code, state }),
        (None, Some(error)) => DiscordReturn::Error(error),
        (None, None) => return None,
    };
    clear_query();
    Some(found)
}

/// Reads `login_code` from the URL and removes the query string, so a reload
/// or a shared URL doesn't retry a consumed code.
fn take_login_code() -> Option<String> {
    let code = query_params()?.get("login_code")?;
    clear_query();
    Some(code)
}

fn query_params() -> Option<web_sys::UrlSearchParams> {
    let search = web_sys::window()?.location().search().ok()?;
    web_sys::UrlSearchParams::new_with_str(&search).ok()
}

/// Drops the query string from the address bar without reloading.
fn clear_query() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let path = window.location().pathname().unwrap_or_else(|_| "/".to_string());
    let _ = window
        .history()
        .and_then(|history| history.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&path)));
}
