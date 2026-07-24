use eframe::egui::Shadow;
use eframe::egui::{
    collapsing_header::CollapsingState, text::LayoutJob, Align, Button, Color32, FontFamily,
    FontId, Frame, Grid, Layout, Margin, RichText, TextFormat, Ui, Vec2, Widget, WidgetText,
};
use database::schema::{ConnectedClient, RecordIdExt};
use std::collections::HashMap;
use crossbeam::channel::Sender;
use chrono::{DateTime, Local, Utc};
use super::ClientUiAction;
use super::SessionLayout;
use crate::get_database_users;
use crate::ui_tools::{icons, theme};
use crate::{PlatformSpawner, Spawner};
use log::info;

use super::{AdminConsole, RightPanel};

/// How stale a `last_update` timestamp can be before we treat the client as
/// offline — even if the DB `connected` flag is still `true`. Five minutes
/// is conservative; the client heartbeats every ~30 s so anything older than
/// that is either crashed or unreachable.
const STALE_THRESHOLD_SECS: i64 = 300;

/// Uniform row height for the buttons inside a client row. All action
/// buttons and the name button are sized to this so the row is visually
/// rectangular regardless of glyph metrics.
const ROW_BTN_H: f32 = 25.0;
const ROW_BTN_W: f32 = 25.0;
const ROW_STATUS_W: f32 = 16.0;
const ROW_ITEM_GAP: f32 = 8.0;

/// Fixed inner width for one client row in the side panel (fits 400px panel margins).
pub const CLIENT_ROW_CONTENT_W: f32 = 368.0;

const ROW_HEADER_CHROME_W: f32 =
    ROW_BTN_W * 3.0 + ROW_STATUS_W + ROW_ITEM_GAP * 4.0;
pub const CLIENT_NAME_BTN_W: f32 = CLIENT_ROW_CONTENT_W - ROW_HEADER_CHROME_W;
const CLIENT_DETAILS_VALUE_W: f32 = CLIENT_ROW_CONTENT_W - 90.0;

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
/// 1. Active admin session open → green `*`
/// 2. DB-connected + recent heartbeat + no admin session → yellow `!`
/// 3. Everything else (disconnected or stale) → gray `-`
fn connection_indicator(is_ws_connected: bool, client: &ConnectedClient) -> (Color32, &'static str) {
    if is_ws_connected {
        (Color32::from_rgb(50, 205, 50), icons::STATUS_ON)
    } else if client.connected && recently_active(client) {
        (Color32::from_rgb(255, 200, 0), icons::STATUS_WARN)
    } else {
        (Color32::from_rgb(110, 110, 118), icons::STATUS_OFF)
    }
}

/// Compose the friendly-name / connection-string text for a client into a
/// styled `WidgetText`. Mirrors the original two-tone behaviour: friendly
/// name in mint green, or `hostname:hash` with the hostname in mint and
/// the hash in muted lavender.
fn client_name_text(client: &ConnectedClient) -> WidgetText {
    let mut job = LayoutJob::default();
    if let Some(ref friendly_name) = client.friendly_name {
        job.append(
            friendly_name,
            0.0,
            TextFormat {
                font_id: FontId::new(13., FontFamily::Proportional),
                color: Color32::from_rgb(51, 255, 189),
                valign: Align::Center,
                ..Default::default()
            },
        );
    } else if let Some((host, hash)) = client.connection_string.split_once(':') {
        job.append(
            &format!("{host}:"),
            0.0,
            TextFormat {
                font_id: FontId::new(13., FontFamily::Proportional),
                color: Color32::from_rgb(51, 255, 189),
                valign: Align::Center,
                ..Default::default()
            },
        );
        job.append(
            hash,
            0.0,
            TextFormat {
                font_id: FontId::new(13., FontFamily::Proportional),
                color: Color32::from_rgb(199, 202, 245),
                valign: Align::Center,
                ..Default::default()
            },
        );
    } else {
        job.append(
            &client.connection_string,
            0.0,
            TextFormat {
                font_id: FontId::new(13., FontFamily::Proportional),
                color: Color32::from_rgb(51, 255, 189),
                valign: Align::Center,
                ..Default::default()
            },
        );
    }
    WidgetText::from(job)
}

