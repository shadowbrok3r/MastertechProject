//! Action handler implementation for TaskModal
use crate::terminal_mode::events::action_handler::{ActionHandler, WidgetEvent, WidgetId};
use super::{ModalPage, TaskModal};

impl<'a> ActionHandler for TaskModal<'a> {
    fn widget_id(&self) -> WidgetId {
        WidgetId(format!("TaskModal_{}", self.modal_id))
    }
    
    fn managed_widget_ids(&self) -> Vec<WidgetId> {
        let mut ids = vec![
            WidgetId("TaskModalClose".to_string()),
        ];
        
        // Add tab button IDs
        for page in ModalPage::all() {
            ids.push(WidgetId(page.widget_id().to_string()));
        }
        
        ids
    }
    
    fn handle_event(&mut self, event: &WidgetEvent) {
        match event {
            WidgetEvent::ButtonClick { widget_id, .. } => {
                match widget_id.0.as_str() {
                    "TaskModalClose" => {
                        log::info!("TaskModal close button clicked");
                        self.request_close();
                    }
                    id => {
                        log::info!("Clicked widget: {id:?}");
                        // Check if it's a tab button click
                        if let Some(page) = ModalPage::from_widget_id(id) {
                            log::info!("TaskModal tab clicked: {:?}", page);
                            self.set_active_tab(page);
                        }
                    }
                }
            }
            WidgetEvent::Active { widget_id } => {
                // Handle tab activation
                if let Some(page) = ModalPage::from_widget_id(&widget_id.0) {
                    self.set_active_tab(page);
                }
            }
            _ => {}
        }
    }
}
