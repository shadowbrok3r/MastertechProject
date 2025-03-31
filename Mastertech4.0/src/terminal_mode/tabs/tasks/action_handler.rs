use crate::terminal_mode::events::action_handler::{ActionHandler, WidgetEvent, WidgetId};
use super::TasksTab;

impl ActionHandler for TasksTab {
    fn widget_id(&self) -> WidgetId {
        WidgetId("TasksTab".to_string()) // Unique ID for the tab
    }

    fn managed_widget_ids(&self) -> Vec<WidgetId> {
        Vec::new()
    }

    fn handle_event(&mut self, event: &WidgetEvent) {
        match event {
            WidgetEvent::Active { widget_id: _ } => {

            }
            WidgetEvent::ButtonClick { widget_id: _, button: _} => {
            }
            _ => {}
        }
    }
}