impl AdminConsole {
    /// Render a single client row in the connected-clients list.
    ///
    /// **Layout (always visible):** uniform-height strip of
    /// `[chevron] [status dot] [name button (stretches)] [focus] [connect]`.
    ///
    /// **Layout (when expanded):** below the strip the row reveals a
    /// details grid (connection_string, last update, customer/computer
    /// linkage, direct-TCP, etc.) plus the secondary action buttons —
    /// **Disconnect**, **Re-link**, **Dock / Float** — which used to be
    /// pinned to the row at all times. Tucking them behind the chevron
    /// keeps the collapsed row compact and prevents accidental clicks on
    /// destructive actions (notably Disconnect).
    ///
    /// State persistence: the open/closed state lives in egui memory
    /// keyed by `connection_string`, so the caller doesn't need to thread
    /// any new state through.
    pub fn client_header(
        ui: &mut Ui,
        tx: Sender<ClientUiAction>,
        client: &ConnectedClient,
        session_layout: HashMap<String, SessionLayout>,
        focused_client: Option<&str>,
        is_ws_connected: bool,
        fk_health_tx: &crossbeam::channel::Sender<(String, bool, bool)>,
        fk_health_cache: &HashMap<String, (bool, bool)>,
        // Slice 2: latest gathered security inventory for this
        // client, or `None` if none has arrived (yet) this session.
        // Rendered as an extra section in the expanded body.
        security_inventory: Option<&[database::schema::InstalledSecurityProduct]>,
        // Slice 5 (post-bugfix): last reachability probe result
        // for this client, or `None` if the prober hasn't gotten
        // to it yet. Surfaced as informational metadata in the
        // details grid — does *not* gate visibility.
        reachability: Option<&crate::ui_data::reachability::ReachabilityStatus>,
        // Live transport path of the open session `(kind, is_connected)`,
        // or `None` when no session entry exists for this client.
        transport: Option<(super::client_interface::TransportKind, bool)>,
    ) {
        let style = ui.style().clone();
        let row_id = ui.make_persistent_id((
            "admin_client_row",
            client.connection_string.as_str(),
        ));
        let mut collapse = CollapsingState::load_with_default_open(ui.ctx(), row_id, false);
        if collapse.is_open() {
            queue_fk_health_check(fk_health_tx, fk_health_cache, client);
        }
        let fk_health = fk_health_cache.get(&client.connection_string).copied();

        // Pre-compute the fields the details grid needs, so the body
        // closure doesn't need to re-parse on every frame.
        let parsed_date = DateTime::parse_from_rfc3339(
            &client
                .last_update
                .clone()
                .unwrap_or(Utc::now().into())
                .to_string(),
        )
        .unwrap_or_default()
        .with_timezone(&Local);
        let formatted_date = parsed_date.format("%Y/%m/%d @ %I:%M%p").to_string();
        let assigned_user_text = if let Some(ref user_id) = client.assigned_user {
            let users = get_database_users();
            users
                .iter()
                .find(|u| u.get_id().key_string() == user_id.key_string())
                .map(|u| u.get_name().to_string())
                .unwrap_or_else(|| user_id.key_string().to_string())
        } else {
            "(none)".to_string()
        };

        ui.allocate_ui_with_layout(
            Vec2::new(CLIENT_ROW_CONTENT_W, 0.0),
            Layout::top_down(Align::Min),
            |ui| {
                Frame::default()
                .fill(theme::bg_surface(ui))
                .inner_margin(Margin::same(4))
                .outer_margin(Margin::ZERO)
                .corner_radius(eframe::egui::CornerRadius::same(5))
                .shadow(Shadow::NONE)
                .stroke(style.visuals.window_stroke)
                .show(ui, |ui| {
                    ui.set_width(CLIENT_ROW_CONTENT_W);
                    ui.set_max_width(CLIENT_ROW_CONTENT_W);
                    let is_focused = focused_client == Some(client.connection_string.as_str());
                    let focus_color = if is_focused {
                        Color32::from_rgb(51, 255, 189)
                    } else {
                        Color32::GRAY
                    };
                    let (indicator_color, indicator_text) =
                        connection_indicator(is_ws_connected, client);
                    let arrow = if collapse.is_open() {
                        icons::CHEV_OPEN
                    } else {
                        icons::CHEV_CLOSED
                    };

                    ui.allocate_ui_with_layout(
                        Vec2::new(CLIENT_ROW_CONTENT_W, ROW_BTN_H),
                        Layout::left_to_right(Align::Center),
                        |ui| {
                            ui.spacing_mut().item_spacing.x = ROW_ITEM_GAP;
                            let chevron = ui
                                .add_sized(
                                    Vec2::new(ROW_BTN_W, ROW_BTN_H),
                                    Button::new(RichText::new(arrow).strong())
                                        .fill(ui.style().visuals.window_fill),
                                )
                                .on_hover_text(if collapse.is_open() {
                                    "Collapse client details"
                                } else {
                                    "Expand client details & secondary actions"
                                });
                            if chevron.clicked() {
                                collapse.toggle(ui);
                            }

                            ui.add_sized(
                                Vec2::new(ROW_STATUS_W, ROW_BTN_H),
                                eframe::egui::Label::new(
                                    RichText::new(indicator_text).color(indicator_color),
                                ),
                            );

                            // Transport badge for the open session's live path.
                            if let Some((kind, session_up)) = transport {
                                use super::client_interface::TransportKind;
                                let (badge, tip) = match kind {
                                    TransportKind::Tcp => ("TCP", "Direct TCP (same network)"),
                                    TransportKind::Relay => ("RELAY", "Relay tunnel via websocket server"),
                                    TransportKind::WebSocket => ("WS", "Legacy WebSocket relay room"),
                                };
                                let color = if session_up {
                                    match kind {
                                        TransportKind::Tcp => Color32::from_rgb(100, 200, 100),
                                        TransportKind::Relay => Color32::from_rgb(235, 170, 80),
                                        TransportKind::WebSocket => Color32::from_rgb(120, 160, 230),
                                    }
                                } else {
                                    Color32::GRAY
                                };
                                let tip = if session_up {
                                    tip.to_string()
                                } else {
                                    format!("{tip} — reconnecting")
                                };
                                ui.label(RichText::new(badge).small().strong().color(color))
                                    .on_hover_text(tip);
                            }

                            let name_btn = ui
                                .add_sized(
                                    Vec2::new(CLIENT_NAME_BTN_W, ROW_BTN_H),
                                    Button::new(client_name_text(client))
                                        .fill(ui.style().visuals.window_fill),
                                )
                                .on_hover_text(
                                    "Click to expand client details and secondary actions",
                                );
                            if name_btn.clicked() {
                                collapse.toggle(ui);
                            }

                            let focus_btn = ui
                                .add_sized(
                                    Vec2::new(ROW_BTN_W, ROW_BTN_H),
                                    Button::new(RichText::new(icons::FOCUS).strong().color(focus_color))
                                        .fill(ui.style().visuals.window_fill),
                                )
                                .on_hover_text(if is_focused {
                                    "Focused (receives commands)"
                                } else {
                                    "Set as focused client"
                                });
                            if focus_btn.clicked() {
                                let _ = tx.try_send(ClientUiAction::FocusClient(
                                    client.connection_string.clone(),
                                ));
                            }

                            let connect_btn = ui
                                .add_sized(
                                    Vec2::new(ROW_BTN_W, ROW_BTN_H),
                                    Button::new(
                                        RichText::new(icons::OPEN)
                                            .strong()
                                            .color(ui.style().visuals.warn_fg_color),
                                    )
                                    .fill(ui.style().visuals.window_fill),
                                )
                                .on_hover_text("Open / focus this machine");
                            if connect_btn.clicked() {
                                info!("Sent Connection Command");
                                let _ = tx.try_send(ClientUiAction::ConnectClient(client.clone()));
                            }
                        },
                    );

                    // ── Expanded body ─────────────────────────────────────────────
                    if collapse.is_open() {
                        ui.add_space(6.);
                        ui.separator();
                        ui.add_space(4.);

                        client_details_grid(
                            ui,
                            &tx,
                            client,
                            &formatted_date,
                            &assigned_user_text,
                            is_ws_connected,
                            reachability,
                            fk_health,
                        );

                        if let Some(products) = security_inventory {
                            if !products.is_empty() {
                                ui.add_space(8.);
                                ui.separator();
                                ui.add_space(4.);
                                ui.label(
                                    RichText::new("Security inventory")
                                        .small()
                                        .color(Color32::GRAY),
                                );
                                ui.add_space(2.);
                                render_security_inventory(ui, products);
                            }
                        }

                        ui.add_space(8.);
                        ui.separator();
                        ui.add_space(4.);

                        ui.label(
                            RichText::new("Actions")
                                .small()
                                .color(Color32::GRAY),
                        );
                        ui.add_space(2.);

                        let layout = session_layout
                            .get(client.connection_string.as_str())
                            .copied()
                            .unwrap_or_default();
                        let (float_label, float_tip) = match layout {
                            SessionLayout::Floating => {
                                ("Dock", "Floating (unlocked) — click to dock")
                            }
                            SessionLayout::Docked => {
                                ("Float", "Docked (locked) — click to float")
                            }
                        };
                        let relink_color = if client.customer_locked {
                            theme::info(ui)
                        } else {
                            theme::weak_text(ui)
                        };
                        let (relink_label, relink_tip) = if client.customer_locked {
                            (
                                "Re-link",
                                "Customer is locked (manually re-linked).\nClick to change linkage.",
                            )
                        } else {
                            (
                                "Re-link",
                                "Re-link to a different customer\n(used-machine-was-our-customer fix)",
                            )
                        };

                        ui.scope(|ui| {
                            ui.set_max_width(CLIENT_ROW_CONTENT_W);
                            ui.horizontal_wrapped(|ui| {
                                let disconnect = Button::new(
                                    RichText::new("Disconnect")
                                        .strong()
                                        .color(ui.style().visuals.error_fg_color),
                                )
                                .fill(ui.style().visuals.window_fill)
                                .min_size(Vec2::new(100., ROW_BTN_H))
                                .ui(ui)
                                .on_hover_text(
                                    "Disconnect this client\n\
                                    (closes the local session and removes it from the list;\n\
                                    the connected_client record stays in the database)",
                                );
                                if disconnect.clicked() {
                                    let _ = tx.try_send(ClientUiAction::DisconnectClient(client.clone()));
                                }


                                let float_btn = Button::new(
                                    RichText::new(float_label).strong().color(ui.style().visuals.error_fg_color),
                                )
                                .fill(ui.style().visuals.window_fill)
                                .min_size(Vec2::new(70., ROW_BTN_H))
                                .ui(ui)
                                .on_hover_text(float_tip);
                                if float_btn.clicked() {
                                    let _ = tx.try_send(ClientUiAction::ToggleClientFloat(
                                        client.connection_string.clone(),
                                    ));
                                }
                                let relink = Button::new(
                                    RichText::new(relink_label).strong().color(relink_color),
                                )
                                .fill(ui.style().visuals.window_fill)
                                .min_size(Vec2::new(100., ROW_BTN_H))
                                .ui(ui)
                                .on_hover_text(relink_tip);
                                if relink.clicked() {
                                    let _ = tx.try_send(ClientUiAction::RelinkCustomer(client.clone()));
                                }
                                let link_comp = Button::new(
                                    RichText::new("Link computer").strong(),
                                )
                                .fill(ui.style().visuals.window_fill)
                                .min_size(Vec2::new(110., ROW_BTN_H))
                                .ui(ui)
                                .on_hover_text("Create or repair the computer record for this client");
                                if link_comp.clicked() {
                                    let _ = tx.try_send(ClientUiAction::LinkComputer(client.clone()));
                                }

                                let repair = Button::new(
                                    RichText::new("Repair links").strong(),
                                )
                                .fill(ui.style().visuals.window_fill)
                                .min_size(Vec2::new(110., ROW_BTN_H))
                                .ui(ui)
                                .on_hover_text(
                                    "Cascade-repoint FKs to canonical computer id and fix diagnostic sessions",
                                );
                                if repair.clicked() {
                                    let _ =
                                        tx.try_send(ClientUiAction::RepairAssociations(client.clone()));
                                }
                            });
                        });
                    }
                });
            },
        );

        // Persist the open/closed flip we may have made via `toggle()`.
        collapse.store(ui.ctx());
    }

