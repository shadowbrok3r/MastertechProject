use crate::terminal_mode::events::action_handler::{ActionHandler, WidgetEvent, WidgetId};

use super::MenuBar;

impl<'a> ActionHandler for MenuBar<'a> {
    fn widget_id(&self) -> WidgetId {
        WidgetId("MenuBar".to_string())
    }

    fn managed_widget_ids(&self) -> Vec<WidgetId> {
        vec![WidgetId("Connect".to_string())]
    }

    fn handle_event(&mut self, event: &WidgetEvent) {
        match event {
            WidgetEvent::ButtonClick { widget_id, button: _, source: _ } => {
                if widget_id.0 == "Connect" {
                    let _ = self.manual_start_tx.send(true);
                }
            }
            WidgetEvent::Api(_api_event) => {}
            WidgetEvent::Active { widget_id: _ } => {}
        }
    }
}
