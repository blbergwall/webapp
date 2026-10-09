use leptos::prelude::*;
use webapp_client::{AuthGate, api_get, use_auth};

#[component]
fn App() -> impl IntoView {
    view! {
        <main>
            <h1>"webapp example"</h1>
            <AuthGate>
                <Home />
            </AuthGate>
        </main>
    }
}

/// Signed-in page: calls the protected `/api/hello` route.
#[component]
fn Home() -> impl IntoView {
    let auth = use_auth();
    let greeting = LocalResource::new(|| api_get::<String>("/api/hello"));

    view! {
        <p class="greeting">
            {move || match greeting.get() {
                None => "Loading…".to_string(),
                Some(Ok(text)) => text,
                Some(Err(err)) => format!("Error: {err}"),
            }}
        </p>
        <p class="account">
            "Account " {move || auth.account.get().id}
            {move || auth.account.get().email.map(|email| format!(" · {email}"))}
            {move || auth.account.get().discord_id.map(|id| format!(" · Discord {id}"))}
        </p>
        <button on:click=move |_| auth.logout.run(())>"Sign out"</button>
    }
}

fn main() {
    mount_to_body(App);
}
