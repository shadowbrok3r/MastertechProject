use dioxus::prelude::*;
use crate::pages::tasks::TaskBoard;

#[component]
pub fn StoreTasksPage() -> Element {
    rsx! { TaskBoard { page: "Store Tasks".to_string() } }
}