    /// Render one pre-boot UEFI box in the same collapsible card style as the
    /// connected-client rows. The collapsed title shows `{model} · {last5}`
    /// (product model + last 5 of the OA3/MSDM key); the expanded body
    /// enumerates the full identity. Returns `true` if View was clicked.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn preboot_card(
        ui: &mut Ui,
        serial: &str,
        friendly: &str,
        age_secs: i64,
        kind: &str,
        direct: bool,
    ) -> bool {
        let style = ui.style().clone();
        let row_id = ui.make_persistent_id(("admin_preboot_card", serial));
        let mut collapse = CollapsingState::load_with_default_open(ui.ctx(), row_id, false);

        // Title: product model (first token of the friendly name) + last 5 of
        // the OA3/MSDM key, e.g. "SM-6 · R3K2J".
        let model = friendly.split_whitespace().next().unwrap_or("");
        let key_tail: String = {
            let n = serial.chars().count();
            serial.chars().skip(n.saturating_sub(5)).collect()
        };
        let title = if model.is_empty() {
            key_tail.clone()
        } else {
            format!("{model} · {key_tail}")
        };
        let live = age_secs < 90;
        let (dot_color, dot_text) = if live {
            (Color32::from_rgb(50, 205, 50), icons::STATUS_ON)
        } else {
            (Color32::from_rgb(110, 110, 118), icons::STATUS_OFF)
        };
        let arrow = if collapse.is_open() { icons::CHEV_OPEN } else { icons::CHEV_CLOSED };
        let mut view_clicked = false;

        ui.allocate_ui_with_layout(
            Vec2::new(CLIENT_ROW_CONTENT_W, 0.0),
            Layout::top_down(Align::Min),
            |ui| {
                Frame::default()
                    .fill(theme::bg_surface(ui))
                    .inner_margin(Margin::same(4))
                    .outer_margin(Margin::ZERO)
                    .corner_radius(eframe::egui::CornerRadius::same(5))
                    .shadow(Shadow::NONE)
                    .stroke(style.visuals.window_stroke)
                    .show(ui, |ui| {
                        ui.set_width(CLIENT_ROW_CONTENT_W);
                        ui.set_max_width(CLIENT_ROW_CONTENT_W);
                        // chevron + status + name + View chrome (mirror client rows).
                        let name_w = CLIENT_ROW_CONTENT_W
                            - (ROW_BTN_W * 2.0 + ROW_STATUS_W + ROW_ITEM_GAP * 3.0);
                        ui.allocate_ui_with_layout(
                            Vec2::new(CLIENT_ROW_CONTENT_W, ROW_BTN_H),
                            Layout::left_to_right(Align::Center),
                            |ui| {
                                ui.spacing_mut().item_spacing.x = ROW_ITEM_GAP;
                                let chevron = ui.add_sized(
                                    Vec2::new(ROW_BTN_W, ROW_BTN_H),
                                    Button::new(RichText::new(arrow).strong())
                                        .fill(ui.style().visuals.window_fill),
                                );
                                if chevron.clicked() {
                                    collapse.toggle(ui);
                                }
                                ui.add_sized(
                                    Vec2::new(ROW_STATUS_W, ROW_BTN_H),
                                    eframe::egui::Label::new(
                                        RichText::new(dot_text).color(dot_color),
                                    ),
                                );
                                let name_btn = ui
                                    .add_sized(
                                        Vec2::new(name_w, ROW_BTN_H),
                                        Button::new(
                                            RichText::new(title)
                                                .color(Color32::from_rgb(51, 255, 189)),
                                        )
                                        .fill(ui.style().visuals.window_fill),
                                    )
                                    .on_hover_text(serial);
                                if name_btn.clicked() {
                                    collapse.toggle(ui);
                                }
                                let view_btn = ui
                                    .add_sized(
                                        Vec2::new(ROW_BTN_W, ROW_BTN_H),
                                        Button::new(
                                            RichText::new(icons::EYE)
                                                .strong()
                                                .color(ui.style().visuals.warn_fg_color),
                                        )
                                        .fill(ui.style().visuals.window_fill),
                                    )
                                    .on_hover_text("View this booting machine's screen");
                                if view_btn.clicked() {
                                    view_clicked = true;
                                }
                            },
                        );

                        if collapse.is_open() {
                            ui.add_space(6.);
                            ui.separator();
                            ui.add_space(4.);
                            Grid::new(("preboot_card_grid", serial))
                                .num_columns(2)
                                .spacing(Vec2::new(8.0, 3.0))
                                .show(ui, |ui| {
                                    let mut row = |k: &str, v: &str, mono: bool| {
                                        ui.label(RichText::new(k).small().color(Color32::GRAY));
                                        let mut t = RichText::new(v)
                                            .color(Color32::from_rgb(199, 202, 245));
                                        if mono {
                                            t = t.monospace();
                                        }
                                        ui.add(
                                            eframe::egui::Label::new(t).wrap(),
                                        );
                                        ui.end_row();
                                    };
                                    row("OA3 key", serial, true);
                                    row("Kind", kind, false);
                                    row("Transport", if direct { "direct TCP" } else { "relay" }, false);
                                    if !friendly.is_empty() {
                                        row("Product", friendly, false);
                                    }
                                    let seen = if age_secs == i64::MAX {
                                        "no heartbeat".to_string()
                                    } else {
                                        format!("{age_secs}s ago")
                                    };
                                    row("Last seen", &seen, false);
                                });
                        }
                    });
            },
        );

        collapse.store(ui.ctx());
        view_clicked
    }

    /// Central panel: always renders the focused (Docked) client session.
    /// Script Editor and Chat now live in the right panel (see `right_panel_ui`).
    pub fn ui(&mut self, ui: &mut Ui) {
        if let Some(focused) = self.focused_client.clone() {
            let layout = self.session_layout
                .get(&focused)
                .copied()
                .unwrap_or_default();
            if layout == SessionLayout::Docked {
                let fresh = self
                    .clients
                    .iter()
                    .find(|c| c.connection_string == focused)
                    .cloned();
                if let Some(ws_client) = self.ws_clients.get_mut(&focused) {
                    // A session opened by hash is absent from the scoped list;
                    // render it from its own copy instead of skipping it.
                    if let Some(data) = fresh {
                        ws_client.client = data;
                    }
                    ws_client.show(ui);
                }
            }
        }
    }

    /// Right-side panel content selected from the Panels menu.
    pub fn right_panel_ui(&mut self, ui: &mut Ui) {
        match self.right_panel {
            Some(RightPanel::ScriptEditor) => self.script_editor.ui(ui),
            Some(RightPanel::Chat) => {
                self.ai_playground.focused_client = self.focused_client.clone();
                self.ai_playground.enhanced_ai_playground(ui);
            }
            None => {}
        }
    }
}

