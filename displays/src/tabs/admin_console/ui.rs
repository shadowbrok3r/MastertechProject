use eframe::egui::{text::LayoutJob, Align, Button, Color32, FontFamily, FontId, Frame, Layout, Margin, RichText, TextFormat, Ui, Vec2, Widget, WidgetText};
use database::schema::{ConnectedClient, RecordIdExt};
use std::collections::HashMap;
use crossbeam::channel::Sender;
use chrono::{DateTime, Local, Utc};
use super::ClientUiAction;
use crate::get_database_users;
use log::info;

use super::{AdminConsole, WebConsolePageState};

/// How stale a `last_update` timestamp can be before we treat the client as
/// offline — even if the DB `connected` flag is still `true`. Five minutes
/// is conservative; the client heartbeats every ~30 s so anything older than
/// that is either crashed or unreachable.
const STALE_THRESHOLD_SECS: i64 = 300;

/// Returns `true` if the client's `last_update` was within [`STALE_THRESHOLD_SECS`].
fn recently_active(client: &ConnectedClient) -> bool {
    let Some(ref dt) = client.last_update else { return false };
    let parsed = DateTime::parse_from_rfc3339(&dt.to_string())
        .map(|d| d.with_timezone(&Utc));
    match parsed {
        Ok(t) => (Utc::now() - t).num_seconds() < STALE_THRESHOLD_SECS,
        Err(_) => false,
    }
}

