use dioxus::prelude::*;
use database::schema::User;

// ── Props ────────────────────────────────────────────────────
#[derive(Props, PartialEq, Clone)]
pub struct NavbarProps {
    pub active: String,
    #[props(default)]
    pub on_tab: Option<Callback<String>>,
    #[props(default)]
    pub on_refresh: Option<Callback<()>>,
    #[props(default)]
    pub on_new_task: Option<Callback<()>>,
    #[props(default)]
    pub on_preferences: Option<Callback<()>>,
    #[props(default)]
    pub on_logout: Option<Callback<()>>,
}

// ── Top bar: slim title + actions ────────────────────────────
#[component]
pub fn TopBar(props: NavbarProps) -> Element {
    let mut menu_open = use_signal(|| false);
    let user: Option<User> = {
        if let Ok(g) = database::CURRENT_USER_INFO.try_lock() { g.clone() } else { None }
    };
    let initials = user.as_ref().map(|u| {
        let n = u.get_username();
        n.chars().next().unwrap_or('?').to_uppercase().to_string()
    }).unwrap_or_else(|| "?".into());

    rsx! {
        div { class: "nav-galaxy flex items-center h-11 px-3 gap-2 sticky top-0 z-30",
            // Logo dot
            div { class: "w-5 h-5 rounded-md grad-crimson-deep flex-shrink-0" }
            span { class: "text-sm font-semibold text-star-white truncate flex-1", {props.active.clone()} }

            // + New
            button {
                class: "btn-nebula text-xs px-3 py-1",
                r#type: "button",
                onclick: move |_| { if let Some(cb) = &props.on_new_task { cb.call(()); } },
                "+"
            }

            // Refresh
            button {
                class: "btn-cosmic text-xs px-2 py-1",
                r#type: "button",
                onclick: move |_| { if let Some(cb) = &props.on_refresh { cb.call(()); } },
                "↻"
            }

            // Avatar / user menu
            div { class: "relative flex-shrink-0",
                button {
                    class: "w-8 h-8 rounded-full grad-crimson flex items-center justify-center text-xs font-bold text-star-white",
                    r#type: "button",
                    onclick: move |_| menu_open.set(!menu_open()),
                    {initials.clone()}
                }
                if menu_open() {
                    // Full-screen backdrop so dropdown doesn't affect layout
                    div {
                        class: "fixed inset-0 z-40",
                        onclick: move |_| menu_open.set(false),
                    }
                    // Fixed position so it overlays content; navbar doesn't grow
                    div { class: "fixed right-2 top-12 w-40 card-cosmic p-1 z-50 shadow-nebula",
                        if let Some(u) = user.clone() {
                            div { class: "px-3 py-1.5 text-xs text-stardust border-b border-[#401c2a]/40 truncate", {u.get_username()} }
                        }
                        button {
                            class: "w-full text-left px-3 py-2 text-xs nav-link",
                            r#type: "button",
                            onclick: move |_| {
                                menu_open.set(false);
                                if let Some(cb) = &props.on_preferences { cb.call(()); }
                            },
                            "Preferences"
                        }
                        button {
                            class: "w-full text-left px-3 py-2 text-xs nav-link",
                            r#type: "button",
                            onclick: move |_| {
                                menu_open.set(false);
                                if let Some(cb) = &props.on_logout { cb.call(()); }
                            },
                            "Logout"
                        }
                    }
                }
            }
        }
    }
}

// ── Bottom tab bar ───────────────────────────────────────────
#[component]
pub fn BottomTabs(props: NavbarProps) -> Element {
    let mut tabs: Vec<(&str, &str)> = vec![("My Tasks", "☰"), ("Store Tasks", "🏪"), ("Completed", "✓")];
    #[cfg(feature = "client-sessions")]
    tabs.push(("Clients", "🖥"));

    rsx! {
        div { class: "flex-shrink-0 tabbar-wrap",
            div { class: "tabbar-float flex items-stretch gap-1",
                for (label, icon) in tabs.iter() {
                    {
                        let is_active = props.active.as_str() == *label
                            || (*label == "Completed" && props.active.as_str() == "Completed Tasks");
                        let label_s = if *label == "Completed" { "Completed Tasks".to_string() } else { label.to_string() };
                        rsx! {
                            button {
                                class: format!("tab-btn {}", if is_active { "tab-btn-active" } else { "" }),
                                r#type: "button",
                                onclick: move |_| {
                                    if let Some(cb) = &props.on_tab { cb.call(label_s.clone()); }
                                },
                                span { class: "tab-icon", {*icon} }
                                span { {*label} }
                            }
                        }
                    }
                }
            }
        }
    }
}
