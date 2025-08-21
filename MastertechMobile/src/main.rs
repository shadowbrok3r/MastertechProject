// The dioxus prelude contains a ton of common items used in dioxus apps. It's a good idea to import wherever you
// need dioxus
use dioxus::prelude::*;
mod components { pub mod toast; pub mod dialog; pub mod navbar; }
mod theme;

mod pages { pub mod tasks; pub mod login; }
mod services { pub mod tasks; pub mod helpers; }
use database::init_database;
use serde::{Deserialize, Serialize};
use std::fs;


// We can import assets in dioxus with the `asset!` macro. This macro takes a path to an asset relative to the crate root.
// The macro returns an `Asset` type that will display as the path to the asset in the browser or a local path in desktop bundles.
const FAVICON: Asset = asset!("/assets/favicon.ico");
// The asset macro also minifies some assets like CSS and JS to make bundled smaller
const TAILWIND_CSS: Asset = asset!("/assets/tailwind.css");

#[derive(Serialize, Deserialize, Default)]
struct SavedSession { token: Option<String>, email: Option<String> }

fn session_file_path() -> Option<std::path::PathBuf> {
    dirs::data_local_dir().map(|p| p.join("mastertech_mobile").join("session.json"))
}

fn load_saved_session() -> Option<SavedSession> {
    let path = session_file_path()?;
    let data = fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
}

fn save_session(sess: &SavedSession) {
    if let Some(path) = session_file_path() {
        if let Some(parent) = path.parent() { let _ = fs::create_dir_all(parent); }
        if let Ok(json) = serde_json::to_string(sess) { let _ = fs::write(path, json); }
    }
}

fn main() {
    // The `launch` function is the main entry point for a dioxus app. It takes a component and renders it with the platform feature
    // you have enabled
        dioxus::launch(app);
}

/// App is the main component of our app. Components are the building blocks of dioxus apps. Each component is a function
/// that takes some props and returns an Element. In this case, App takes no props because it is the root of our app.
///
/// Components should be annotated with `#[component]` to support props, better error messages, and autocomplete
#[component]
fn app() -> Element {
    // initialize a guest connection so we can render login or tasks
    let _db = use_future(|| async move { let _ = init_database().await; });
    let authed = use_signal(|| false); // Signal to track authentication status
    let err = use_signal(|| Option::<String>::None);
    let toast = use_signal(|| crate::components::toast::ToastManager::default());
    use_context_provider(|| toast);
    let theme_sig = use_signal(|| theme::ThemeConfig::default());

    // Attempt token auto-login once
    {
        let mut authed_sig = authed.to_owned();
        let mut err_sig = err.to_owned();
        use_future(move || async move {
            if let Some(saved) = load_saved_session() {
                if let Some(tok) = saved.token.clone() {
                    match database::token_login(&tok).await {
                        Ok(sess) => {
                            // populate statics
                            if let Ok(mut g) = database::CURRENT_USER_INFO.try_lock() { *g = Some(sess.user.clone()); }
                            authed_sig.set(true);
                        }
                        Err(e) => err_sig.set(Some(format!("Auto-login failed: {e}"))),
                    }
                }
            }
        });
    }

    // Router now handles page navigation
    let on_login = {
        let mut authed_sig = authed.to_owned();
        let mut err_sig = err.to_owned();
        Callback::new(move |(ok, e): (bool, Option<String>)| {
            if let Some(e) = e { err_sig.set(Some(e)); }
            if ok { authed_sig.set(true); }
        })
    };
    let page = use_signal(|| "My Tasks".to_string());
    let refresh_nonce = use_signal(|| 0u64);
    let create_nonce = use_signal(|| 0u64);
    let show_prefs = use_signal(|| false);
    rsx! {
        // In addition to element and text (which we will see later), rsx can contain other components. In this case,
        // we are using the `document::Link` component to add a link to our favicon and main CSS file into the head of our app.
        document::Link { rel: "icon", href: FAVICON }
        document::Link { rel: "stylesheet", href: TAILWIND_CSS }
        // Inject theme CSS variables (skeleton)
        {
            let default_theme = theme_sig().to_css_vars();
            rsx! { theme::ThemeStyle { css: default_theme } }
        }
    crate::components::toast::ToastFrame { manager: toast }

        if !authed() {
            pages::login::LoginPage { on_login }
    } else {
            components::navbar::NavbarWithLogo {
                active: page(),
                on_tab: Callback::new({ let mut p = page.to_owned(); move |pname: String| p.set(pname) }),
                on_refresh: Callback::new({ let mut r = refresh_nonce.to_owned(); move |_| r.set(r()+1) }),
                on_new_task: Callback::new({ let mut c = create_nonce.to_owned(); move |_| c.set(c()+1) }),
                on_preferences: Callback::new({ let mut sp = show_prefs.to_owned(); move |_| sp.set(true) }),
            }
            pages::tasks::TaskBoard { page: page(), on_navigate: None, refresh_token: refresh_nonce(), create_task_trigger: create_nonce() }
            if show_prefs() { PreferencesModal { show: show_prefs, theme_sig: theme_sig } }
        }
        if let Some(e) = err() { div { class: "fixed bottom-3 right-3 bg-red-900/80 text-red-100 px-3 py-2 rounded shadow", {e} } }

    }
}

// No Router: desktop is for development only; mobile uses in-app tabs

#[derive(Props, PartialEq, Clone)]
struct PreferencesModalProps { show: Signal<bool>, theme_sig: Signal<theme::ThemeConfig> }

#[component]
fn PreferencesModal(props: PreferencesModalProps) -> Element {
    let mut input_json = use_signal(|| String::new());
    let mut feedback = use_signal(|| Option::<String>::None);
    rsx! {
        crate::components::dialog::Dialog { show_modal: props.show, wrap_class: Some("w-[96%] max-w-md".into()), close_button_label: Some("Close".into()),
            div { class: "space-y-4 text-sm text-slate-200",
                h2 { class: "text-lg font-semibold", "Preferences" }
                div { class: "space-y-1",
                    p { class: "opacity-70", "Import Theme JSON" }
                    textarea { class: "w-full h-32 bg-[#111216] rounded px-2 py-1 text-xs border border-[#2a2c5d]/60 font-mono", value: input_json(), oninput: move |e| input_json.set(e.value()) }
                    button { class: "px-3 py-1 rounded border border-[#2a2c5d]/60 hover:bg-[#1e1a2a]/50", onclick: move |_| {
                        let txt = input_json();
                        match serde_json::from_str::<theme::ThemeConfig>(&txt) {
                            Ok(cfg) => { let mut sig = props.theme_sig.to_owned(); theme::apply_theme_signal(&mut sig, cfg.clone()); feedback.set(Some("Applied".into())); },
                            Err(e) => feedback.set(Some(format!("Invalid JSON: {e}")))
                        }
                    }, "Apply" }
                    if let Some(msg) = feedback() { div { class: "text-xs opacity-70", {msg} } }
                }
                div { class: "text-xs opacity-60 pt-2", "Version: dev" }
            }
        }
    }
}
