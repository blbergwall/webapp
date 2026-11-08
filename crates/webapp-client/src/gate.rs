//! `<AuthGate>`: sign-in UI for website mode and (with the
//! `discord-activity` feature) Discord activity mode, rendering its children
//! once signed in.
//!
//! The views are plain semantic HTML with `webapp-*` classes and no built-in
//! styles; apps style them.

use leptos::{ev::SubmitEvent, prelude::*, task::spawn_local};
use webapp_common::{
    Account, AuthConfig, EmailConfirmRequest, EmailStartRequest, OAuthConfirmRequest,
    OAuthStartResponse,
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
/// `?login_code=` the emailed link opens with), plus "Sign in with Discord"
/// and "Sign in with Google" when the server offers them. With the `discord-activity`
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
    // Website mode: which provider sign-ins the server offers (from
    // `/api/auth/config`), to show their buttons next to the email form.
    let offered = RwSignal::new(Offered::default());

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
        spawn_local(load_offered(offered));
        spawn_local(website_start(stage, sign_in));
    }

    move || match stage.get() {
        Stage::Busy(message) => view! { <p class="webapp-status">{message}</p> }.into_any(),
        Stage::EmailForm(notice) => view! { <EmailForm stage notice offered /> }.into_any(),
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

/// A website-mode sign-in with another provider (OAuth redirect flow).
#[derive(Clone, Copy, PartialEq)]
enum Provider {
    Discord,
    Google,
}

impl Provider {
    const ALL: [Provider; 2] = [Provider::Discord, Provider::Google];

    fn name(self) -> &'static str {
        match self {
            Provider::Discord => "Discord",
            Provider::Google => "Google",
        }
    }

    fn start_path(self) -> &'static str {
        match self {
            Provider::Discord => "/api/auth/discord/web/start",
            Provider::Google => "/api/auth/google/start",
        }
    }

    fn confirm_path(self) -> &'static str {
        match self {
            Provider::Discord => "/api/auth/discord/web/confirm",
            Provider::Google => "/api/auth/google/confirm",
        }
    }

    fn button_class(self) -> &'static str {
        match self {
            Provider::Discord => "webapp-discord",
            Provider::Google => "webapp-google",
        }
    }

    fn busy_message(self) -> &'static str {
        match self {
            Provider::Discord => "Signing in with Discord…",
            Provider::Google => "Signing in with Google…",
        }
    }

    /// The provider a returned `state` belongs to: the server prefixes it
    /// with the provider's name (`discord.…`, `google.…`).
    fn from_state(state: &str) -> Option<Provider> {
        match state.split_once('.')?.0 {
            "discord" => Some(Provider::Discord),
            "google" => Some(Provider::Google),
            _ => None,
        }
    }
}

/// Which provider sign-ins the server offers.
#[derive(Clone, Copy, Default, PartialEq)]
struct Offered {
    discord: bool,
    google: bool,
}

impl Offered {
    fn includes(self, provider: Provider) -> bool {
        match provider {
            Provider::Discord => self.discord,
            Provider::Google => self.google,
        }
    }
}

/// The website sign-in form: email magic link, plus a "Sign in with Discord"
/// / "Sign in with Google" button for each provider the server offers.
#[component]
fn EmailForm(stage: RwSignal<Stage>, notice: Option<String>, offered: RwSignal<Offered>) -> impl IntoView {
    let email = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(notice);

    let start_provider = move |provider: Provider| {
        busy.set(true);
        spawn_local(async move {
            let started =
                api_post::<_, OAuthStartResponse>(provider.start_path(), &serde_json::json!({})).await;
            let navigated = match started {
                Ok(response) => web_sys::window()
                    .ok_or_else(|| "no window".to_string())
                    .and_then(|window| {
                        window.location().assign(&response.url).map_err(|err| format!("{err:?}"))
                    }),
                Err(err) => Err(err.to_string()),
            };
            // On success the page is leaving for the provider, so stay busy.
            if let Err(message) = navigated {
                let name = provider.name();
                error.try_set(Some(format!("Couldn't start {name} sign-in ({message}).")));
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
                let offered = offered.get();
                let providers: Vec<Provider> = Provider::ALL
                    .into_iter()
                    .filter(|provider| offered.includes(*provider))
                    .collect();
                // Grouped apart from the email field, so apps can separate them.
                (!providers.is_empty()).then(|| view! {
                    <div class="webapp-providers">
                        {providers
                            .into_iter()
                            .map(|provider| view! {
                                <button
                                    type="button"
                                    class=provider.button_class()
                                    on:click=move |_| start_provider(provider)
                                    disabled=move || busy.get()
                                >
                                    {format!("Sign in with {}", provider.name())}
                                </button>
                            })
                            .collect_view()}
                    </div>
                })
            }}
            {move || error.get().map(|message| view! { <p class="webapp-error">{message}</p> })}
        </form>
    }
}

/// Website mode: asks the server which provider sign-ins it offers. On
/// failure the buttons just stay hidden; the email form still works.
async fn load_offered(offered: RwSignal<Offered>) {
    if let Ok(config) = api_get::<AuthConfig>("/api/auth/config").await {
        offered.try_set(Offered {
            discord: config.discord_client_id.is_some(),
            google: config.google_sign_in,
        });
    }
}

/// Website mode on page load: finish a provider sign-in or a `?login_code=`
/// link if the URL carries one, otherwise check for an existing session
/// cookie.
async fn website_start(stage: RwSignal<Stage>, sign_in: impl Fn(Account) + Copy + 'static) {
    match take_provider_return() {
        Some((provider, ProviderReturn::Code(request))) => {
            confirm_provider(stage, sign_in, provider, request).await;
            return;
        }
        Some((provider, ProviderReturn::Error(error))) => {
            let name = provider.name();
            let message = if error == "access_denied" {
                format!("{name} sign-in was cancelled.")
            } else {
                format!("{name} sign-in failed. Please try again.")
            };
            stage.set(Stage::EmailForm(Some(message)));
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

async fn confirm_provider(
    stage: RwSignal<Stage>,
    sign_in: impl Fn(Account) + Copy + 'static,
    provider: Provider,
    request: OAuthConfirmRequest,
) {
    stage.set(Stage::Busy(provider.busy_message()));
    let name = provider.name();
    let message = match api_post::<_, Account>(provider.confirm_path(), &request).await {
        Ok(account) => return sign_in(account),
        Err(FetchError::Status(400)) => {
            format!("That {name} sign-in expired or was started in another browser. Please try again.")
        }
        Err(FetchError::Status(401)) => format!("{name} didn't accept that sign-in. Please try again."),
        Err(err) => format!("Couldn't sign in with {name} ({err})."),
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

/// What a provider sent the browser back to `/` with after a sign-in.
enum ProviderReturn {
    /// `?code=…&state=…`: confirm with the server.
    Code(OAuthConfirmRequest),
    /// `?error=…&state=…`, e.g. `access_denied` when the user cancelled.
    Error(String),
}

/// Reads a provider sign-in return from the URL (a provider-prefixed `state`
/// plus either `code` or `error`) and removes the query string, so a reload
/// doesn't replay it.
fn take_provider_return() -> Option<(Provider, ProviderReturn)> {
    let params = query_params()?;
    let state = params.get("state")?;
    let provider = Provider::from_state(&state)?;
    let found = match (params.get("code"), params.get("error")) {
        (Some(code), _) => ProviderReturn::Code(OAuthConfirmRequest { code, state }),
        (None, Some(error)) => ProviderReturn::Error(error),
        (None, None) => return None,
    };
    clear_query();
    Some((provider, found))
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
