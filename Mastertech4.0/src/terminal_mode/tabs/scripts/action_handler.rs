use std::sync::{Arc, Mutex};

use ratatui::layout::Rect;

use crate::terminal_mode::{context::TerminalContext, events::action_handler::{ActionHandler, WidgetEvent}, widgets::ButtonType};
use super::ScriptsTab;

impl<'a> ActionHandler for ScriptsTab<'a> {
    fn handle_event(&mut self, event: &WidgetEvent, _ctx: Arc<Mutex<TerminalContext>>) {
        match event {
            WidgetEvent::ButtonClick { widget_id , button} => {
                log::info!("Button: {button:?}\nwidget: {widget_id:?}");
                // Show popup to the right of the clicked button
                let button = match widget_id.0.as_str() {
                    "Tuneup" => Some(&self.tuneup_btn),
                    "Qc" => Some(&self.qc_btn),
                    "WindowsUpdates" => Some(&self.updates_btn),
                    "RunPrechecks" => Some(&self.prechecks_btn),
                    "Informational" => Some(&self.informational_btn),
                    _ => None,
                };

                if let Some(btn) = button {
                    if let (Some(button_area), Some(frame_area)) = (btn.get_area(), *self.frame_area.borrow()) {
                        let popup_items = self.popup_items.borrow();
                        let items = popup_items.get(&widget_id.0);
                        let item_count = items.map_or(2, |items| items.len()).max(1);
                        let popup_height = item_count as u16 + 2; // Borders
                        let popup_width = items
                            .map(|items| {
                                items.iter()
                                    .map(|item| item.text.len())
                                    .max()
                                    .unwrap_or(10) + 5 // Padding
                            })
                            .unwrap_or(12) as u16;

                        let popup_x = button_area.x + button_area.width;
                        let popup_y = button_area.y;
                        let adjusted_x = popup_x.min(frame_area.width.saturating_sub(popup_width));
                        let adjusted_y = popup_y.min(frame_area.height.saturating_sub(popup_height));
                        let popup_area = Rect::new(adjusted_x, adjusted_y, popup_width, popup_height);
                        log::info!(
                            "Opening popup for {}: {} items, width: {}, height: {}, area: {:?}",
                            widget_id.0,
                            item_count,
                            popup_width,
                            popup_height,
                            popup_area
                        );
                        self.active_popup.replace(Some((widget_id.clone(), popup_area)));
                        self.list_state.borrow_mut().select(None);
                        self.popup_list_state.borrow_mut().select(None);
                    }
                }
                let id = widget_id.0.as_str();
                match id {
                    "Run" => self.run_selected_scripts(),
                    "Tuneup" => {}
                    "Qc" => {}
                    "WindowsUpdates" => {}
                    "RunPrechecks" => {}
                    "Informational" => {}
                    // Clear popup if click is outside any button
                    _ => {
                        let mut is_open = self.is_popup_open.borrow_mut();
                        for btn in self.data_path_buttons.iter() {
                            let btn_widget_id = btn.get_widget_id().clone();
                            let btn_id = btn_widget_id.0.as_str();
                            if btn_id.eq(id) {
                                let destination = self
                                    .source_directories
                                    .iter()
                                    .filter(|(path, _size)| path.eq(btn_id))
                                    .collect::<Vec<&(String, String)>>();

                                self.log_message(format!("destination dir: {destination:?}"));

                                let sources = self
                                    .source_directories
                                    .iter()
                                    .filter(|(path, _size)| !path.eq(btn_id))
                                    .collect::<Vec<&(String, String)>>();

                                self.log_message(format!("sources: {sources:?}"));

                                *is_open = false;
                            }
                        }

                        self.active_popup.replace(None);
                    }
                }
            }
            WidgetEvent::Api(_) => {},
            WidgetEvent::Active { widget_id } => {self.log_message(&format!("{widget_id:?}"));}
        }
    }
}
