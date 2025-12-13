//! Client card component for displaying individual connected clients.
//!
//! Displays:
//! - Hostname / friendly name
//! - Assigned user
//! - Connection status with color indicator
//! - Last ping time
//! - Computer specs summary
//! - Action buttons (Delete, TUR, Shell, Explorer)

use super::{ConnectionState, ShellType, WebConsoleAction};
use crossbeam::channel::Sender;
use database::schema::{ComputerData, ConnectedClient, User};
use eframe::egui::{
    Align, Button, Color32, Frame, Layout, Margin, Response, RichText, Rounding, Sense, Stroke,
    Ui, Vec2,
};
use chrono::{DateTime, Local, Utc};
use std::collections::HashMap;

/// A card component displaying a single connected client
pub struct ClientCard;

impl ClientCard {
    /// Render a client card
    ///
    /// Returns true if the card was clicked (for selection)
    pub fn show(
        ui: &mut Ui,
        client: &ConnectedClient,
        connection_state: ConnectionState,
        last_pong_secs: Option<u64>,
        user_cache: &HashMap<String, User>,
        computer_cache: &HashMap<String, ComputerData>,
        action_tx: &Sender<WebConsoleAction>,
        is_selected: bool,
    ) -> bool {
        let mut clicked = false;
        let style = ui.style().clone();

        // Card frame styling
        let bg_color = if is_selected {
            Color32::from_rgb(30, 35, 45)
        } else {
            Color32::from_rgb(20, 22, 28)
        };

        let stroke = if is_selected {
            Stroke::new(2.0, Color32::from_rgb(100, 149, 237)) // Cornflower blue
        } else {
            Stroke::new(1.0, Color32::from_rgb(50, 55, 65))
        };

        Frame::none()
            .fill(bg_color)
            .stroke(stroke)
            .rounding(Rounding::same(8.0))
            .inner_margin(Margin::same(12.0))
            .outer_margin(Margin::same(4.0))
            .show(ui, |ui| {
                ui.set_min_width(280.0);
                ui.set_max_width(320.0);

                // Make the whole card clickable
                let card_response = ui.interact(
                    ui.available_rect_before_wrap(),
                    ui.id().with("card"),
                    Sense::click(),
                );
                if card_response.clicked() {
                    clicked = true;
                }

                // Header row: Status indicator + Name
                ui.horizontal(|ui| {
                    // Connection status indicator (colored circle)
                    let status_color = connection_state.color();
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                    ui.painter().circle_filled(rect.center(), 6.0, status_color);

                    ui.add_space(8.0);

                    // Client name
                    let name = client
                        .friendly_name
                        .clone()
                        .unwrap_or_else(|| {
                            // Parse connection string for display
                            client
                                .connection_string
                                .split(':')
                                .next()
                                .unwrap_or(&client.connection_string)
                                .to_string()
                        });

                    ui.label(
                        RichText::new(&name)
                            .size(14.0)
                            .strong()
                            .color(Color32::from_rgb(220, 225, 235)),
                    );

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        // Pong indicator
                        if let Some(secs) = last_pong_secs {
                            let pong_color = if secs < 10 {
                                Color32::from_rgb(50, 205, 50) // Green
                            } else if secs < 20 {
                                Color32::YELLOW
                            } else {
                                Color32::from_rgb(220, 20, 60) // Red
                            };
                            ui.label(
                                RichText::new(format!("{}s", secs))
                                    .size(10.0)
                                    .color(pong_color),
                            );
                        }
                    });
                });

                ui.add_space(4.0);

                // Connection string (smaller, muted)
                ui.label(
                    RichText::new(&client.connection_string)
                        .size(10.0)
                        .color(Color32::from_rgb(130, 135, 145)),
                );

                ui.add_space(8.0);

                // Assigned user row
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("User:")
                            .size(11.0)
                            .color(Color32::from_rgb(160, 165, 175)),
                    );

                    let user_name = client
                        .assigned_user
                        .as_ref()
                        .and_then(|id| user_cache.get(&id.to_string()))
                        .map(|u| u.get_username().to_string())
                        .unwrap_or_else(|| "Unassigned".to_string());

