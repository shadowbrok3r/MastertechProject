use crate::terminal_mode::events::action_handler::{ActionHandler, WidgetEvent, WidgetId};
use super::{SortColumn, TasksTab};

impl<'a> ActionHandler for TasksTab<'a> {
    fn widget_id(&self) -> WidgetId {
        WidgetId("TasksTab".to_string())
    }

    fn managed_widget_ids(&self) -> Vec<WidgetId> {
        vec![
            // Sort header buttons
            WidgetId("TasksSortDue".to_string()),
            WidgetId("TasksSortStatus".to_string()),
            WidgetId("TasksSortPriority".to_string()),
        ]
    }

    fn handle_event(&mut self, event: &WidgetEvent) {
        match event {
            WidgetEvent::ButtonClick { widget_id, .. } => {
                match widget_id.0.as_str() {
                    "TasksSortDue" => {
                        log::info!("Sort by Due Date clicked");
                        self.toggle_sort(SortColumn::DueDate);
                        self.update_sort_button_labels();
                    }
                    "TasksSortStatus" => {
                        log::info!("Sort by Status clicked");
                        self.toggle_sort(SortColumn::Status);
                        self.update_sort_button_labels();
                    }
                    "TasksSortPriority" => {
                        log::info!("Sort by Priority clicked");
                        self.toggle_sort(SortColumn::Priority);
                        self.update_sort_button_labels();
                    }
                    _ => {}
                }
            }
            WidgetEvent::Active { widget_id: _ } => {}
            _ => {}
        }
    }
}