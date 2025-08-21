use dioxus::prelude::*;
use crate::pages::tasks::TaskBoard;

#[component]
pub fn MyTasksPage() -> Element {
    rsx! { TaskBoard { page: "My Tasks".to_string() } }
}
