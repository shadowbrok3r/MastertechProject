use crate::terminal_mode::events::action_handler::{ActionHandler, WidgetEvent, WidgetId};

use super::LoginTab;



impl <'a> ActionHandler for LoginTab <'a> {
    fn handle_event(&mut self, event: &WidgetEvent) {
        match event {
            WidgetEvent::Active { widget_id } => {
                match widget_id.0.as_str() {
                    "Username" => {
                        self.set_active_field(WidgetId("Username".to_string()));
                    }
                    "Password" => {
                        self.set_active_field(WidgetId("Password".to_string()));
                    }
                    _ => {
                        self.active_field.replace(None);
                    }
                }
            }
            _ => {}
        }
    }
}