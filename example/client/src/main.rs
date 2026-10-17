use leptos::{ev::SubmitEvent, prelude::*};
use webapp_client::{AuthGate, WsStatus, api_get, connect_ws, use_auth};

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
        <Echo />
    }
}

/// WebSocket demo: connects to the example's `/api/ws` echo route and lists
/// what it sends back.
#[component]
fn Echo() -> impl IntoView {
    let received = RwSignal::new(Vec::<String>::new());
    let ws = connect_ws("/api/ws", move |text| received.update(|lines| lines.push(text)));
    let draft = RwSignal::new(String::new());

    let submit = move |event: SubmitEvent| {
        event.prevent_default();
        if ws.send(&draft.get_untracked()) {
            draft.set(String::new());
        }
    };

    view! {
        <section class="echo">
            <h2>"WebSocket echo"</h2>
            <p class="ws-status">
                {move || match ws.status.get() {
                    WsStatus::Connecting => "Connecting…",
                    WsStatus::Open => "Connected",
                    WsStatus::Closed => "Closed",
                }}
            </p>
            <form on:submit=submit>
                <input placeholder="Message" bind:value=draft />
                <button type="submit" disabled=move || ws.status.get() != WsStatus::Open>
                    "Send"
                </button>
            </form>
            <ul>{move || received.get().into_iter().map(|line| view! { <li>{line}</li> }).collect_view()}</ul>
        </section>
    }
}

fn main() {
    mount_to_body(App);
}
