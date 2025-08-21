use dioxus::prelude::*;

#[derive(Props, PartialEq, Clone)]
pub struct DialogProps {
    pub show_modal: Signal<bool>,
    #[props(default)]
    pub wrap_class: Option<String>,
    #[props(default)]
    pub close_button_label: Option<String>,
    #[props(default)]
    pub close_button_class: Option<String>,
    #[props(default)]
    pub children: Option<Element>,
}

#[component]
pub fn Dialog(props: DialogProps) -> Element {
    if !(props.show_modal)() { return rsx!{}; }
    let wrap = props.wrap_class.clone().unwrap_or_default();
    let close_label = props.close_button_label.clone().unwrap_or_else(|| "Close".into());
    let close_class = props.close_button_class.clone().unwrap_or_else(|| "px-3 py-1 rounded border border-[#2a2c5d]/60 hover:bg-[#1e1a2a]/50".into());
    rsx! {
        div { class: "fixed inset-0 z-50 flex items-center justify-center bg-black/50",
            div { class: format!("bg-[#0b0b0f] border border-[#2a2c5d]/60 rounded shadow max-w-md w-[92%] p-4 {}", wrap),
                div { class: "mb-3", {props.children.clone()} }
                div { class: "text-right",
                    button { class: close_class, onclick: move |_| { let mut sig = props.show_modal.to_owned(); sig.set(false); }, {close_label} }
                }
            }
        }
    }
}
