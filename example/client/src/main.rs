use leptos::prelude::*;

#[component]
fn App() -> impl IntoView {
    view! { <p>"webapp example: placeholder"</p> }
}

fn main() {
    mount_to_body(App);
}
