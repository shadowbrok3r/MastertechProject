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
        div { class: "fixed inset-0 z-50 flex items-center justify-center bg-galaxy-overlay p-3",
            div {
                class: "card-cosmic p-3 max-w-md w-full shadow-nebula overflow-y-auto ".to_string() + &wrap,
                style: "max-height: 85vh;",
                div { class: "mb-2", {props.children} }
                div { class: "text-right",
                    button {
                        class: "btn-cosmic",
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