/// Render the detail fields for a client in a two-column grid. This was
/// previously a hover popup; it's now the contents of the row's
/// collapsing body so admins can leave details pinned open while they
/// work, copy values out of it, etc.
fn client_details_grid(
    ui: &mut Ui,
    tx: &Sender<ClientUiAction>,
    client: &ConnectedClient,
    formatted_date: &str,
    assigned_user: &str,
    is_ws_connected: bool,
    reachability: Option<&crate::ui_data::reachability::ReachabilityStatus>,
    fk_health: Option<(bool, bool)>,
) {
    let value_max_w = CLIENT_DETAILS_VALUE_W;
    ui.set_max_width(CLIENT_ROW_CONTENT_W);
    if let Some(fname) = client.friendly_name.as_deref() {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(fname)
                    .strong()
                    .color(Color32::from_rgb(51, 255, 189)),
            );
            if client.customer_locked {
                ui.label(
                    RichText::new("locked")
                        .small()
                        .color(theme::info(ui)),
                )
                .on_hover_text(
                    "Customer was manually re-linked. \
                     OA-key auto-detection won't overwrite it.",
                );
            }
        });
        ui.add_space(2.);
    }

    Grid::new(("client_details_grid", &client.connection_string))
        .num_columns(2)
        .spacing(eframe::egui::Vec2::new(10., 2.))
        .show(ui, |ui| {
            row(ui, "Connection", &client.connection_string, value_max_w);
            row(ui, "Last update", formatted_date, value_max_w);
            row(
                ui,
                "Status",
                if is_ws_connected {
                    "* connected (active session)"
                } else if client.connected && recently_active(client) {
                    "! online — no active admin session"
                } else if client.connected {
                    "- stale — DB still connected but no heartbeat for >5 min"
                } else {
                    "x disconnected"
                },
                value_max_w,
            );
            row(ui, "Assigned to", assigned_user, value_max_w);

            // Direct TCP row — surfaces both what the client
            // *advertised* (its `local_ip:tcp_port`) and what the
            // per-admin probe *found* when it actually tried to
            // connect there. The two can disagree (advertised but
            // unreachable from this admin's network) and that
            // mismatch is exactly what slice 5 tracks.
            match (client.local_ip.as_deref(), client.tcp_port) {
                (Some(ip), Some(port)) if !ip.is_empty() => {
                    let endpoint = format!("{ip}:{port}");
                    let detail = match reachability {
                        Some(r) if r.reachable => {
                            format!("{endpoint}  + reachable")
                        }
                        Some(r) => {
                            let err = r
                                .error
                                .as_deref()
                                .unwrap_or("unreachable")
                                .chars()
                                .take(80)
                                .collect::<String>();
                            format!("{endpoint}  x {err} (relay still works)")
                        }
                        None => format!("{endpoint}  (probing…)"),
                    };
                    row(ui, "Direct TCP", &detail, value_max_w);
                }
                _ => {
                    row(ui, "Direct TCP", "(not advertised — relay only)", value_max_w);
                }
            }

            let (cust_ok, comp_ok) = fk_health.unwrap_or((false, false));
            let cust_label = client
                .customer
                .as_ref()
                .map(|c| c.key_string().to_string())
                .unwrap_or_else(|| "(none)".into());
            ui.label(RichText::new("Customer").small().color(Color32::GRAY));
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(&cust_label)
                        .small()
                        .color(fk_color(ui, cust_ok, client.customer.is_some())),
                );
                if !cust_ok {
                    if ui.small_button("Link").clicked() {
                        let _ = tx.try_send(ClientUiAction::LinkCustomer(client.clone()));
                    }
                }
            });
            ui.end_row();

            let comp_label = client
                .computer
                .as_ref()
                .map(|c| c.key_string().to_string())
                .unwrap_or_else(|| "(none)".into());
            ui.label(RichText::new("Computer").small().color(Color32::GRAY));
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(&comp_label)
                        .small()
                        .color(fk_color(ui, comp_ok, client.computer.is_some())),
                );
                if !comp_ok {
                    if ui.small_button("Link").clicked() {
                        let _ = tx.try_send(ClientUiAction::LinkComputer(client.clone()));
                    }
                }
            });
            ui.end_row();

            if let Some(created) = client.created_at.as_ref() {
                row(ui, "Created", &created.to_string(), value_max_w);
            }
        });
}

