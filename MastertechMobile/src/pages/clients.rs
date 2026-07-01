use dioxus::prelude::*;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Duration;

use crate::services::clients::{fetch_connected_clients, ClientSession};
use database::schema::ConnectedClient;

// Owned snapshot of the selected session for rendering without holding the
// RefCell borrow across the rsx! build.
struct PanelData {
    cs: String,
    friendly: String,
    transport: String,
    connected: bool,
    status: String,
    log: Vec<String>,
}

#[component]
pub fn ClientsPage() -> Element {
    let sessions: Rc<RefCell<HashMap<String, ClientSession>>> =
        use_hook(|| Rc::new(RefCell::new(HashMap::new())));
    let mut clients = use_signal(Vec::<ConnectedClient>::new);
    let mut version = use_signal(|| 0u64);
    let mut selected = use_signal(|| Option::<String>::None);
    let mut shell_input = use_signal(String::new);
    let mut pending = use_signal(|| Option::<&'static str>::None);

    // Poll the auth-scoped connected-client list every 3s.
    {
        let mut clients = clients.to_owned();
        use_future(move || async move {
            loop {
                clients.set(fetch_connected_clients().await);
                futures_timer::Delay::new(Duration::from_secs(3)).await;
            }
        });
    }

    // Pump every open session's transport; bump `version` when anything changes.
    {
        let sessions = sessions.clone();
        let mut version = version.to_owned();
        use_future(move || {
            let sessions = sessions.clone();
            async move {
                loop {
                    let mut changed = false;
                    {
                        let mut map = sessions.borrow_mut();
                        for session in map.values_mut() {
                            if session.pump() {
                                changed = true;
                            }
                        }
                    }
                    if changed {
                        version.set(version() + 1);
                    }
                    futures_timer::Delay::new(Duration::from_millis(150)).await;
                }
            }
        });
    }

    let _ = version(); // subscribe to session-state changes
    let client_list = clients();
    let sel = selected();

    let (panel, open_keys): (Option<PanelData>, HashSet<String>) = {
        let map = sessions.borrow();
        let open_keys = map.keys().cloned().collect::<HashSet<_>>();
        let panel = sel.as_ref().and_then(|cs| map.get(cs)).map(|s| PanelData {
            cs: s.connection_string.clone(),
            friendly: s.friendly.clone(),
            transport: s.transport_label().to_string(),
            connected: s.connected,
            status: s.status.clone(),
            log: s.log.clone(),
        });
        (panel, open_keys)
    };

    rsx! {
        div { class: "px-4 pt-3 pb-2",
            div { class: "text-xs text-stardust", "Connected clients — tap to open a control session" }
        }

        // ── Connected client list ─────────────────────────────
        div { class: "px-4 pb-3 space-y-2 max-w-2xl mx-auto",
            if client_list.is_empty() {
                div { class: "p-6 text-center text-sm text-stardust", "No connected clients." }
            }
            for client in client_list.iter().cloned() {
                {
                    let cs = client.connection_string.clone();
                    let is_open = open_keys.contains(&cs);
                    let name = client.friendly_name.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| cs.clone());
                    let via = if client.local_ip.as_deref().map(|s| !s.is_empty()).unwrap_or(false) { "TCP" } else { "Relay" };
                    let cs_open = cs.clone();
                    let btn_cls = if is_open { "btn-cosmic text-xs px-3 py-1" } else { "btn-nebula text-xs px-3 py-1" };
                    rsx! {
                        div { class: "card-cosmic p-3 flex items-center gap-2",
                            div { class: "flex-1 min-w-0",
                                div { class: "text-sm font-medium text-star-white truncate", {name} }
                                div { class: "text-[10px] text-stardust truncate", "{cs} · {via}" }
                            }
                            button {
                                class: btn_cls,
                                r#type: "button",
                                onclick: {
                                    let sessions = sessions.clone();
                                    let client = client.clone();
                                    move |_| {
                                        if !sessions.borrow().contains_key(&cs_open) {
                                            if let Some(session) = ClientSession::open(&client) {
                                                sessions.borrow_mut().insert(cs_open.clone(), session);
                                            }
                                        }
                                        selected.set(Some(cs_open.clone()));
                                        pending.set(None);
                                        version.set(version() + 1);
                                    }
                                },
                                if is_open { "View" } else { "Open" }
                            }
                        }
                    }
                }
            }
        }

        // ── Active session panel ──────────────────────────────
        if let Some(p) = panel {
            {
                let cs_shell = p.cs.clone();
                let cs_reboot = p.cs.clone();
                let cs_shutdown = p.cs.clone();
                let cs_lock = p.cs.clone();
                let cs_refresh = p.cs.clone();
                let cs_disc = p.cs.clone();
                let cs_confirm = p.cs.clone();
                let dot = if p.connected { "bg-alien-green" } else { "bg-warning-red" };
                rsx! {
                    div { class: "px-4 pb-6 max-w-2xl mx-auto space-y-2",
                        div { class: "card-cosmic p-3 space-y-3",
                            // Header
                            div { class: "flex items-center gap-2",
                                div { class: "w-2.5 h-2.5 rounded-full {dot}" }
                                div { class: "flex-1 min-w-0",
                                    div { class: "text-sm font-semibold text-star-white truncate", {p.friendly.clone()} }
                                    div { class: "text-[10px] text-stardust truncate", "{p.transport} · {p.status}" }
                                }
                                button {
                                    class: "btn-cosmic text-xs px-2 py-1",
                                    r#type: "button",
                                    onclick: {
                                        let sessions = sessions.clone();
                                        move |_| {
                                            if let Some(s) = sessions.borrow_mut().get_mut(&cs_disc) { s.disconnect(); }
                                            selected.set(None);
                                            version.set(version() + 1);
                                        }
                                    },
                                    "Close"
                                }
                            }

                            // Power / live actions
                            div { class: "flex flex-wrap gap-2",
                                button {
                                    class: "btn-cosmic text-xs px-3 py-1", r#type: "button",
                                    onclick: {
                                        let sessions = sessions.clone();
                                        move |_| { if let Some(s) = sessions.borrow_mut().get_mut(&cs_refresh) { s.refresh_live(); } version.set(version() + 1); }
                                    },
                                    "Live data"
                                }
                                button {
                                    class: "btn-cosmic text-xs px-3 py-1", r#type: "button",
                                    onclick: move |_| pending.set(Some("lock")),
                                    "Lock"
                                }
                                button {
                                    class: "btn-cosmic text-xs px-3 py-1", r#type: "button",
                                    onclick: move |_| pending.set(Some("reboot")),
                                    "Reboot"
                                }
                                button {
                                    class: "btn-cosmic text-xs px-3 py-1", r#type: "button",
                                    onclick: move |_| pending.set(Some("shutdown")),
                                    "Shutdown"
                                }
                            }

                            // Confirm bar for destructive actions
                            if let Some(action) = pending() {
                                div { class: "flex items-center gap-2 text-xs card-stat p-2",
                                    span { class: "text-comet-gold flex-1", {format!("Confirm {action} on {}?", p.friendly)} }
                                    button {
                                        class: "btn-danger text-xs px-3 py-1", r#type: "button",
                                        onclick: {
                                            let sessions = sessions.clone();
                                            move |_| {
                                                if let Some(s) = sessions.borrow_mut().get_mut(&cs_confirm) {
                                                    match action {
                                                        "reboot" => s.reboot(),
                                                        "shutdown" => s.shutdown(),
                                                        "lock" => s.lock(),
                                                        _ => {}
                                                    }
                                                }
                                                pending.set(None);
                                                version.set(version() + 1);
                                            }
                                        },
                                        "Yes"
                                    }
                                    button {
                                        class: "btn-cosmic text-xs px-3 py-1", r#type: "button",
                                        onclick: move |_| pending.set(None),
                                        "Cancel"
                                    }
                                }
                            }

                            // Shell command
                            div { class: "flex items-end gap-2",
                                input {
                                    class: "flex-1 text-xs", r#type: "text", placeholder: "Shell command…",
                                    value: shell_input(),
                                    oninput: move |e| shell_input.set(e.value()),
                                }
                                button {
                                    class: "btn-nebula text-xs px-3 py-1", r#type: "button",
                                    onclick: {
                                        let sessions = sessions.clone();
                                        move |_| {
                                            let cmd = shell_input().trim().to_string();
                                            if cmd.is_empty() { return; }
                                            if let Some(s) = sessions.borrow_mut().get_mut(&cs_shell) { s.run_shell(cmd); }
                                            shell_input.set(String::new());
                                            version.set(version() + 1);
                                        }
                                    },
                                    "Run"
                                }
                            }

                            // Session log
                            div { class: "rounded-lg p-2 max-h-64 overflow-y-auto",
                                style: "background: rgba(12,4,8,0.9); border: 1px solid rgba(255,45,85,0.2);",
                                if p.log.is_empty() {
                                    div { class: "text-[11px] text-stardust", "No output yet." }
                                }
                                for line in p.log.iter() {
                                    div { class: "text-[11px] text-moonlight whitespace-pre-wrap", style: "font-family: 'JetBrains Mono', monospace;", {line.clone()} }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
