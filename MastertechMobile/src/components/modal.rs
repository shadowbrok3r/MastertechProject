use dioxus::prelude::*;

/// Simple modal wrapper with galactic theme. Use for task detail, create task, preferences.
#[derive(Props, PartialEq, Clone)]
pub struct DialogProps {
    pub show_modal: Signal<bool>,
    #[props(default)]
    pub wrap_class: Option<String>,
    #[props(default)]
    pub close_button_label: Option<String>,
    #[props(default)]
    pub close_button_class: Option<String>,
    /// Modal body content. Explicit type helps inference on iOS/linux cross-build.
    pub children: Element,
}

#[component]
pub fn Dialog(props: DialogProps) -> Element {
    if !(props.show_modal)() {
        return rsx! {};
    }
    let wrap = props.wrap_class.clone().unwrap_or_default();
    let close_label = props.close_button_label.clone().unwrap_or_else(|| "Close".into());
    rsx! {
        div { class: "fixed inset-0 z-50 flex items-center justify-center bg-galaxy-overlay",
            div { class: "card-cosmic p-4 max-w-md w-[92%] shadow-nebula ".to_string() + &wrap,
                div { class: "mb-3", {props.children} }
                div { class: "text-right",
                    button {
                        class: "btn-cosmic touch-target",
                        onclick: move |_| {
                            let mut sig = props.show_modal.to_owned();
                            sig.set(false);
                        },
                        {close_label}
                    }
                }
            }
        }
    }
}
