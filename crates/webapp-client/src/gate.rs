//! `<AuthGate>`: sign-in UI for website mode and (with the
//! `discord-activity` feature) Discord activity mode, rendering its children
//! once signed in.
//!
//! The views are plain semantic HTML with `webapp-*` classes and no built-in
//! styles; apps style them.

use leptos::{ev::SubmitEvent, prelude::*, task::spawn_local};
use webapp_common::{Account, EmailConfirmRequest, EmailStartRequest};

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
        spawn_local(website_start(stage, sign_in));
    }

    move || match stage.get() {
        Stage::Busy(message) => view! { <p class="webapp-status">{message}</p> }.into_any(),
        Stage::EmailForm(notice) => view! { <EmailForm stage notice /> }.into_any(),
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

#[component]
fn EmailForm(stage: RwSignal<Stage>, notice: Option<String>) -> impl IntoView {
    let email = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(notice);

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
            {move || error.get().map(|message| view! { <p class="webapp-error">{message}</p> })}
        </form>
    }
}

/// Website mode on page load: complete a `?login_code=` link if present,
/// otherwise check for an existing session cookie.
async fn website_start(stage: RwSignal<Stage>, sign_in: impl Fn(Account) + Copy + 'static) {
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

#[cfg(feature = "discord-activity")]
async fn activity_sign_in(stage: RwSignal<Stage>, sign_in: impl Fn(Account) + Copy + 'static) {
    stage.set(Stage::Busy("Signing in with Discord…"));
    match discord::sign_in().await {
        Ok(account) => sign_in(account),
        Err(message) => stage.set(Stage::ActivitySignedOut(Some(message))),
    }
}

/// Reads `login_code` from the URL and removes the query string, so a reload
/// or a shared URL doesn't retry a consumed code.
fn take_login_code() -> Option<String> {
    let window = web_sys::window()?;
    let search = window.location().search().ok()?;
    let code = web_sys::UrlSearchParams::new_with_str(&search)
        .ok()?
        .get("login_code")?;
    let path = window.location().pathname().unwrap_or_else(|_| "/".to_string());
    let _ = window
        .history()
        .and_then(|history| history.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&path)));
    Some(code)
}
