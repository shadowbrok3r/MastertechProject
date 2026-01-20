
use database::schema::utilities::get_tasks_for_store;

use crate::terminal_mode::events::action_handler::{get_event_sender, ActionHandler, WidgetButton, WidgetEvent, WidgetId};

use super::{MenuBar, Tab};

impl<'a> ActionHandler for MenuBar<'a> {
    fn widget_id(&self) -> WidgetId {
        WidgetId("MenuBar".to_string()) // Unique ID for the tab
    }

    fn managed_widget_ids(&self) -> Vec<WidgetId> {
        vec![
            WidgetId("Ticket".to_string()),
            WidgetId("Scripts".to_string()),
            WidgetId("System".to_string()),
            WidgetId("Ncdu".to_string()),
            WidgetId("Tasks".to_string()),
            WidgetId("Webconsole".to_string()),
            WidgetId("Logs".to_string()),
            WidgetId("Login".to_string()),
            WidgetId("Connect".to_string()),
        ]
    }

    fn handle_event(&mut self, event: &WidgetEvent) {
        match event {
            WidgetEvent::ButtonClick { widget_id , button: _, source: _} => {
                // Helper to update tab selection state on all buttons
                let update_tab_selection = |new_tab: Tab, menu: &Self| {
                    menu.ticket_tab.set_selected(matches!(new_tab, Tab::TurSheet));
                    menu.scripts_tab.set_selected(matches!(new_tab, Tab::Scripts));
                    menu.system_tab.set_selected(matches!(new_tab, Tab::SystemInfo));
                    menu.ncdu_tab.set_selected(matches!(new_tab, Tab::Ncdu));
                    menu.tasks_tab.set_selected(matches!(new_tab, Tab::Tasks));
                    menu.webconsole_tab.set_selected(matches!(new_tab, Tab::Webconsole));
                    menu.logs_tab.set_selected(matches!(new_tab, Tab::Logs));
                    menu.login_tab.set_selected(matches!(new_tab, Tab::Login));
                };
                
                let mut current_tab = self.current_tab.borrow_mut();
                match widget_id.0.as_str() {
                    "Ticket" => { 
                        *current_tab = Tab::TurSheet;
                        drop(current_tab);
                        update_tab_selection(Tab::TurSheet, self);
                    }
                    "Scripts" => { 
                        *current_tab = Tab::Scripts;
                        drop(current_tab);
                        update_tab_selection(Tab::Scripts, self);
                    }
                    "System" => { 
                        *current_tab = Tab::SystemInfo;
                        drop(current_tab);
                        update_tab_selection(Tab::SystemInfo, self);
                    }
                    "Ncdu" => { 
                        *current_tab = Tab::Ncdu;
                        drop(current_tab);
                        update_tab_selection(Tab::Ncdu, self);
                    }
                    "Tasks" => { 
                        if let Ok(ctx) = self.ctx.try_lock() {
                            let tx = ctx.tasks_tx.clone();
                            let store = ctx.user.get_store().as_str().to_string();
                            if !store.is_empty() {
                                tokio::spawn(async move {
                                    let tasks_result = get_tasks_for_store(tx, store).await;
                                    log::info!("Tasks result: {tasks_result:?}");
                                });
                            }
                        }
                        *current_tab = Tab::Tasks;
                        drop(current_tab);
                        update_tab_selection(Tab::Tasks, self);
                     }
                    "Webconsole" => { 
                        if *current_tab == Tab::Webconsole {
                            let _ = get_event_sender().try_send(
                                WidgetEvent::ButtonClick { 
                                    widget_id: WidgetId("ToggleSidePanel".to_string()), 
                                    button: WidgetButton::Left,
                                    source: Default::default()
                                }
                            );
                        } else {
                            *current_tab = Tab::Webconsole;
                            drop(current_tab);
                            update_tab_selection(Tab::Webconsole, self);
                        }
                     }
                    "Logs" => { 
                        *current_tab = Tab::Logs;
                        drop(current_tab);
                        update_tab_selection(Tab::Logs, self);
                    }
                    "Login" => { 
                        *current_tab = Tab::Login;
                        drop(current_tab);
                        update_tab_selection(Tab::Login, self);
                    }
                    "Connect" => { let _ = self.manual_start_tx.send(true); }
                    _ => {}
                }
            }
            WidgetEvent::Api(_api_event) => {}
            WidgetEvent::Active { widget_id: _ } => {}
        }
    }
}
