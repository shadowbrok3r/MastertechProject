use dioxus::prelude::*;
use database::schema::User;

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
}

#[component]
pub fn NavbarWithLogo(props: NavbarProps) -> Element {
    let active = props.active.clone();
    let mut menu_open = use_signal(|| false);
    let user: Option<User> = {
        if let Ok(g) = database::CURRENT_USER_INFO.try_lock() { g.clone() } else { None }
    };

    let tab_btn = |name: &str| {
        let is_active = active.as_str() == name;
        let cls = if is_active {
            // Active tab: subtle gradient + ring accent, crisper border
            "h-8 px-3 text-sm rounded-md bg-gradient-to-b from-[#1f1a33]/70 to-[#141124]/70 border border-[#7c76c3]/80 text-[#e8e6ff] shadow-sm ring-1 ring-[#7c76c3]/30"
        } else {
            // Inactive tab: neutral surface with hover transitions
            "h-8 px-3 text-sm rounded-md border border-[#2a2c5d]/60 text-[#c8c3e6]/90 hover:bg-[#171225]/60 hover:border-[#6a659b]/80 transition-colors duration-150"
        };
    let name_owned = name.to_string();
    let label = name_owned.clone();
    rsx! { button { class: cls, r#type: "button", role: "tab", aria_selected: is_active, onclick: move |_| { if let Some(cb)=&props.on_tab { cb.call(name_owned.clone()); } }, {label} } }
    };

    rsx! {
        nav { class: "sticky top-0 z-30 bg-[#0b0b0f]/90 backdrop-blur border-b border-[#2a2c5d]/60 px-3 py-2 flex items-center gap-3 shadow-[0_1px_0_0_rgba(124,118,195,0.12)]",
            // Logo
            div { class: "flex items-center gap-2 select-none",
                div { class: "w-6 h-6 rounded-md bg-gradient-to-br from-[#6a659b] to-[#4a4575] shadow" }
                span { class: "text-sm font-semibold tracking-wide text-[#e8e6ff]", "Mastertech" }
            }

            // Tabs: allow side-scroll on narrow screens
            div { class: "flex items-center gap-2 ml-2 overflow-x-auto whitespace-nowrap pr-2" , role: "tablist",
                {tab_btn("My Tasks")}
                {tab_btn("Store Tasks")}
                {tab_btn("Completed Tasks")}
            }

            div { class: "ml-auto flex items-center gap-2",
                button { class: "h-8 px-3 text-sm rounded-md border border-[#2a2c5d]/60 text-[#c8c3e6]/90 hover:bg-[#1e1a2a]/55 hover:border-[#6a659b]/80 transition-colors", r#type: "button", onclick: move |_| { if let Some(cb)=&props.on_refresh { cb.call(()); } }, "Refresh" }
                button { class: "h-8 px-3 text-sm rounded-md border border-[#2a2c5d]/60 text-[#e8e6ff] bg-[#251d3d]/60 hover:bg-[#2b214a]/70 hover:border-[#7c76c3]/80 transition-colors", r#type: "button", onclick: move |_| { if let Some(cb)=&props.on_new_task { cb.call(()); } }, "+ New" }
                // User menu
                if let Some(u) = user.clone() {
                    div { class: "relative",
                        button { class: "h-8 px-3 text-sm rounded-md border border-[#2a2c5d]/60 text-[#c8c3e6]/90 hover:bg-[#1e1a2a]/55 hover:border-[#6a659b]/80 transition-colors", r#type: "button", onclick: move |_| menu_open.set(!menu_open()), {u.get_username()} }
                        if menu_open() {
                            div { class: "absolute right-0 mt-2 w-48 bg-[#0b0b0f]/98 border border-[#2a2c5d]/60 rounded-md shadow-lg z-40 overflow-hidden backdrop-blur",
                                button { class: "w-full text-left px-3 py-2 text-sm hover:bg-[#1e1a2a]/55 transition-colors", r#type: "button", onclick: move |_| { menu_open.set(false); if let Some(cb)=&props.on_preferences { cb.call(()); } }, "Preferences" }
                                button { class: "w-full text-left px-3 py-2 text-sm hover:bg-[#1e1a2a]/55 transition-colors", r#type: "button", onclick: move |_| { menu_open.set(false); /* TODO logout - for desktop dev we just reset auth? */ }, "Logout" }
                            }
                        }
                    }
                }
            }
        }
    }
}
