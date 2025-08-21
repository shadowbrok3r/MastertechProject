use dioxus::prelude::*;
use crate::pages::tasks::TaskBoard;

#[component]
pub fn CompletedTasksPage() -> Element {
    rsx! { TaskBoard { page: "Completed Tasks".to_string() } }
}
