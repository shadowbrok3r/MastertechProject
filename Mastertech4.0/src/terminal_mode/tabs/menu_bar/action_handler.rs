
use crate::terminal_mode::events::action_handler::{ActionHandler, WidgetEvent};

use super::{MenuBar, Tab};

impl<'a> ActionHandler for MenuBar<'a> {
    fn handle_event(&mut self, event: &WidgetEvent) {
        match event {
            WidgetEvent::ButtonClick { widget_id , button: _} => {
                let mut current_tab = self.current_tab.borrow_mut();
                match widget_id.0.as_str() {
                    "Ticket" => { *current_tab = Tab::TurSheet; }
                    "Scripts" => { *current_tab = Tab::Scripts; }
                    "System" => { *current_tab = Tab::SystemInfo; }
                    "Ncdu" => { *current_tab = Tab::Ncdu; }
                    "Tasks" => { *current_tab = Tab::Tasks; }
                    "Webconsole" => { *current_tab = Tab::Webconsole; }
                    "Logs" => { *current_tab = Tab::Logs; }
                    "Login" => { *current_tab = Tab::Login; }
                    "Connect" => { let _ = self.manual_start_tx.send(true); }
                    _ => {}
                }
            }
            WidgetEvent::Api(_api_event) => {}
            WidgetEvent::Active { widget_id: _ } => {}
        }
    }
}
