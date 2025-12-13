//! Client grid component for displaying all connected clients.
//!
//! Supports both grid and list view modes with filtering and search.

use super::{ClientCard, ClientFilter, ViewMode, WebConsole, WebConsoleAction};
use eframe::egui::{
    Align, Button, Color32, ComboBox, CornerRadius, Frame, Layout, Margin, RichText, ScrollArea,
    TextEdit, Ui, Vec2,
};

/// Grid/List view component for displaying clients
pub struct ClientGrid;

impl ClientGrid {
    /// Render the client grid with toolbar
    pub fn show(ui: &mut Ui, console: &mut WebConsole) {
        // Top toolbar
        Self::render_toolbar(ui, console);

        ui.add_space(8.0);

        // Main content area
        let filtered = console.filtered_clients();

        if console.loading {
            ui.centered_and_justified(|ui| {
                ui.spinner();
                ui.label("Loading clients...");
            });
            return;
        }

        if filtered.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(
                    RichText::new("No clients found")
                        .size(16.0)
                        .color(Color32::from_rgb(150, 155, 165)),
                );
            });
            return;
        }

        // Clone needed data to avoid borrow conflicts
        let view_mode = console.view_mode;
        
        // Render clients based on view mode
        match view_mode {
            ViewMode::Grid => Self::render_grid(ui, console),
            ViewMode::List => Self::render_list(ui, console),
        }
    }

    /// Render the toolbar with search, filter, and view mode controls
    fn render_toolbar(ui: &mut Ui, console: &mut WebConsole) {
        Frame::NONE
            .fill(Color32::from_rgb(25, 28, 35))
            .inner_margin(Margin::symmetric(8, 12))
            .corner_radius(CornerRadius::same(6))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    // Refresh button
                    let refresh_btn = Button::new(
                        RichText::new("🔄")
                            .size(14.0)
                            .color(Color32::from_rgb(100, 180, 255)),
                    )
                    .min_size(Vec2::new(32.0, 28.0));

                    if ui.add(refresh_btn).on_hover_text("Refresh").clicked() {
                        let _ = console.action_tx.send(WebConsoleAction::RefreshClients);
                    }

                    ui.add_space(12.0);

                    // Search input
                    ui.label(RichText::new("🔍").size(14.0));
                    let search_edit = TextEdit::singleline(&mut console.search_query)
                        .hint_text("Search clients...")
                        .desired_width(200.0);
                    ui.add(search_edit);

                    ui.add_space(16.0);

                    // Filter dropdown
                    ui.label(
                        RichText::new("Filter:")
                            .size(11.0)
                            .color(Color32::from_rgb(160, 165, 175)),
                    );

                    ComboBox::from_id_salt("client_filter")
                        .selected_text(match console.filter {
                            ClientFilter::All => "All",
                            ClientFilter::Connected => "Connected",
                            ClientFilter::Disconnected => "Disconnected",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut console.filter, ClientFilter::All, "All");
                            ui.selectable_value(
                                &mut console.filter,
                                ClientFilter::Connected,
                                "Connected",
                            );
                            ui.selectable_value(
                                &mut console.filter,
                                ClientFilter::Disconnected,
                                "Disconnected",
                            );
                        });

                    ui.add_space(8.0);
                    
                    // Hide stale clients toggle
                    ui.checkbox(&mut console.hide_stale_clients, "Hide stale")
                        .on_hover_text("Hide disconnected clients older than 4 hours");

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        // View mode toggle
                        let grid_btn = Button::new(
                            RichText::new("▦")
                                .size(16.0)
                                .color(if console.view_mode == ViewMode::Grid {
                                    Color32::WHITE
                                } else {
                                    Color32::GRAY
                                }),
                        )
                        .min_size(Vec2::new(28.0, 28.0))
                        .fill(if console.view_mode == ViewMode::Grid {
                            Color32::from_rgb(60, 100, 150)
                        } else {
                            Color32::TRANSPARENT
                        });

                        if ui.add(grid_btn).on_hover_text("Grid view").clicked() {
                            console.view_mode = ViewMode::Grid;
                        }

                        let list_btn = Button::new(
                            RichText::new("≡")
                                .size(16.0)
                                .color(if console.view_mode == ViewMode::List {
                                    Color32::WHITE
                                } else {
                                    Color32::GRAY
                                }),
                        )
                        .min_size(Vec2::new(28.0, 28.0))
                        .fill(if console.view_mode == ViewMode::List {
                            Color32::from_rgb(60, 100, 150)
                        } else {
                            Color32::TRANSPARENT
                        });

                        if ui.add(list_btn).on_hover_text("List view").clicked() {
                            console.view_mode = ViewMode::List;
                        }

                        ui.add_space(16.0);

                        // Client count - use filtered clients for accuracy
                        let filtered = console.filtered_clients();
                        let visible_total = filtered.len();
                        let visible_connected = filtered.iter().filter(|c| c.connected).count();
                        let db_total = console.clients.len();
                        
                        ui.label(
                            RichText::new(format!("{}/{} online ({} shown)", visible_connected, db_total, visible_total))
                                .size(11.0)
                                .color(Color32::from_rgb(51, 255, 189)),
                        );

                        // Last refresh time
                        if let Some(last_refresh) = &console.last_refresh {
                            let elapsed = last_refresh.elapsed().as_secs();
                            let time_str = if elapsed < 60 {
                                format!("{}s ago", elapsed)
                            } else {
                                format!("{}m ago", elapsed / 60)
                            };
                            ui.label(
                                RichText::new(format!("Updated: {}", time_str))
                                    .size(10.0)
                                    .color(Color32::from_rgb(120, 125, 135)),
                            );
                        }
                    });
                });
            });
    }

    /// Render clients in a grid layout
    fn render_grid(ui: &mut Ui, console: &mut WebConsole) {
        // Collect all needed data upfront to avoid borrow conflicts
        let filtered: Vec<_> = console.filtered_clients().into_iter().cloned().collect();
        
        // Calculate cards per row based on available width
        let available_width = ui.available_width();
        let card_width = 300.0; // Approximate card width including margins
        let cards_per_row = ((available_width / card_width).floor() as usize).max(1);

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Use a grid layout for proper wrapping
                eframe::egui::Grid::new("client_grid")
                    .num_columns(cards_per_row)
                    .spacing([8.0, 8.0])
                    .show(ui, |ui| {
                        for (idx, client) in filtered.iter().enumerate() {
                            let conn_string = &client.connection_string;

                            // Get connection state
                            let connection_state = console
                                .connections
                                .get(conn_string)
                                .map(|m| m.state)
                                .unwrap_or_else(|| {
                                    if client.connected {
                                        super::ConnectionState::Disconnected // DB says connected but no WS
                                    } else {
                                        super::ConnectionState::Disconnected
                                    }
                                });

                            // Get last pong time
                            let last_pong_secs = console
                                .get_last_pong_elapsed(conn_string)
                                .map(|d| d.as_secs());

                            let is_selected = console
                                .selected_client
                                .as_ref()
                                .map(|s| s == conn_string)
                                .unwrap_or(false);

                            if ClientCard::show(
                                ui,
                                &client,
                                connection_state,
                                last_pong_secs,
                                &console.user_cache,
                                &console.computer_cache,
                                &console.action_tx,
                                is_selected,
                            ) {
                                console.selected_client = Some(conn_string.clone());
                            }
                            
                            // End row after cards_per_row cards
                            if (idx + 1) % cards_per_row == 0 {
                                ui.end_row();
                            }
                        }
                    });
            });
    }

    /// Render clients in a list layout
    fn render_list(ui: &mut Ui, console: &mut WebConsole) {
        // Collect all needed data upfront to avoid borrow conflicts
        let filtered: Vec<_> = console.filtered_clients().into_iter().cloned().collect();

        // Header row
        Frame::NONE
            .fill(Color32::from_rgb(30, 33, 40))
            .inner_margin(Margin::symmetric(6, 8))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(18.0); // For status indicator
                    ui.label(
                        RichText::new("Name")
                            .size(11.0)
                            .strong()
                            .color(Color32::from_rgb(180, 185, 195)),
                    );
                    ui.add_space(120.0);
                    ui.label(
                        RichText::new("User")
                            .size(11.0)
                            .strong()
                            .color(Color32::from_rgb(180, 185, 195)),
                    );
                    ui.add_space(80.0);
                    ui.label(
                        RichText::new("Last Update")
                            .size(11.0)
                            .strong()
                            .color(Color32::from_rgb(180, 185, 195)),
                    );
                    ui.add_space(60.0);
                    ui.label(
                        RichText::new("Ping")
                            .size(11.0)
                            .strong()
                            .color(Color32::from_rgb(180, 185, 195)),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new("Actions")
                                .size(11.0)
                                .strong()
                                .color(Color32::from_rgb(180, 185, 195)),
                        );
                    });
                });
            });

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for client in filtered.iter() {
                    let conn_string = &client.connection_string;

                    let connection_state = console
                        .connections
                        .get(conn_string)
                        .map(|m| m.state)
                        .unwrap_or(super::ConnectionState::Disconnected);

                    let last_pong_secs = console
                        .get_last_pong_elapsed(conn_string)
                        .map(|d| d.as_secs());

                    let is_selected = console
                        .selected_client
                        .as_ref()
                        .map(|s| s == conn_string)
                        .unwrap_or(false);

                    if ClientCard::show_list_row(
                        ui,
                        client,
                        connection_state,
                        last_pong_secs,
                        &console.user_cache,
                        &console.action_tx,
                        is_selected,
                    ) {
                        console.selected_client = Some(conn_string.clone());
                    }
                }
            });
    }
}