                    ui.label(
                        RichText::new(&user_name)
                            .size(11.0)
                            .color(Color32::from_rgb(51, 255, 189)),
                    );
                });

                // Computer specs summary (if available)
                if let Some(computer) = computer_cache.get(&client.connection_string) {
                    ui.add_space(4.0);
                    Self::render_specs_summary(ui, computer);
                }

                // Last update time
                if let Some(last_update) = &client.last_update {
                    ui.add_space(4.0);
                    let parsed = DateTime::parse_from_rfc3339(&last_update.to_string())
                        .map(|dt| dt.with_timezone(&Local))
                        .ok();
                    
                    if let Some(dt) = parsed {
                        let formatted = dt.format("%m/%d %I:%M%p").to_string();
                        ui.label(
                            RichText::new(format!("Last seen: {}", formatted))
                                .size(10.0)
                                .color(Color32::from_rgb(100, 105, 115)),
                        );
                    }
                }

                ui.add_space(12.0);

                // Action buttons row
                ui.horizontal(|ui| {
                    let btn_size = Vec2::new(32.0, 28.0);

                    // Connect/Disconnect button
                    let (connect_icon, connect_tooltip) = match connection_state {
                        ConnectionState::Connected | ConnectionState::Stale => ("⏹", "Disconnect"),
                        ConnectionState::Connecting => ("⏳", "Connecting..."),
                        ConnectionState::Disconnected => ("▶", "Connect"),
                    };

                    let connect_btn = Button::new(
                        RichText::new(connect_icon)
                            .size(14.0)
                            .color(connection_state.color()),
                    )
                    .min_size(btn_size)
                    .fill(Color32::from_rgb(35, 40, 50));

                    if ui.add(connect_btn).on_hover_text(connect_tooltip).clicked() {
                        match connection_state {
                            ConnectionState::Connected | ConnectionState::Stale => {
                                let _ = action_tx.send(WebConsoleAction::DisconnectClient(
                                    client.connection_string.clone(),
                                ));
                            }
                            ConnectionState::Disconnected => {
                                let _ = action_tx.send(WebConsoleAction::ConnectClient(client.clone()));
                            }
                            _ => {}
                        }
                    }

                    ui.add_space(4.0);

                    // Shell button with dropdown for shell type
                    let shell_btn = Button::new(
                        RichText::new("🖥")
                            .size(14.0)
                            .color(Color32::from_rgb(100, 200, 255)),
                    )
                    .min_size(btn_size)
                    .fill(Color32::from_rgb(35, 40, 50));

                    let shell_response = ui.add(shell_btn).on_hover_text("Remote Shell");
                    
                    // Show shell type menu on click
                    if shell_response.clicked() {
                        // Default to PowerShell
                        let _ = action_tx.send(WebConsoleAction::OpenShell(
                            client.clone(),
                            ShellType::PowerShell,
                        ));
                    }
                    
                    shell_response.context_menu(|ui| {
                        if ui.button("PowerShell").clicked() {
                            let _ = action_tx.send(WebConsoleAction::OpenShell(
                                client.clone(),
                                ShellType::PowerShell,
                            ));
                            ui.close_menu();
                        }
                        if ui.button("CMD").clicked() {
                            let _ = action_tx.send(WebConsoleAction::OpenShell(
                                client.clone(),
                                ShellType::Cmd,
                            ));
                            ui.close_menu();
                        }
                    });

                    ui.add_space(4.0);

                    // File Explorer button
                    let explorer_btn = Button::new(
                        RichText::new("📁")
                            .size(14.0)
                            .color(Color32::from_rgb(255, 200, 100)),
                    )
                    .min_size(btn_size)
                    .fill(Color32::from_rgb(35, 40, 50));

                    if ui.add(explorer_btn).on_hover_text("File Explorer").clicked() {
                        let _ = action_tx.send(WebConsoleAction::OpenFileExplorer(client.clone()));
                    }

                    ui.add_space(4.0);

                    // Create TUR button
                    let tur_btn = Button::new(
                        RichText::new("📋")
                            .size(14.0)
                            .color(Color32::from_rgb(150, 255, 150)),
                    )
                    .min_size(btn_size)
                    .fill(Color32::from_rgb(35, 40, 50));

                    if ui.add(tur_btn).on_hover_text("Create TUR Sheet").clicked() {
                        let _ = action_tx.send(WebConsoleAction::OpenTurModal(client.clone()));
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        // Delete button (right-aligned, danger color)
                        let delete_btn = Button::new(
                            RichText::new("🗑")
                                .size(14.0)
                                .color(Color32::from_rgb(255, 100, 100)),
                        )
                        .min_size(btn_size)
                        .fill(Color32::from_rgb(45, 30, 35));

                        if ui.add(delete_btn).on_hover_text("Delete Client").clicked() {
                            let _ = action_tx.send(WebConsoleAction::DeleteClient(client.clone()));
                        }
                    });
                });
            });

        clicked
    }

    /// Render a compact specs summary
    fn render_specs_summary(ui: &mut Ui, computer: &ComputerData) {
        ui.horizontal(|ui| {
            // OS
            if !computer.operating_system.is_empty() {
                let os_short = computer
                    .operating_system
                    .split_whitespace()
                    .take(3)
                    .collect::<Vec<_>>()
                    .join(" ");
                ui.label(
                    RichText::new(&os_short)
                        .size(9.0)
                        .color(Color32::from_rgb(180, 180, 200)),
                );
                ui.label(RichText::new("•").size(9.0).color(Color32::DARK_GRAY));
            }

            // RAM
            if !computer.ram.is_empty() {
                ui.label(
                    RichText::new(&computer.ram)
                        .size(9.0)
                        .color(Color32::from_rgb(180, 180, 200)),
                );
            }
        });

        // CPU on second line
        if !computer.cpu.is_empty() {
            let cpu_short = if computer.cpu.len() > 35 {
                format!("{}...", &computer.cpu[..32])
            } else {
                computer.cpu.clone()
            };
            ui.label(
                RichText::new(&cpu_short)
                    .size(9.0)
                    .color(Color32::from_rgb(150, 155, 165)),
            );
        }
    }

    /// Render a compact list-style row instead of a card
    pub fn show_list_row(
        ui: &mut Ui,
        client: &ConnectedClient,
        connection_state: ConnectionState,
        last_pong_secs: Option<u64>,
        user_cache: &HashMap<String, User>,
        action_tx: &Sender<WebConsoleAction>,
        is_selected: bool,
    ) -> bool {
        let mut clicked = false;

        let bg_color = if is_selected {
            Color32::from_rgb(30, 35, 45)
        } else {
            Color32::TRANSPARENT
        };

        Frame::none()
            .fill(bg_color)
            .inner_margin(Margin::symmetric(8.0, 4.0))
            .show(ui, |ui| {
                let response = ui.horizontal(|ui| {
                    // Status indicator
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                    ui.painter()
                        .circle_filled(rect.center(), 5.0, connection_state.color());

                    ui.add_space(8.0);

                    // Name
                    let name = client
                        .friendly_name
                        .clone()
                        .unwrap_or_else(|| client.connection_string.clone());
                    ui.label(RichText::new(&name).size(12.0).strong());

                    ui.add_space(16.0);

                    // User
                    let user_name = client
                        .assigned_user
                        .as_ref()
                        .and_then(|id| user_cache.get(&id.to_string()))
                        .map(|u| u.get_username().to_string())
                        .unwrap_or_else(|| "-".to_string());
                    ui.label(
                        RichText::new(&user_name)
                            .size(11.0)
                            .color(Color32::from_rgb(51, 255, 189)),
                    );

                    ui.add_space(16.0);

                    // Pong time
                    if let Some(secs) = last_pong_secs {
                        let color = if secs < 10 {
                            Color32::from_rgb(50, 205, 50)
                        } else if secs < 20 {
                            Color32::YELLOW
                        } else {
                            Color32::RED
                        };
                        ui.label(RichText::new(format!("{}s", secs)).size(10.0).color(color));
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        // Quick action buttons
                        if ui.small_button("🗑").on_hover_text("Delete").clicked() {
                            let _ = action_tx.send(WebConsoleAction::DeleteClient(client.clone()));
                        }
                        if ui.small_button("📋").on_hover_text("TUR").clicked() {
                            let _ = action_tx.send(WebConsoleAction::OpenTurModal(client.clone()));
                        }
                        if ui.small_button("🖥").on_hover_text("Shell").clicked() {
                            let _ = action_tx.send(WebConsoleAction::OpenShell(
                                client.clone(),
                                ShellType::PowerShell,
                            ));
                        }
                        if ui.small_button("📁").on_hover_text("Files").clicked() {
                            let _ = action_tx.send(WebConsoleAction::OpenFileExplorer(client.clone()));
                        }
                    });
                });

                if response.response.interact(Sense::click()).clicked() {
                    clicked = true;
                }
            });

        ui.separator();
        clicked
    }
}