fn row(ui: &mut Ui, key: &str, val: &str, value_max_w: f32) {
    ui.label(RichText::new(key).small().color(Color32::GRAY));
    ui.scope(|ui| {
        ui.set_max_width(value_max_w);
        ui.label(RichText::new(val).small());
    });
    ui.end_row();
}

fn fk_color(ui: &Ui, exists: bool, has_fk: bool) -> Color32 {
    if !has_fk {
        theme::error(ui)
    } else if exists {
        theme::success(ui)
    } else {
        theme::warn(ui)
    }
}

fn queue_fk_health_check(
    tx: &crossbeam::channel::Sender<(String, bool, bool)>,
    cache: &HashMap<String, (bool, bool)>,
    client: &ConnectedClient,
) {
    if cache.contains_key(&client.connection_string) {
        return;
    }
    let cs = client.connection_string.clone();
    let cust = client.customer.clone();
    let comp = client.computer.clone();
    let tx = tx.clone();
    crate::PlatformSpawner::spawn(async move {
        use database::schema::utilities::record_exists;
        let cust_ok = match cust {
            Some(id) => matches!(record_exists(id).await, Ok(Some(true))),
            None => false,
        };
        let comp_ok = match comp {
            Some(id) => matches!(record_exists(id).await, Ok(Some(true))),
            None => false,
        };
        let _ = tx.try_send((cs, cust_ok, comp_ok));
    });
}

