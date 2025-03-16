use crate::terminal_mode::events::action_handler::{ActionHandler, WidgetEvent};
use super::TasksTab;

impl ActionHandler for TasksTab {
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