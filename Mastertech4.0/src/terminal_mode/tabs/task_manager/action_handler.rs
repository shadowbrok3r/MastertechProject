use crate::terminal_mode::events::action_handler::{ActionHandler, WidgetEvent, WidgetId};

use super::SysinfoTab;

impl ActionHandler for SysinfoTab {
    fn widget_id(&self) -> WidgetId {
        WidgetId("SysinfoTab".to_string()) // Unique ID for the tab
    }

    fn managed_widget_ids(&self) -> Vec<WidgetId> {
        vec![]
    }

    fn handle_event(&mut self, event: &WidgetEvent) {
        match event {
            WidgetEvent::ButtonClick { widget_id: _ , button: _, source: _} => {}
            _ => {}
        }
    }
}