/// Render the slice-2 security-inventory list inside an expanded
/// client row. Three columns: product (name + version when known),
/// status badge (Active / Disabled / —), source pill
/// (SecurityCenter / Registry / Heuristic). The pill makes it
/// obvious whether "Webroot Active" is a Windows-Security-Center
/// fact or a registry-walk inference — operators triaging an
/// infected machine want to know which one before they trust it.
fn render_security_inventory(
    ui: &mut Ui,
    products: &[database::schema::InstalledSecurityProduct],
) {
    use database::schema::SecurityProductSource;

    Grid::new("client_security_inventory_grid")
        .num_columns(3)
        .spacing(eframe::egui::Vec2::new(10., 2.))
        .show(ui, |ui| {
            for product in products {
                // Column 1: name + version (+ vendor if we have it
                // but no version, so the grid row doesn't go bare).
                let label = match (
                    product.version.as_deref(),
                    product.vendor.as_deref(),
                ) {
                    (Some(v), _) => format!("{}  {v}", product.name),
                    (None, Some(vendor)) => format!("{}  ({vendor})", product.name),
                    (None, None) => product.name.clone(),
                };
                ui.label(RichText::new(label).small());

                // Column 2: status badge.
                let (color, text) = match product.active {
                    Some(true) => (Color32::from_rgb(100, 200, 100), "* Active"),
                    Some(false) => (Color32::from_rgb(255, 150, 80), "o Disabled"),
                    None => (Color32::GRAY, "—"),
                };
                ui.label(RichText::new(text).small().color(color));

                // Column 3: source pill — short and color-coded so
                // the eye can pick out "is this data trustworthy?"
                // without reading the full word.
                let (src_color, src_text) = match product.source {
                    SecurityProductSource::SecurityCenter => {
                        (Color32::from_rgb(120, 200, 255), "SecurityCenter")
                    }
                    SecurityProductSource::Registry => {
                        (Color32::from_rgb(199, 202, 245), "Registry")
                    }
                    SecurityProductSource::Heuristic => {
                        (Color32::from_rgb(180, 180, 180), "Heuristic")
                    }
                };
                ui.label(RichText::new(src_text).small().color(src_color));

                ui.end_row();
            }
        });
}
