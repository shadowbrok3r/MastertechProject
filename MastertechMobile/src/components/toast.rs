use dioxus::prelude::*;

#[derive(Clone, Debug, PartialEq)]
pub struct ToastInfo {
    pub heading: Option<String>,
    pub context: String,
}

#[derive(Clone, Default)]
pub struct ToastManager {
    pub items: Vec<(u64, ToastInfo)>,
    pub next_id: u64,
}

impl ToastManager {
    pub fn popup(&mut self, info: ToastInfo) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.items.push((id, info));
        id
    }
    pub fn remove(&mut self, id: u64) {
        if let Some(idx) = self.items.iter().position(|(i, _)| *i == id) { self.items.remove(idx); }
    }
}

#[derive(Props, PartialEq, Clone)]
pub struct ToastFrameProps { pub manager: Signal<ToastManager> }

#[component]
pub fn ToastFrame(props: ToastFrameProps) -> Element {
    let mut mgr = props.manager;
    rsx! {
        div { class: "fixed top-3 right-3 space-y-2 z-50",
            for (id, info) in mgr.read().items.clone() {
                div { class: "bg-[#0c0c10]/95 border border-[#401c2a]/60 rounded shadow px-3 py-2 text-sm text-slate-100 flex gap-2 items-start max-w-[280px]",
                    div { class: "flex-1",
                        if let Some(h) = info.heading.clone() { div { class: "font-semibold text-xs opacity-80", {h} } }
                        div { {info.context.clone()} }
                    }
                    button { class: "px-2 py-1 text-xs rounded border border-[#401c2a]/60 hover:bg-[#1e1a2a]/50",
                        onclick: move |_| { let mut m = mgr.write(); m.remove(id); }, "×" }
                }
            }
        }
    }
}
