use eframe::egui::{text::LayoutJob, Align, Button, Color32, FontFamily, FontId, Frame, Layout, Margin, RichText, TextFormat, Ui, Vec2, Widget, WidgetText};
use database::schema::{ConnectedClient, RecordIdExt};
use std::collections::HashMap;
use crossbeam::channel::Sender;
use chrono::{DateTime, Local, Utc};
use super::ClientUiAction;
use crate::get_database_users;
use log::info;

use super::{AdminConsole, WebConsolePageState};

impl AdminConsole {
    pub fn client_header(
        ui: &mut Ui, 
        tx: Sender<ClientUiAction>, 
        client: &ConnectedClient, 
        undock_client: HashMap<String, bool>,
        is_ws_connected: bool, // True if WebSocket connection is active and responding
    ) {
        let style = ui.style().clone();
        Frame::default()
            .fill(Color32::from_rgb(13, 13, 15))
            .inner_margin(Margin::same(4))
            .outer_margin(Margin::symmetric(3, 0))
            .corner_radius(eframe::egui::CornerRadius::same(5))
            .stroke(style.visuals.window_stroke)
            .show(ui, |ui| 
        {
            // let ui = &mut header_frame.content_ui;
            ui.set_height(25.);
            ui.horizontal_top(|ui| {
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    // Green dot indicator for active connection
                    let (indicator_color, indicator_text) = if is_ws_connected {
                        (
                            Color32::from_rgb(50, 205, 50), // Lime green for active
                            "●"
                        )
                    } else if client.connected {
                        (
                            Color32::from_rgb(255, 200, 0), // Yellow for DB-connected but no active WS
                            "⚠"
                        )
                    } else {
                        (
                            Color32::from_rgb(128, 128, 128), // Gray for disconnected
                            "⊗"
                        )
                    };
                    
                    ui.colored_label(indicator_color, indicator_text);
                    ui.add_space(4.0);
                    
                    // Create a new LayoutJob
                    let mut job = LayoutJob::default();

                    if let Some(friendly_name) = client.clone().friendly_name {
                        job.append(
                            &friendly_name,
                            0.0,
                            TextFormat {
                                font_id: FontId::new(13., FontFamily::Proportional),
                                color: Color32::from_rgb(51, 255, 189), // Set the color for the first part
                                valign: Align::Min,
                                ..Default::default()
                            },
                        );
                    } else {
                        let conn_string = &client.connection_string;
                        let txt = conn_string.split_once(':');
                        if let Some(txt) = txt {
                            let text = format!("{}:", txt.0);
                            job.append(
                                &text,
                                0.0,
                                TextFormat {
                                    font_id: FontId::new(13., FontFamily::Proportional),
                                    color: Color32::from_rgb(51, 255, 189), // Set the color for the first part
                                    valign: Align::Min,
                                    ..Default::default()
                                },
                            );
                            job.append(
                                txt.1,
                                0.0,
                                TextFormat {
                                    font_id: FontId::new(13., FontFamily::Proportional),
                                    color: Color32::from_rgb(199, 202, 245),
                                    valign: Align::Min,
                                    ..Default::default()
                                },
                            );
                        }
                    };

                    // Convert LayoutJob to WidgetText
                    let formatted_text = WidgetText::from(job);
                    
                    // Build hover text with date and assigned user info
                    let parsed_date = DateTime::parse_from_rfc3339(
                        &client.last_update.clone().unwrap_or(Utc::now().into()).to_string()
                    )
                    .unwrap_or_default()
                    .with_timezone(&Local);
                    let formatted_date = parsed_date.format("%Y/%m/%d @ %I:%M%p").to_string();
                    
                    // Look up assigned user name
                    let assigned_user_text = if let Some(ref user_id) = client.assigned_user {
                        let users = get_database_users();
                        users.iter()
                            .find(|u| u.get_id().key_string() == user_id.key_string())
                            .map(|u| format!("Assigned to: {}", u.get_name()))
                            .unwrap_or_else(|| format!("Assigned to: {}", user_id.key_string()))
                    } else {
                        "Assigned to: (none)".to_string()
                    };
                    
                    let hover_text = format!("{}\n{}", formatted_date, assigned_user_text);
                    let _ = Button::new(formatted_text).ui(ui).on_hover_text(hover_text);
                });


                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let button = Button::new(RichText::new("⬈").strong().color(ui.style().visuals.warn_fg_color))
                        .fill(ui.style().visuals.window_fill)
                        .min_size(Vec2::new(30.0, 30.))
                        .ui(ui);

                    if button.clicked() {
                        info!("Sent Connection Command");
                        let _ = tx.try_send(ClientUiAction::ConnectClient(client.clone()));
                    }

                    let txt = if let Some(docked) = undock_client.get(client.connection_string.as_str()) {
                        if *docked { "🔒" } // Docked = locked
                        else { "🔓" }       // Undocked = unlocked
                    } else { "🔒" };        // Default to docked

                    let undock = Button::new(RichText::new(txt).strong().color(Color32::LIGHT_RED))
                        .fill(ui.style().visuals.window_fill)
                        .min_size(Vec2::new(30., 30.))
                        .ui(ui);

                    if undock.clicked() {
                        let _ = tx.try_send(ClientUiAction::UndockClient(client.connection_string.clone()));
                    }

                    let button = Button::new(RichText::new("✖").strong().color(ui.style().visuals.error_fg_color))
                        .fill(ui.style().visuals.window_fill)
                        .min_size(Vec2::new(30., 30.))
                        .ui(ui);

                    if button.clicked() {
                        let _ = tx.try_send(ClientUiAction::DeleteClient(client.clone()));
                    }
                    // ui.add_space(10.0);

                    // let export =
                    //     Button::new(RichText::new("Export").size(10.0).color(Color32::LIGHT_RED))
                    //         .fill(Color32::TRANSPARENT)
                    //         .min_size(Vec2::new(30.0, ui.available_height()))
                    //         .ui(ui);

                    // if export.clicked() {
                    //     let _ = tx.try_send(ClientUiAction::ExportHistory(client.clone()));
                    // }
                    
                });
            });
        });

        // let response = header_frame.allocate_space(ui);
        // if response.hovered() {
        //     header_frame.frame.stroke = style.visuals.widgets.hovered.fg_stroke;
        //     header_frame.frame.shadow = style.visuals.window_shadow;
        // } else {
        //     header_frame.frame.stroke = style.visuals.widgets.open.bg_stroke;
        // }
        // header_frame.
        // header_frame.paint(ui);

    }

    pub fn ui(&mut self, ui: &mut Ui) {
        match self.state {
            WebConsolePageState::ScriptEditor => self.script_editor.ui(ui),
            #[cfg(not(target_arch = "wasm32"))]
            WebConsolePageState::AiPlayground => self.ai_playground.enhanced_ai_playground(ui),
            _ => {
                for client in self.clients.iter() {
                    if self.undock_client.iter().any(|c| 
                        !c.1 && c.0 == &client.connection_string
                    ) {
                        if let Some(ws_client) = self.ws_clients.get_mut(&client.connection_string) {
                            // Sync the latest ConnectedClient data (especially last_activity) from live queries
                            ws_client.client = client.clone();
                            ws_client.show(ui);
                        }
                    }
                }
            }
        }
    
    }
}