use dioxus::prelude::*;
mod components;
mod theme;

mod pages { pub mod tasks; pub mod login; }
mod services { pub mod tasks; pub mod helpers; }
use database::init_database;
use serde::{Deserialize, Serialize};
use std::fs;

const FAVICON: Asset = asset!("/assets/favicon.ico");
const STYLES_CSS: Asset = asset!("/assets/styles.css");

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

fn clear_session() {
    if let Some(path) = session_file_path() { let _ = fs::remove_file(path); }
}

fn main() {
    dioxus::launch(app);
}

#[component]
fn app() -> Element {
    let _db = use_future(|| async move { let _ = init_database().await; });
    let mut authed = use_signal(|| false);
    let err = use_signal(|| Option::<String>::None);
    let toast = use_signal(|| crate::components::toast::ToastManager::default());
    use_context_provider(|| toast);

    // Auto-login
    {
        let mut authed_sig = authed.to_owned();
        let mut err_sig = err.to_owned();
        use_future(move || async move {
            if let Some(saved) = load_saved_session() {
                if let Some(tok) = saved.token.clone() {
                    match database::token_login(&tok).await {
                        Ok(sess) => {
                            if let Ok(mut g) = database::CURRENT_USER_INFO.try_lock() { *g = Some(sess.user.clone()); }
                            authed_sig.set(true);
                        }
                        Err(e) => err_sig.set(Some(format!("Auto-login failed: {e}"))),
                    }
                }
            }
        });
    }

    // Clear any persisted error when user is on login screen (e.g. after wrong password)
    {
        let mut err_sig = err.to_owned();
        use_effect(move || if !authed() { err_sig.set(None); });
    }

    let on_login = {
        let mut authed_sig = authed.to_owned();
        let mut err_sig = err.to_owned();
        Callback::new(move |(ok, e): (bool, Option<String>)| {
            if ok { err_sig.set(None); authed_sig.set(true); }
            else if let Some(msg) = e { err_sig.set(Some(msg)); }
        })
    };

    let page = use_signal(|| "My Tasks".to_string());
    let refresh_nonce = use_signal(|| 0u64);
    let mut create_nonce = use_signal(|| 0u64);
    let show_prefs = use_signal(|| false);

    // Callbacks
    let on_tab = Callback::new({ let mut p = page.to_owned(); move |pname: String| p.set(pname) });
    let on_refresh = Callback::new({ let mut r = refresh_nonce.to_owned(); move |_| r.set(r()+1) });
    let on_new_task = Callback::new(move |_| create_nonce.set(create_nonce()+1));
    let on_preferences = Callback::new({ let mut sp = show_prefs.to_owned(); move |_| sp.set(true) });
    let on_logout = Callback::new({
        let mut a = authed.to_owned();
        move |_| {
            clear_session();
            if let Ok(mut g) = database::CURRENT_USER_INFO.try_lock() { *g = None; }
            a.set(false);
        }
    });

    rsx! {
        document::Link { rel: "icon", href: FAVICON }
        document::Link { rel: "stylesheet", href: STYLES_CSS }
        crate::components::toast::ToastFrame { manager: toast }

        if !authed() {
            pages::login::LoginPage { on_login }
        } else {
            div { class: "h-screen max-h-screen bg-galaxy text-star-white flex flex-col overflow-hidden",
                // Top bar (fixed height)
                components::navbar::TopBar {
                    active: page(),
                    on_tab: on_tab.clone(),
                    on_refresh: on_refresh.clone(),
                    on_new_task: on_new_task.clone(),
                    on_preferences: on_preferences.clone(),
                    on_logout: on_logout.clone(),
                }

                // Scrollable content (takes remaining space; bottom tabs stay in flow)
                div { class: "flex-1 min-h-0 overflow-y-auto overflow-x-hidden",
                    pages::tasks::TaskBoard {
                        page: page(),
                        on_navigate: None,
                        refresh_token: refresh_nonce(),
                        create_task_trigger: create_nonce(),
                    }
                }

                // Bottom tab bar (always visible, in flow)
                components::navbar::BottomTabs {
                    active: page(),
                    on_tab: on_tab.clone(),
                }
            }
            if show_prefs() { PreferencesModal { show: show_prefs } }
        }
        if let Some(e) = err() {
            div { class: "fixed bottom-20 left-4 right-4 card-cosmic px-3 py-2 text-warning-red text-xs z-50", {e} }
        }
    }
}

// ── Preferences Modal ────────────────────────────────────────
#[derive(Props, PartialEq, Clone)]
struct PreferencesModalProps { show: Signal<bool> }

#[component]
fn PreferencesModal(props: PreferencesModalProps) -> Element {
    let content: Element = rsx! {
        div { class: "space-y-3 text-sm text-star-white",
            h2 { class: "text-base font-semibold", "Preferences" }
            div { class: "text-xs text-stardust", "Version: dev" }
        }
    };
    rsx! {
        crate::components::modal::Dialog { show_modal: props.show, close_button_label: Some("Done".into()), children: content }
    }
}
