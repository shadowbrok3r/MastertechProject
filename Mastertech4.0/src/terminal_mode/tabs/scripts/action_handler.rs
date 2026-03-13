use crate::{tabs::file_browser::command::run_robocopy, terminal_mode::{events::action_handler::{ActionHandler, ApiEvent, WidgetButton, WidgetEvent, WidgetId}, widgets::ButtonType}};
use ratatui::layout::Rect;
use std::path::PathBuf;
use super::ScriptsTab;

impl<'a> ActionHandler for ScriptsTab<'a> {
    fn widget_id(&self) -> WidgetId {
        WidgetId("ScriptsTab".to_string()) // Unique ID for the tab
    }

    fn managed_widget_ids(&self) -> Vec<WidgetId> {
        let mut widgets = vec![
            WidgetId("Run".to_string()),
            WidgetId("Tuneup / QC".to_string()),
            WidgetId("Informational".to_string()),
            WidgetId("UserScripts".to_string()),
            WidgetId("ServiceNumberScriptsPage".to_string()),
            WidgetId("CustomPath".to_string()),
        ];

        for btn in self.data_path_buttons.iter() {
            widgets.push(btn.get_widget_id());
        }
        
        widgets
    }

    fn handle_event(&mut self, event: &WidgetEvent) {
        match event {
            WidgetEvent::ButtonClick { widget_id , button, source: _} => {
                log::info!("Button: {button:?}\nwidget: {widget_id:?}");
                // Show popup to the right of the clicked button
                let widget_button = match widget_id.0.as_str() {
                    "Tuneup / QC" => Some(&self.tuneup_btn),
                    "Informational" => Some(&self.informational_btn),
                    "UserScripts" => {
                        self.check_for_scripts = true;
                        Some(&self.user_scripts_btn)
                    },
                    _ => None,
                };

                if let Some(btn) = widget_button {
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
                        self.active_popup.replace(Some((widget_id.clone(), popup_area)));
                        self.list_state.borrow_mut().select(None);
                        self.popup_list_state.borrow_mut().select(None);
                    }
                }
                let id = widget_id.0.as_str();
                match id {
                    "Run" => {
                        if self.run_button_should_be_disabled() {
                            self.log_message("Provide a service number to run Activate Webroot, Activate SuperAnti, or Activate SEB.");
                            return;
                        }
                        let text_area_input = self.service_number_field.input.borrow().clone();
                        let user_input = &text_area_input.lines()[0];
                        self.service_number = user_input.clone();

                        if !self.service_number.is_empty() {
                            if let Ok(ctx) = &mut self.ctx.lock() {
                                let cust_email = ctx.service_data.customer_data.email.clone();
                                let so_num = ctx.service_data.ticket_data.service_number.clone();
                                
                                self.log_message(format!("so_num and cust_email: {so_num} and {cust_email}"));

                                if !cust_email.is_empty() && !so_num.is_empty() {
                                    self.service_number = so_num;
                                    self.customer_email = cust_email;
                                    self.log_message(format!("both empty, assigned"));
                                } else {
                                    ctx.service_data.ticket_data.service_number = self.service_number.clone();
                                    self.log_message(format!("Pulling ticket info: {:?}", self.service_number.clone()));
                                    ctx.service_data.get_ticket();
                                }
                            }
                        }

                        #[cfg(target_os="windows")]
                        self.run_selected_scripts(false);
                    },
                    "Tuneup / QC" => {}
                    "Informational" => {}
                    "UserScripts" => {}
                    "Continue" => {}
                    "CustomPath" => { }
                    _ => {
                        if WidgetButton::Right == *button {
                            // Collect the ID to remove (assuming single match for simplicity)
                            let matching_id = self.data_path_buttons.iter().find_map(|btn| {
                                let btn_widget_id = btn.get_widget_id().clone();
                                let btn_id = btn_widget_id.0.as_str();
                                if btn_id.eq(widget_id.0.as_str()) {
                                    Some(btn_id.to_string())
                                } else {
                                    None
                                }
                            });

                            // Now mutate self after the immutable borrow ends
                            if let Some(id) = matching_id {
                                self.remove_button(&id);
                            }

                        } else {
                            let mut is_open = self.is_popup_open.borrow_mut();
                            for btn in self.data_path_buttons.iter() {
                                let btn_widget_id = btn.get_widget_id().clone();
                                let btn_id = btn_widget_id.0.as_str();
                                if btn_id.eq(id) {
                                    let destination = self
                                        .source_directories
                                        .iter()
                                        .cloned()
                                        .filter(|(path, _size)| path.eq(btn_id))
                                        .collect::<Vec<(String, String)>>();
    
                                    self.log_message(format!("destination dir: {destination:?}"));
    
                                    let sources = self
                                        .source_directories
                                        .iter()
                                        .filter(|(path, _size)| !path.eq(btn_id))
                                        .collect::<Vec<&(String, String)>>();
    
                                    self.log_message(format!("sources: {sources:?}"));
    
                                    *is_open = false;
                                    let data_transfer_progress_tx = self.data_transfer_progress_tx.clone();
                                    let source_clone = self.source_directories.clone();
                                    let destination_clone = destination.clone();

                                    tokio::spawn(async move {
                                        for (src, _size) in  source_clone.iter() {
                                            for (dest, _dest_size) in destination_clone.iter() {
                                                log::info!("Source: {:?}\nDestination: {:?}", src, dest);
                                                if src != dest {
                                                    let result = run_robocopy(
                                                        &PathBuf::from(src),
                                                        &PathBuf::from(dest),
                                                        data_transfer_progress_tx.clone()
                                                    ).await;
                                                    log::info!("Robocopy Run Result: {result:?}");
                                                }
                                            }
                                        }
                                    });
                                }
                            }
                            self.active_popup.replace(None);
                        }
                    }
                }
            }
            WidgetEvent::Api(api_event) => {
                match api_event {
                    ApiEvent::GetTicketResponse(presta_data) => {
                        self.customer_email = presta_data.customer.email.clone();
                        self.log_message(format!("self.customer_email: {:?}", self.customer_email));
                        #[cfg(target_os="windows")]
                        self.run_selected_scripts(true);
                    }
                    ApiEvent::GetSebResponse(_carbonite_response) => {
                        
                    },
                    // These events are handled by ServiceFormTab, not ScriptsTab
                    ApiEvent::DuplicateCheckResponse(_) => {},
                    ApiEvent::TaskCreationResponse(_) => {},
                }
            }
            WidgetEvent::Active { widget_id } => {self.log_message(&format!("{widget_id:?}"));}
        }
    }
}