/// Returns `(color, symbol)` for the connection status dot.
///
/// Priority:
/// 1. Active admin session open → green `●`
/// 2. DB-connected + recent heartbeat + no admin session → yellow `⚠`
/// 3. Everything else (disconnected or stale) → gray `⊗`
fn connection_indicator(is_ws_connected: bool, client: &ConnectedClient) -> (Color32, &'static str) {
    if is_ws_connected {
        (Color32::from_rgb(50, 205, 50), "●")
    } else if client.connected && recently_active(client) {
        (Color32::from_rgb(255, 200, 0), "⚠")
    } else {
        (Color32::from_rgb(110, 110, 118), "⊗")
    }
}

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
                    let (indicator_color, indicator_text) = connection_indicator(is_ws_connected, client);
                    
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

                    // Pre-compute the friendly fields the rich hover panel
                    // needs (does the same date parse + user resolve as
                    // before, just done once and surfaced through the
                    // panel rather than a single-line tooltip).
                    let parsed_date = DateTime::parse_from_rfc3339(
                        &client.last_update.clone().unwrap_or(Utc::now().into()).to_string()
                    )
                    .unwrap_or_default()
                    .with_timezone(&Local);
                    let formatted_date = parsed_date.format("%Y/%m/%d @ %I:%M%p").to_string();
                    let assigned_user_text = if let Some(ref user_id) = client.assigned_user {
                        let users = get_database_users();
                        users.iter()
                            .find(|u| u.get_id().key_string() == user_id.key_string())
                            .map(|u| u.get_name().to_string())
                            .unwrap_or_else(|| user_id.key_string().to_string())
                    } else {
                        "(none)".to_string()
                    };

                    // The hover panel exposes everything an admin needs
                    // to identify the machine without opening it: raw
                    // connection_string, friendly name, direct-TCP
                    // address (when published), DB linkage, and a hint
                    // pointing at the 🔗 button for the re-link flow.
                    let client_for_hover = client.clone();
                    let formatted_date_h = formatted_date.clone();
                    let assigned_user_h = assigned_user_text.clone();
                    let _ = Button::new(formatted_text)
                        .ui(ui)
                        .on_hover_ui(|ui| {
                            client_hover_panel(
                                ui,
                                &client_for_hover,
                                &formatted_date_h,
                                &assigned_user_h,
                                is_ws_connected,
                            );
                        });
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

                    // Re-link customer button. Opens a popup where the
                    // admin searches by phone / email / order # and
                    // commits a manual customer binding (sets
                    // `customer_locked` so the OA-key auto-detection
                    // stops overwriting it on reconnect).
                    let relink_color = if client.customer_locked {
                        Color32::from_rgb(120, 200, 255) // distinct cue when already locked
                    } else {
                        Color32::from_rgb(199, 202, 245)
                    };
                    let relink_glyph = if client.customer_locked { "🔗" } else { "🔍" };
                    let relink = Button::new(RichText::new(relink_glyph).strong().color(relink_color))
                        .fill(ui.style().visuals.window_fill)
                        .min_size(Vec2::new(30., 30.))
                        .ui(ui)
                        .on_hover_text(if client.customer_locked {
                            "Customer is locked (manually re-linked).\nClick to change linkage."
                        } else {
                            "Re-link to a different customer\n(used-machine-was-our-customer fix)"
                        });
                    if relink.clicked() {
                        let _ = tx.try_send(ClientUiAction::RelinkCustomer(client.clone()));
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

/// Rich hover-panel for a client button. Shows everything an admin needs
/// to identify a machine without having to open it: raw connection
/// string (the auto-derived hostname:hash), friendly_name (and whether
/// it's locked), direct-TCP advertise address (when published), and the
/// usual last-update / assigned-user / customer / computer linkages.
///
/// The previous tooltip was a single string with date+user; this is the
/// place to add anything else admins keep asking for.
fn client_hover_panel(
    ui: &mut Ui,
    client: &ConnectedClient,
    formatted_date: &str,
    assigned_user: &str,
    is_ws_connected: bool,
) {
    use eframe::egui::Grid;

    ui.set_max_width(420.);

    // Header line: friendly name (if any) + lock indicator
    if let Some(fname) = client.friendly_name.as_deref() {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(fname)
                    .strong()
                    .color(Color32::from_rgb(51, 255, 189)),
            );
            if client.customer_locked {
                ui.label(
                    RichText::new("🔒 locked")
                        .small()
                        .color(Color32::from_rgb(120, 200, 255)),
                )
                .on_hover_text(
                    "Customer was manually re-linked. \
                     OA-key auto-detection won't overwrite it.",
                );
            }
        });
        ui.add_space(2.);
    }

    Grid::new(("client_hover_panel", &client.connection_string))
        .num_columns(2)
        .spacing(eframe::egui::Vec2::new(10., 2.))
        .show(ui, |ui| {
            row(ui, "Connection", &client.connection_string);
            row(ui, "Last update", formatted_date);
            row(
                ui,
                "Status",
                if is_ws_connected {
                    "● connected (active session)"
                } else if client.connected && recently_active(client) {
                    "⚠ online — no active admin session"
                } else if client.connected {
                    "⊗ stale — DB still connected but no heartbeat for >5 min"
                } else {
                    "⊗ disconnected"
                },
            );
            row(ui, "Assigned to", assigned_user);

            match (client.local_ip.as_deref(), client.tcp_port) {
                (Some(ip), Some(port)) if !ip.is_empty() => {
                    row(ui, "Direct TCP", &format!("{ip}:{port}"));
                }
                _ => {
                    row(ui, "Direct TCP", "(not advertised — relay only)");
                }
            }

            row(
                ui,
                "Customer",
                client
                    .customer
                    .as_ref()
                    .map(|c| c.key_string().to_string())
                    .unwrap_or_else(|| "(none)".into())
                    .as_str(),
            );
            row(
                ui,
                "Computer",
                client
                    .computer
                    .as_ref()
                    .map(|c| c.key_string().to_string())
                    .unwrap_or_else(|| "(none)".into())
                    .as_str(),
            );

            if let Some(created) = client.created_at.as_ref() {
                row(ui, "Created", &created.to_string());
            }
        });

    ui.add_space(6.);
    ui.label(
        RichText::new(
            if client.customer_locked {
                "Click 🔗 to change linkage."
            } else {
                "Click 🔍 to re-link to a different customer."
            },
        )
        .small()
        .color(Color32::GRAY),
    );
}

fn row(ui: &mut Ui, key: &str, val: &str) {
    ui.label(RichText::new(key).small().color(Color32::GRAY));
    ui.label(RichText::new(val).small());
    ui.end_row();
}