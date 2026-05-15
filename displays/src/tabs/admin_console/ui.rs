use eframe::egui::{
    collapsing_header::CollapsingState, text::LayoutJob, Align, Button, Color32, FontFamily,
    FontId, Frame, Grid, Margin, RichText, TextFormat, Ui, Vec2, Widget, WidgetText,
};
use database::schema::{ConnectedClient, RecordIdExt};
use std::collections::HashMap;
use crossbeam::channel::Sender;
use chrono::{DateTime, Local, Utc};
use super::ClientUiAction;
use super::SessionLayout;
use crate::get_database_users;
use log::info;

use super::{AdminConsole, WebConsolePageState};

/// How stale a `last_update` timestamp can be before we treat the client as
/// offline — even if the DB `connected` flag is still `true`. Five minutes
/// is conservative; the client heartbeats every ~30 s so anything older than
/// that is either crashed or unreachable.
const STALE_THRESHOLD_SECS: i64 = 300;

/// Uniform row height for the buttons inside a client row. All action
/// buttons and the name button are sized to this so the row is visually
/// rectangular regardless of glyph metrics.
const ROW_BTN_H: f32 = 30.0;
const ROW_BTN_W: f32 = 30.0;

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
        // Slice 2: latest gathered security inventory for this
        // client, or `None` if none has arrived (yet) this session.
        // Rendered as an extra section in the expanded body.
        security_inventory: Option<&[database::schema::InstalledSecurityProduct]>,
    ) {
        let style = ui.style().clone();
        let row_id = ui.make_persistent_id((
            "admin_client_row",
            client.connection_string.as_str(),
        ));
        let mut collapse = CollapsingState::load_with_default_open(ui.ctx(), row_id, false);

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

        Frame::default()
            .fill(Color32::from_rgb(13, 13, 15))
            .inner_margin(Margin::same(4))
            .outer_margin(Margin::symmetric(3, 0))
            .corner_radius(eframe::egui::CornerRadius::same(5))
            .stroke(style.visuals.window_stroke)
            .show(ui, |ui| {
                // ── Header row ───────────────────────────────────────────────
                //
                // We avoid `egui_extras::StripBuilder` here even though it
                // has a tidy "fill remaining" cell, because its
                // `.horizontal()` allocates from
                // `available_rect_before_wrap()` — and inside the parent
                // `ScrollArea::show(...)` that rect is effectively
                // unbounded vertically. The first client row would then
                // stretch to the full viewport height (a bug observed in
                // the wild). A plain `ui.horizontal` sizes itself to the
                // max of its children's heights, which is what we want.
                //
                // To still get a "fill remaining width" name button we
                // compute the remaining horizontal budget manually after
                // accounting for the right-side buttons.
                let is_focused = focused_client == Some(client.connection_string.as_str());
                let focus_color = if is_focused {
                    Color32::from_rgb(51, 255, 189)
                } else {
                    Color32::GRAY
                };
                let (indicator_color, indicator_text) =
                    connection_indicator(is_ws_connected, client);
                let arrow = if collapse.is_open() { "⏷" } else { "⏵" };

                ui.horizontal(|ui| {
                    // Chevron toggles the collapsing body.
                    let chevron = Button::new(RichText::new(arrow).strong())
                        .fill(ui.style().visuals.window_fill)
                        .min_size(Vec2::new(ROW_BTN_W, ROW_BTN_H))
                        .ui(ui)
                        .on_hover_text(if collapse.is_open() {
                            "Collapse client details"
                        } else {
                            "Expand client details & secondary actions"
                        });
                    if chevron.clicked() {
                        collapse.toggle(ui);
                    }

                    // Status dot — vertically centered within the row by
                    // wrapping in a small fixed allocation so it doesn't
                    // ride high on the baseline next to the buttons.
                    ui.add_sized(
                        [16.0, ROW_BTN_H],
                        eframe::egui::Label::new(
                            RichText::new(indicator_text).color(indicator_color),
                        ),
                    );

                    // Name button fills the remainder. `item_spacing.x`
                    // appears between every child the parent
                    // `ui.horizontal` lays out, including between the
                    // last item before us and us, and between us and the
                    // two right-side buttons — so reserve two gaps' worth
                    // of spacing on top of the two button widths.
                    let item_gap = ui.spacing().item_spacing.x;
                    let right_strip_w = ROW_BTN_W * 2.0 + item_gap * 2.0;
                    let name_w = (ui.available_width() - right_strip_w).max(80.0);

                    let name_btn = Button::new(client_name_text(client))
                        .fill(ui.style().visuals.window_fill)
                        .min_size(Vec2::new(name_w, ROW_BTN_H))
                        .ui(ui)
                        .on_hover_text(
                            "Click to expand client details and secondary actions",
                        );
                    if name_btn.clicked() {
                        collapse.toggle(ui);
                    }

                    // Focus — always visible: changes which client
                    // receives commands without opening a session.
                    let focus_btn =
                        Button::new(RichText::new("◉").strong().color(focus_color))
                            .fill(ui.style().visuals.window_fill)
                            .min_size(Vec2::new(ROW_BTN_W, ROW_BTN_H))
                            .ui(ui)
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

                    // Connect / open — always visible: the primary
                    // action for this row.
                    let connect_btn = Button::new(
                        RichText::new("⬈")
                            .strong()
                            .color(ui.style().visuals.warn_fg_color),
                    )
                    .fill(ui.style().visuals.window_fill)
                    .min_size(Vec2::new(ROW_BTN_W, ROW_BTN_H))
                    .ui(ui)
                    .on_hover_text("Open / focus this machine");
                    if connect_btn.clicked() {
                        info!("Sent Connection Command");
                        let _ = tx.try_send(ClientUiAction::ConnectClient(client.clone()));
                    }
                });

                // ── Expanded body ─────────────────────────────────────────────
                if collapse.is_open() {
                    ui.add_space(6.);
                    ui.separator();
                    ui.add_space(4.);

                    client_details_grid(
                        ui,
                        client,
                        &formatted_date,
                        &assigned_user_text,
                        is_ws_connected,
                    );

                    // Slice 2: security inventory section. Renders
                    // only when we have data — operators looking at
                    // a client that hasn't yet responded to the
                    // GatherSecurityInventory request shouldn't see
                    // a "Security: (empty)" placeholder either.
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

                    // Secondary actions: kept off the always-visible row
                    // because they're either destructive (Disconnect),
                    // rare (Re-link, the linkage-fix flow), or layout
                    // tweaks (Dock/Float). Uniform sized so it reads as a
                    // toolbar rather than ad-hoc buttons.
                    let layout = session_layout
                        .get(client.connection_string.as_str())
                        .copied()
                        .unwrap_or_default();
                    let (float_label, float_tip) = match layout {
                        SessionLayout::Floating => {
                            ("🔓 Dock", "Floating (unlocked) — click to dock")
                        }
                        SessionLayout::Docked => {
                            ("🔒 Float", "Docked (locked) — click to float")
                        }
                    };
                    let relink_color = if client.customer_locked {
                        Color32::from_rgb(120, 200, 255)
                    } else {
                        Color32::from_rgb(199, 202, 245)
                    };
                    let (relink_label, relink_tip) = if client.customer_locked {
                        (
                            "🔗 Re-link",
                            "Customer is locked (manually re-linked).\nClick to change linkage.",
                        )
                    } else {
                        (
                            "🔍 Re-link",
                            "Re-link to a different customer\n(used-machine-was-our-customer fix)",
                        )
                    };

                    ui.horizontal(|ui| {
                        let disconnect = Button::new(
                            RichText::new("✖ Disconnect")
                                .strong()
                                .color(ui.style().visuals.error_fg_color),
                        )
                        .fill(ui.style().visuals.window_fill)
                        .min_size(Vec2::new(130., ROW_BTN_H))
                        .ui(ui)
                        .on_hover_text(
                            "Disconnect this client\n\
                             (closes the local session and removes it from the list;\n\
                             the connected_client record stays in the database)",
                        );
                        if disconnect.clicked() {
                            let _ = tx.try_send(ClientUiAction::DisconnectClient(client.clone()));
                        }

                        let relink = Button::new(
                            RichText::new(relink_label).strong().color(relink_color),
                        )
                        .fill(ui.style().visuals.window_fill)
                        .min_size(Vec2::new(130., ROW_BTN_H))
                        .ui(ui)
                        .on_hover_text(relink_tip);
                        if relink.clicked() {
                            let _ = tx.try_send(ClientUiAction::RelinkCustomer(client.clone()));
                        }

                        let float_btn = Button::new(
                            RichText::new(float_label).strong().color(Color32::LIGHT_RED),
                        )
                        .fill(ui.style().visuals.window_fill)
                        .min_size(Vec2::new(110., ROW_BTN_H))
                        .ui(ui)
                        .on_hover_text(float_tip);
                        if float_btn.clicked() {
                            let _ = tx.try_send(ClientUiAction::ToggleClientFloat(
                                client.connection_string.clone(),
                            ));
                        }
                    });
                }
            });

        // Persist the open/closed flip we may have made via `toggle()`.
        collapse.store(ui.ctx());
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        match self.state {
            WebConsolePageState::ScriptEditor => self.script_editor.ui(ui),
            #[cfg(not(target_arch = "wasm32"))]
            WebConsolePageState::AiPlayground => self.ai_playground.enhanced_ai_playground(ui),
            _ => {
                // Render the focused client (Docked layout) in the central panel.
                if let Some(focused) = self.focused_client.clone() {
                    let layout = self.session_layout
                        .get(&focused)
                        .copied()
                        .unwrap_or_default();
                    if layout == SessionLayout::Docked {
                        if let Some(data) = self.clients.iter().find(|c| c.connection_string == focused).cloned() {
                            if let Some(ws_client) = self.ws_clients.get_mut(&focused) {
                                ws_client.client = data;
                                ws_client.show(ui);
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Render the detail fields for a client in a two-column grid. This was
/// previously a hover popup; it's now the contents of the row's
/// collapsing body so admins can leave details pinned open while they
/// work, copy values out of it, etc.
fn client_details_grid(
    ui: &mut Ui,
    client: &ConnectedClient,
    formatted_date: &str,
    assigned_user: &str,
    is_ws_connected: bool,
) {
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

    Grid::new(("client_details_grid", &client.connection_string))
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
}

fn row(ui: &mut Ui, key: &str, val: &str) {
    ui.label(RichText::new(key).small().color(Color32::GRAY));
    ui.label(RichText::new(val).small());
    ui.end_row();
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
                    Some(true) => (Color32::from_rgb(100, 200, 100), "● Active"),
                    Some(false) => (Color32::from_rgb(255, 150, 80), "○ Disabled"),
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
