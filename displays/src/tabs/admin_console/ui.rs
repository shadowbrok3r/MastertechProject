use eframe::egui::Shadow;
use eframe::egui::{
    collapsing_header::CollapsingState, Align, Color32, Frame, Grid, Id, Layout, Margin, Rangef,
    Rect, RichText, ScrollArea, Sense, Ui, Vec2,
};
use database::schema::{ConnectedClient, LiveTaskPayload, RecordIdExt};
use std::collections::HashMap;
use crossbeam::channel::Sender;
use chrono::{DateTime, Local, TimeZone, Utc};
use super::ClientUiAction;
use super::ClientLinkHealth;
use super::SessionLayout;
use crate::get_database_users;
use crate::ui_tools::controls::{
    self, IconBox, IconButton, IconLink, ListItem, Metrics, Section, SpinnerSize,
};
use crate::ui_tools::icons::svg::{self, Icon};
use crate::ui_tools::{glass_card, theme};
use crate::Spawner;

use super::{AdminConsole, RightPanel};

/// How stale a `last_update` timestamp can be before we treat the client as
/// offline — even if the DB `connected` flag is still `true`. Five minutes
/// is conservative; the client heartbeats every ~30 s so anything older than
/// that is either crashed or unreachable.
const STALE_THRESHOLD_SECS: i64 = 300;

const ROW_STATUS_W: f32 = 16.0;
const ROW_ITEM_GAP: f32 = 8.0;

/// Outer width of a client row drawn outside the side-panel list.
pub const CLIENT_ROW_W: f32 = 378.0;

/// Slot for the transport badge, wide enough for the longest tag (`RELAY`).
/// Always allocated, so opening a session cannot widen the row.
const ROW_BADGE_W: f32 = 38.0;

/// Width the details grid's key column and its gap take from a row.
const DETAILS_KEY_COL_W: f32 = 90.0;

/// Display format shared by the details grid's timestamps.
const CLIENT_TIME_FORMAT: &str = "%Y/%m/%d @ %I:%M%p";

/// Side of a client row's square buttons, which is also its header height.
fn row_btn(ui: &Ui) -> f32 {
    IconBox::Md.side(&Metrics::get(ui.ctx()))
}

/// Name-button width: the content minus two buttons, the status slot, the badge slot and four gaps.
fn client_name_btn_w(content_w: f32, btn: f32) -> f32 {
    content_w - (btn * 2.0 + ROW_STATUS_W + ROW_BADGE_W + ROW_ITEM_GAP * 4.0)
}

/// Id of a client row's open state.
fn client_row_id(ui: &Ui, connection_string: &str) -> Id {
    ui.make_persistent_id(("admin_client_row", connection_string))
}

/// Card frame of a client or pre-boot row; painted, not frosted.
fn row_frame(ui: &Ui) -> Frame {
    Frame::default()
        .fill(glass_card::card_fill(ui))
        .inner_margin(Margin::same(4))
        .outer_margin(Margin::ZERO)
        .corner_radius(eframe::egui::CornerRadius::same(
            ui.visuals().window_corner_radius.nw.max(4),
        ))
        .shadow(Shadow::NONE)
        .stroke(glass_card::card_stroke(ui))
}

/// Runs `add` with its content width inside a top-down [`row_frame`] `width` wide; returns the card's rect.
fn row_card(ui: &mut Ui, width: f32, add: impl FnOnce(&mut Ui, f32)) -> Rect {
    ui.allocate_ui_with_layout(Vec2::new(width, 0.0), Layout::top_down(Align::Min), |ui| {
        let frame = row_frame(ui);
        let mut card = glass_card::FixedWidthFrame::begin(ui, frame, width);
        let content_w = card.content_ui.max_rect().width();
        add(&mut card.content_ui, content_w);
        card.end(ui).rect
    })
    .inner
}

/// Outer width of a row at the cursor, ending as far inside `list`'s right edge as it starts inside its left.
pub(super) fn client_row_width(ui: &Ui, list: Rangef) -> f32 {
    let avail = ui.available_rect_before_wrap();
    let right = (list.max - (avail.left() - list.min)).min(avail.right());
    (right - avail.left()).max(0.0)
}

/// Scrolling client list with rows indented under optional store sections; `row` draws one client at an outer width.
pub(super) fn client_list(
    ui: &mut Ui,
    max_height: f32,
    groups: &[(Option<String>, Vec<usize>)],
    mut row: impl FnMut(&mut Ui, usize, f32),
) {
    let list = ui.available_rect_before_wrap().x_range();
    ScrollArea::vertical()
        .auto_shrink([false, true])
        .max_height(max_height)
        .show(ui, |ui| {
            let mut rows = |ui: &mut Ui, indices: &[usize]| {
                for &index in indices {
                    let width = client_row_width(ui, list);
                    row(ui, index, width);
                }
            };
            for (store, indices) in groups {
                match store {
                    Some(store) => {
                        let title = format!("{store}  ({})", indices.len());
                        Section::new(("admin_store_group", store.as_str()), &title)
                            .icon(svg::STOREFRONT)
                            .show(ui, |ui| rows(ui, indices));
                    }
                    None => {
                        ui.indent("admin_client_rows", |ui| rows(ui, indices));
                    }
                }
            }
        });
}

/// RFC 3339 `raw` in `tz` as [`CLIENT_TIME_FORMAT`], or `raw` when it does not parse.
fn format_client_time<Tz: TimeZone>(raw: &str, tz: &Tz) -> String
where
    Tz::Offset: std::fmt::Display,
{
    DateTime::parse_from_rfc3339(raw)
        .map(|t| t.with_timezone(tz).format(CLIENT_TIME_FORMAT).to_string())
        .unwrap_or_else(|_| raw.to_string())
}

/// [`format_client_time`] in local time.
fn client_time(raw: &str) -> String {
    format_client_time(raw, &Local)
}

/// The details grid's Link button.
fn link_button() -> IconLink<'static> {
    IconLink::new("Link").icon(svg::LINK).small(true)
}

/// A `width`-wide slot holding the Link button when `show`; returns whether it was clicked.
fn link_slot(ui: &mut Ui, width: f32, show: bool, tip: &str) -> bool {
    if show {
        link_button().show(ui).on_hover_text(tip).clicked()
    } else {
        ui.allocate_exact_size(Vec2::new(width, 0.0), Sense::hover());
        false
    }
}

/// Paints `icon` in `color` centred in a status slot `height` tall.
fn status_slot(ui: &mut Ui, icon: Icon, color: Color32, height: f32) {
    let size = Metrics::get(ui.ctx()).icon_sm;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ROW_STATUS_W, height), Sense::hover());
    icon.paint(ui, rect, size, color);
}

/// A small faint sub-heading led by an icon.
fn section_title(ui: &mut Ui, icon: Icon, text: &str) {
    ui.horizontal(|ui| {
        icon.show_tinted(ui, Metrics::get(ui.ctx()).icon_sm, theme::weak_text(ui));
        ui.label(RichText::new(text).small().color(theme::weak_text(ui)));
    });
}

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

/// Returns `(color, icon)` for the connection status dot.
///
/// Priority:
/// 1. Active admin session open → success
/// 2. DB-connected + recent heartbeat + no admin session → warn
/// 3. Everything else (disconnected or stale) → muted
fn connection_indicator(
    ui: &Ui,
    is_ws_connected: bool,
    client: &ConnectedClient,
) -> (Color32, Icon) {
    if is_ws_connected {
        (theme::success(ui), svg::CHECK_CIRCLE)
    } else if client.connected && recently_active(client) {
        (theme::warn(ui), svg::WARNING)
    } else {
        (theme::weak_text(ui), svg::MINUS_CIRCLE)
    }
}

/// Badge color for a live session's transport path, graded by how direct it is: local TCP reads
/// as good, a relay hop as a caveat, the legacy websocket room as merely informational.
pub(crate) fn transport_color(ui: &Ui, kind: super::client_interface::TransportKind) -> Color32 {
    use super::client_interface::TransportKind;
    match kind {
        TransportKind::Tcp => theme::success(ui),
        TransportKind::Relay => theme::warn(ui),
        TransportKind::WebSocket => theme::info(ui),
        TransportKind::Local => theme::success(ui),
    }
}

/// The client's friendly name, or its connection string.
fn client_name(client: &ConnectedClient) -> &str {
    client
        .friendly_name
        .as_deref()
        .unwrap_or(&client.connection_string)
}

/// Version cell text and tooltip: the row's recorded build, else the open session's, else "—".
fn version_cell(
    recorded: Option<&str>,
    session: Option<&str>,
    is_ws_connected: bool,
) -> (String, String) {
    match (recorded, session) {
        (Some(rec), Some(live)) if rec != live => (
            format!("v{rec}"),
            format!("Recorded at the client's last check-in; the open session reports v{live}"),
        ),
        (Some(rec), _) => (
            format!("v{rec}"),
            "MasterTech build recorded at the client's last check-in".to_string(),
        ),
        (None, Some(live)) => (
            format!("v{live}"),
            "MasterTech build reported by the open session".to_string(),
        ),
        (None, None) if is_ws_connected => (
            "—".to_string(),
            "The open session has not reported a build".to_string(),
        ),
        (None, None) => (
            "—".to_string(),
            "Not recorded by the client's build; reported once an admin session is open"
                .to_string(),
        ),
    }
}

/// Square icon button for a client row's secondary actions; `tip` names the action.
fn row_icon_action(ui: &mut Ui, icon: Icon, color: Color32, tip: &str) -> bool {
    controls::IconButton::new(icon, IconBox::Md)
        .tint(color)
        .tip(tip)
        .show(ui)
        .clicked()
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
    ///
    /// `row_w` is the card's outer width; returns the card's rect.
    #[allow(clippy::too_many_arguments)]
    pub fn client_header(
        ui: &mut Ui,
        row_w: f32,
        tx: Sender<ClientUiAction>,
        client: &ConnectedClient,
        session_layout: HashMap<String, SessionLayout>,
        focused_client: Option<&str>,
        is_ws_connected: bool,
        fk_health_tx: &crossbeam::channel::Sender<(String, ClientLinkHealth)>,
        fk_health_cache: &HashMap<String, ClientLinkHealth>,
        client_tasks_tx: &crossbeam::channel::Sender<(String, Option<Vec<LiveTaskPayload>>)>,
        client_tasks_cache: &HashMap<String, Option<Vec<LiveTaskPayload>>>,
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
        // MasterTech build the open session reported, or `None` without one.
        client_version: Option<&str>,
    ) -> Rect {
        let row_id = client_row_id(ui, &client.connection_string);
        let mut collapse = CollapsingState::load_with_default_open(ui.ctx(), row_id, false);
        if collapse.is_open() {
            queue_fk_health_check(fk_health_tx, fk_health_cache, client);
            queue_client_tasks_fetch(client_tasks_tx, client_tasks_cache, client);
        }
        let fk_health = fk_health_cache.get(&client.connection_string).copied();
        let client_tasks = client_tasks_cache
            .get(&client.connection_string)
            .and_then(|t| t.as_deref());

        let formatted_date = client_time(
            &client
                .last_update
                .unwrap_or_else(|| Utc::now().into())
                .to_string(),
        );
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

        let rect = row_card(ui, row_w, |ui, content_w| {
            let is_focused = focused_client == Some(client.connection_string.as_str());
            let focus_color = if is_focused {
                theme::success(ui)
            } else {
                theme::weak_text(ui)
            };
            let (indicator_color, indicator_icon) =
                connection_indicator(ui, is_ws_connected, client);
            let arrow = if collapse.is_open() {
                svg::CARET_DOWN
            } else {
                svg::CARET_RIGHT
            };
            let btn = row_btn(ui);

            ui.allocate_ui_with_layout(
                Vec2::new(content_w, btn),
                Layout::left_to_right(Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing.x = ROW_ITEM_GAP;

                    let name_btn = ListItem::new(client_name(client))
                        .icon(arrow)
                        .icon_tone(theme::weak_text(ui))
                        .tone(theme::success(ui))
                        .width(client_name_btn_w(content_w, btn))
                        .show(ui)
                        .on_hover_text(if collapse.is_open() {
                            "Collapse client details"
                        } else {
                            "Expand client details & secondary actions"
                        });
                    if name_btn.clicked() {
                        collapse.toggle(ui);
                    }

                    status_slot(ui, indicator_icon, indicator_color, btn);

                    // Transport badge for the open session's live path,
                    // in a fixed slot that is allocated either way so
                    // every row is the same width.
                    let (badge, badge_color, badge_tip) = match transport {
                        Some((kind, session_up)) => {
                            use super::client_interface::TransportKind;
                            let (badge, tip) = match kind {
                                TransportKind::Tcp => ("TCP", "Direct TCP (same network)"),
                                TransportKind::Relay => ("RELAY", "Relay tunnel via websocket server"),
                                TransportKind::WebSocket => ("WS", "Legacy WebSocket relay room"),
                                TransportKind::Local => ("LOCAL", "This machine, in-process"),
                            };
                            let color = if session_up {
                                transport_color(ui, kind)
                            } else {
                                theme::weak_text(ui)
                            };
                            let tip = if session_up {
                                tip.to_string()
                            } else {
                                format!("{tip} — reconnecting")
                            };
                            (badge, color, Some(tip))
                        }
                        None => ("", Color32::TRANSPARENT, None),
                    };
                    let badge_slot = ui.add_sized(
                        Vec2::new(ROW_BADGE_W, btn),
                        eframe::egui::Label::new(
                            RichText::new(badge).small().strong().color(badge_color),
                        ),
                    );
                    if let Some(tip) = badge_tip {
                        badge_slot.on_hover_text(tip);
                    }

                    let focus_btn = IconButton::new(svg::CROSSHAIR, IconBox::Md)
                        .selected(is_focused)
                        .tint(focus_color)
                        .tip(if is_focused {
                            "Focused (receives commands)"
                        } else {
                            "Set as focused client"
                        })
                        .show(ui);
                    if focus_btn.clicked() {
                        let _ = tx.try_send(ClientUiAction::FocusClient(
                            client.connection_string.clone(),
                        ));
                    }

                    let connect_btn = IconButton::new(svg::ARROW_SQUARE_OUT, IconBox::Md)
                        .tint(theme::primary(ui))
                        .tip("Open / focus this machine")
                        .show(ui);
                    if connect_btn.clicked() {
                        let _ = tx.try_send(ClientUiAction::ConnectClient(client.clone()));
                    }
                },
            );

            // ── Expanded body ─────────────────────────────────────────────
            if collapse.is_open() {
                ui.add_space(6.);
                controls::hairline(ui);
                ui.add_space(4.);

                client_details_grid(
                    ui,
                    content_w,
                    &tx,
                    client,
                    &formatted_date,
                    &assigned_user_text,
                    is_ws_connected,
                    reachability,
                    fk_health,
                    client_tasks,
                    client_version,
                );

                if let Some(products) = security_inventory
                    && !products.is_empty()
                {
                    ui.add_space(8.);
                    controls::hairline(ui);
                    ui.add_space(4.);
                    section_title(ui, svg::SHIELD_CHECK, "Security inventory");
                    ui.add_space(2.);
                    render_security_inventory(ui, products);
                }

                ui.add_space(8.);
                controls::hairline(ui);
                ui.add_space(4.);

                section_title(ui, svg::LIGHTNING, "Actions");
                ui.add_space(2.);

                let layout = session_layout
                    .get(client.connection_string.as_str())
                    .copied()
                    .unwrap_or_default();
                let (float_icon, float_tip) = match layout {
                    SessionLayout::Floating => {
                        (
                            svg::ARROW_SQUARE_IN,
                            "Dock: floating (unlocked) — click to dock",
                        )
                    }
                    SessionLayout::Docked => {
                        (
                            svg::PICTURE_IN_PICTURE,
                            "Float: docked (locked) — click to float",
                        )
                    }
                };
                let relink_color = if client.customer_locked {
                    theme::info(ui)
                } else {
                    theme::weak_text(ui)
                };
                let (auto_color, auto_tip) = if client.autopilot_opt_out {
                    (
                        theme::weak_text(ui),
                        "Autopilot off: excluded from unattended agent sweeps.\nClick to allow sweeps again.",
                    )
                } else {
                    (
                        theme::info(ui),
                        "Autopilot on: eligible for unattended agent sweeps.\nClick to exclude this client.",
                    )
                };
                let staff = fk_health.map(|h| h.staff_machine).unwrap_or(false);
                let (staff_color, staff_tip) = if staff {
                    (
                        theme::info(ui),
                        "Staff machine: owns no customer and is excluded from
                                 outcome reporting. Click to treat it as a customer machine.",
                    )
                } else {
                    (
                        theme::weak_text(ui),
                        "Customer machine. Click to mark it a staff machine:
                                 clears its customer link, keeps future service orders
                                 from adopting it, and drops it from outcome reporting.",
                    )
                };
                let relink_tip = if client.customer_locked {
                    "Re-link customer: locked (manually re-linked).\nClick to change linkage."
                } else {
                    "Re-link to a different customer\n(used-machine-was-our-customer fix)"
                };

                let err_color = theme::error(ui);
                let plain = theme::text(ui);
                ui.scope(|ui| {
                    ui.set_max_width(content_w);
                    ui.horizontal_wrapped(|ui| {
                        if row_icon_action(
                            ui,
                            svg::LINK_BREAK,
                            err_color,
                            "Disconnect this client\n\
                             (closes the local session and removes it from the list;\n\
                             the connected_client record stays in the database)",
                        ) {
                            let _ = tx.try_send(ClientUiAction::DisconnectClient(client.clone()));
                        }
                        if row_icon_action(ui, float_icon, plain, float_tip) {
                            let _ = tx.try_send(ClientUiAction::ToggleClientFloat(
                                client.connection_string.clone(),
                            ));
                        }
                        if row_icon_action(
                            ui,
                            svg::PENCIL,
                            plain,
                            "Rename: edit this client's friendly name",
                        ) {
                            let _ = tx.try_send(ClientUiAction::RenameClient(client.clone()));
                        }
                        if row_icon_action(ui, svg::USER_SWITCH, relink_color, relink_tip) {
                            let _ = tx.try_send(ClientUiAction::RelinkCustomer(client.clone()));
                        }
                        if row_icon_action(
                            ui,
                            svg::PLUGS_CONNECTED,
                            plain,
                            "Link computer: create or repair the computer record for this client",
                        ) {
                            let _ = tx.try_send(ClientUiAction::LinkComputer(client.clone()));
                        }
                        if row_icon_action(
                            ui,
                            svg::WRENCH,
                            plain,
                            "Repair links: cascade-repoint FKs to canonical computer id and fix diagnostic sessions",
                        ) {
                            let _ =
                                tx.try_send(ClientUiAction::RepairAssociations(client.clone()));
                        }
                        if row_icon_action(ui, svg::HOUSE, staff_color, staff_tip) {
                            let _ = tx.try_send(ClientUiAction::ToggleStaffMachine {
                                client: client.clone(),
                                staff,
                            });
                        }
                        if row_icon_action(ui, svg::ROBOT, auto_color, auto_tip) {
                            let _ = tx.try_send(ClientUiAction::ToggleAutopilotOptOut(
                                client.clone(),
                            ));
                        }
                    });
                });
            }
        });

        // Persist the open/closed flip we may have made via `toggle()`.
        collapse.store(ui.ctx());
        rect
    }

    /// Render one pre-boot UEFI box in the same collapsible card style as the
    /// connected-client rows. The collapsed title shows `{model} · {last5}`
    /// (product model + last 5 of the OA3/MSDM key); the expanded body
    /// enumerates the full identity. Returns `true` if View was clicked.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn preboot_card(
        ui: &mut Ui,
        row_w: f32,
        serial: &str,
        friendly: &str,
        age_secs: i64,
        kind: &str,
        direct: bool,
    ) -> bool {
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
        let arrow = if collapse.is_open() { svg::CARET_DOWN } else { svg::CARET_RIGHT };
        let mut view_clicked = false;

        row_card(ui, row_w, |ui, content_w| {
            let (dot_color, dot_icon) = if live {
                (theme::success(ui), svg::CHECK_CIRCLE)
            } else {
                (theme::weak_text(ui), svg::MINUS_CIRCLE)
            };
            // chevron + status + name + View chrome (mirror client rows).
            let btn = row_btn(ui);
            let name_w = content_w
                - (btn * 2.0 + ROW_STATUS_W + ROW_ITEM_GAP * 3.0);
            ui.allocate_ui_with_layout(
                Vec2::new(content_w, btn),
                Layout::left_to_right(Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing.x = ROW_ITEM_GAP;
                    let chevron = IconButton::new(arrow, IconBox::Md)
                        .tip("Show or hide details")
                        .show(ui);
                    if chevron.clicked() {
                        collapse.toggle(ui);
                    }
                    status_slot(ui, dot_icon, dot_color, btn);
                    let name_btn = controls::TextButton::new(&title)
                        .ink(theme::success(ui))
                        .min_width(name_w)
                        .show(ui)
                        .on_hover_text(serial);
                    if name_btn.clicked() {
                        collapse.toggle(ui);
                    }
                    let view_btn = IconButton::new(svg::MONITOR_PLAY, IconBox::Md)
                        .tint(theme::primary(ui))
                        .tip("View this booting machine's screen")
                        .show(ui);
                    if view_btn.clicked() {
                        view_clicked = true;
                    }
                },
            );

            if collapse.is_open() {
                ui.add_space(6.);
                controls::hairline(ui);
                ui.add_space(4.);
                Grid::new(("preboot_card_grid", serial))
                    .num_columns(2)
                    .spacing(Vec2::new(8.0, 4.0))
                    .show(ui, |ui| {
                        let mut row = |k: &str, v: &str, mono: bool| {
                            let (key, value) = (theme::weak_text(ui), theme::text(ui));
                            ui.label(RichText::new(k).small().color(key));
                            let mut t = RichText::new(v).color(value);
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
#[allow(clippy::too_many_arguments)]
fn client_details_grid(
    ui: &mut Ui,
    content_w: f32,
    tx: &Sender<ClientUiAction>,
    client: &ConnectedClient,
    formatted_date: &str,
    assigned_user: &str,
    is_ws_connected: bool,
    reachability: Option<&crate::ui_data::reachability::ReachabilityStatus>,
    fk_health: Option<ClientLinkHealth>,
    client_tasks: Option<&[LiveTaskPayload]>,
    client_version: Option<&str>,
) {
    let value_max_w = content_w - DETAILS_KEY_COL_W;
    ui.set_max_width(content_w);
    if let Some(fname) = client.friendly_name.as_deref() {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(fname).strong().color(theme::success(ui)),
            );
            if client.customer_locked {
                svg::LOCK.show_tinted(ui, Metrics::get(ui.ctx()).icon_sm, theme::info(ui));
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
        .spacing(eframe::egui::Vec2::new(8., 2.))
        .show(ui, |ui| {
            row(ui, "Connection", &client.connection_string, value_max_w);
            row(ui, "Last update", formatted_date, value_max_w);
            let (version_text, version_tip) =
                version_cell(client.client_version.as_deref(), client_version, is_ws_connected);
            icon_row(ui, "Version", svg::TAG, theme::weak_text(ui), &version_text, value_max_w)
                .on_hover_text(version_tip);
            let (status_icon, status_tone, status_text) = if is_ws_connected {
                (svg::CHECK_CIRCLE, theme::success(ui), "connected (active session)")
            } else if client.connected && recently_active(client) {
                (svg::WARNING, theme::warn(ui), "online — no active admin session")
            } else if client.connected {
                (
                    svg::CLOCK_COUNTDOWN,
                    theme::weak_text(ui),
                    "stale — DB still connected but no heartbeat for >5 min",
                )
            } else {
                (svg::X_CIRCLE, theme::weak_text(ui), "disconnected")
            };
            icon_row(ui, "Status", status_icon, status_tone, status_text, value_max_w);
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
                    let (icon, tone, detail) = match reachability {
                        Some(r) if r.reachable => (
                            svg::PLUGS_CONNECTED,
                            theme::success(ui),
                            format!("{endpoint}  reachable"),
                        ),
                        Some(r) => {
                            let err = r
                                .error
                                .as_deref()
                                .unwrap_or("unreachable")
                                .chars()
                                .take(80)
                                .collect::<String>();
                            (
                                svg::PLUG,
                                theme::warn(ui),
                                format!("{endpoint}  {err} (relay still works)"),
                            )
                        }
                        None => (
                            svg::HOURGLASS_SIMPLE,
                            theme::weak_text(ui),
                            format!("{endpoint}  (probing…)"),
                        ),
                    };
                    icon_row(ui, "Direct TCP", icon, tone, &detail, value_max_w);
                }
                _ => {
                    icon_row(
                        ui,
                        "Direct TCP",
                        svg::CLOUD,
                        theme::weak_text(ui),
                        "(not advertised — relay only)",
                        value_max_w,
                    );
                }
            }

            let ClientLinkHealth {
                customer_exists: cust_ok,
                computer_exists: comp_ok,
                staff_machine,
            } = fk_health.unwrap_or_default();
            // Newest matched task; customer/computer links open its modal.
            let primary_task = client_tasks.and_then(|t| t.first());
            let cust_label = client
                .customer
                .as_ref()
                .map(|c| c.key_string().to_string())
                .unwrap_or_else(|| {
                    if staff_machine { "(staff machine)".into() } else { "(none)".into() }
                });
            // Leading Link column, present on both rows when either needs it.
            let link_slot_w = (!cust_ok || !comp_ok).then(|| link_button().width(ui));
            ui.label(RichText::new("Customer").small().color(theme::weak_text(ui)));
            ui.horizontal(|ui| {
                if link_slot_w
                    .is_some_and(|w| link_slot(ui, w, !cust_ok, "Link this client to a customer"))
                {
                    let _ = tx.try_send(ClientUiAction::LinkCustomer(client.clone()));
                }
                let tone = if staff_machine && client.customer.is_none() {
                    theme::weak_text(ui)
                } else {
                    fk_color(ui, cust_ok, client.customer.is_some())
                };
                let text = RichText::new(&cust_label).small().color(tone);
                match client.customer.as_ref() {
                    Some(id) => {
                        let tip = if primary_task.is_some() {
                            "Open the service task modal (ticket & customer info)"
                        } else {
                            "Open the full customer record"
                        };
                        let link = IconLink::new(&cust_label)
                            .icon(svg::USER)
                            .tone(tone)
                            .small(true)
                            .show(ui);
                        if link.on_hover_text(tip).clicked() {
                            let _ = tx.try_send(match primary_task {
                                Some(task) => ClientUiAction::OpenClientTask {
                                    task: task.clone(),
                                    page: crate::modals::task_modal::ModalAction::TicketInfoPage,
                                },
                                None => ClientUiAction::ViewRecord(
                                    super::RecordKind::Customer,
                                    id.clone(),
                                ),
                            });
                        }
                    }
                    None => {
                        ui.label(text);
                    }
                }
            });
            ui.end_row();

            let comp_label = client
                .computer
                .as_ref()
                .map(|c| c.key_string().to_string())
                .unwrap_or_else(|| "(none)".into());
            ui.label(RichText::new("Computer").small().color(theme::weak_text(ui)));
            ui.horizontal(|ui| {
                if link_slot_w.is_some_and(|w| {
                    link_slot(ui, w, !comp_ok, "Create or repair this client's computer record")
                }) {
                    let _ = tx.try_send(ClientUiAction::LinkComputer(client.clone()));
                }
                let tone = fk_color(ui, comp_ok, client.computer.is_some());
                let text = RichText::new(&comp_label).small().color(tone);
                match client.computer.as_ref() {
                    Some(id) => {
                        let tip = if primary_task.is_some() {
                            "Open the service task modal on the computer page"
                        } else {
                            "Open the full computer record"
                        };
                        let link = IconLink::new(&comp_label)
                            .icon(svg::DESKTOP_TOWER)
                            .tone(tone)
                            .small(true)
                            .show(ui);
                        if link.on_hover_text(tip).clicked() {
                            let _ = tx.try_send(match primary_task {
                                Some(task) => ClientUiAction::OpenClientTask {
                                    task: task.clone(),
                                    page: crate::modals::task_modal::ModalAction::ComputerInfoPage,
                                },
                                None => ClientUiAction::ViewRecord(
                                    super::RecordKind::Computer,
                                    id.clone(),
                                ),
                            });
                        }
                    }
                    None => {
                        ui.label(text);
                    }
                }
            });
            ui.end_row();

            ui.label(RichText::new("Task").small().color(theme::weak_text(ui)));
            ui.scope(|ui| {
                ui.set_max_width(value_max_w);
                match client_tasks {
                    None => {
                        ui.horizontal(|ui| {
                            controls::spinner(ui, SpinnerSize::Sm);
                            ui.label(
                                RichText::new("(looking up…)")
                                    .small()
                                    .color(theme::faint_text(ui)),
                            );
                        });
                    }
                    Some([]) => {
                        ui.label(
                            RichText::new("(none)").small().color(theme::weak_text(ui)),
                        );
                    }
                    Some(tasks) => {
                        ui.vertical(|ui| {
                            for task in tasks.iter().take(5) {
                                let label = client_task_label(task);
                                if IconLink::new(&label)
                                    .icon(svg::TICKET)
                                    .small(true)
                                    .show(ui)
                                    .on_hover_text("Open the service task modal")
                                    .clicked()
                                {
                                    let _ = tx.try_send(ClientUiAction::OpenClientTask {
                                        task: task.clone(),
                                        page: crate::modals::task_modal::ModalAction::TicketInfoPage,
                                    });
                                }
                            }
                            if tasks.len() > 5 {
                                ui.label(
                                    RichText::new(format!("(+{} more)", tasks.len() - 5))
                                        .small()
                                        .color(theme::faint_text(ui)),
                                );
                            }
                        });
                    }
                }
            });
            ui.end_row();

            if let Some(created) = client.created_at.as_ref() {
                row(ui, "Created", &client_time(&created.to_string()), value_max_w);
            }
        });
}

fn row(ui: &mut Ui, key: &str, val: &str, value_max_w: f32) {
    ui.label(RichText::new(key).small().color(theme::weak_text(ui)));
    ui.scope(|ui| {
        ui.set_max_width(value_max_w);
        ui.label(RichText::new(val).small());
    });
    ui.end_row();
}

/// A [`row`] whose value is led by `icon` in `tone`; returns the value label's response.
fn icon_row(
    ui: &mut Ui,
    key: &str,
    icon: Icon,
    tone: Color32,
    val: &str,
    value_max_w: f32,
) -> eframe::egui::Response {
    ui.label(RichText::new(key).small().color(theme::weak_text(ui)));
    let value = ui
        .scope(|ui| {
            ui.set_max_width(value_max_w);
            ui.horizontal(|ui| {
                icon.show_tinted(ui, Metrics::get(ui.ctx()).icon_sm, tone);
                ui.label(RichText::new(val).small())
            })
            .inner
        })
        .inner;
    ui.end_row();
    value
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

/// One-line label for a matched task link: `name — #service (status)`.
fn client_task_label(t: &LiveTaskPayload) -> String {
    match &t.service_number {
        Some(sn) if !sn.is_empty() => format!("{} — #{} ({})", t.task_name, sn, t.status.as_str()),
        _ => format!("{} ({})", t.task_name, t.status.as_str()),
    }
}

/// Queue the matched-task lookup for an expanded row. Sends a pending marker
/// immediately so the fetch fires once, then resolves by linked computer
/// RecordId with a hostname fallback.
fn queue_client_tasks_fetch(
    tx: &crossbeam::channel::Sender<(String, Option<Vec<LiveTaskPayload>>)>,
    cache: &HashMap<String, Option<Vec<LiveTaskPayload>>>,
    client: &ConnectedClient,
) {
    if cache.contains_key(&client.connection_string) {
        return;
    }
    let cs = client.connection_string.clone();
    let _ = tx.try_send((cs.clone(), None));
    let computer = client.computer.clone();
    let hostname = cs
        .split_once(':')
        .map(|(h, _)| h.to_string())
        .unwrap_or_else(|| cs.clone());
    let tx = tx.clone();
    crate::PlatformSpawner::spawn(async move {
        let mut tasks: Vec<LiveTaskPayload> = Vec::new();
        if let Some(comp_id) = computer {
            match LiveTaskPayload::get_tasks_by_computer_id(&comp_id).await {
                Ok(found) => tasks = found,
                Err(e) => log::error!("client tasks: tasks-by-computer failed: {e:?}"),
            }
        }
        if tasks.is_empty() && !hostname.is_empty() {
            match LiveTaskPayload::get_tasks_by_hostname(&hostname).await {
                Ok(found) => tasks = found,
                Err(e) => log::error!("client tasks: tasks-by-hostname failed: {e:?}"),
            }
        }
        let _ = tx.try_send((cs, Some(tasks)));
    });
}

fn queue_fk_health_check(
    tx: &crossbeam::channel::Sender<(String, ClientLinkHealth)>,
    cache: &HashMap<String, ClientLinkHealth>,
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
        let customer_exists = match cust {
            Some(id) => matches!(record_exists(id).await, Ok(Some(true))),
            None => false,
        };
        let computer_exists = match comp {
            Some(id) => matches!(record_exists(id).await, Ok(Some(true))),
            None => false,
        };
        let staff_machine = database::schema::internal_computer_for_client(&cs)
            .await
            .ok()
            .flatten()
            .is_some();
        let _ = tx.try_send((
            cs,
            ClientLinkHealth { customer_exists, computer_exists, staff_machine },
        ));
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
        .spacing(eframe::egui::Vec2::new(8., 2.))
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
                let (color, icon, text) = match product.active {
                    Some(true) => (theme::success(ui), Some(svg::CHECK_CIRCLE), "Active"),
                    Some(false) => (theme::warn(ui), Some(svg::MINUS_CIRCLE), "Disabled"),
                    None => (theme::weak_text(ui), None, "—"),
                };
                ui.horizontal(|ui| {
                    if let Some(icon) = icon {
                        icon.show_tinted(ui, Metrics::get(ui.ctx()).icon_sm, color);
                    }
                    ui.label(RichText::new(text).small().color(color));
                });

                // Column 3: source pill — short and color-coded so
                // the eye can pick out "is this data trustworthy?"
                // without reading the full word.
                let (src_color, src_text) = match product.source {
                    SecurityProductSource::SecurityCenter => {
                        (theme::info(ui), "SecurityCenter")
                    }
                    SecurityProductSource::Registry => {
                        (theme::text(ui), "Registry")
                    }
                    SecurityProductSource::Heuristic => {
                        (theme::weak_text(ui), "Heuristic")
                    }
                };
                controls::badge(ui, src_text, src_color);

                ui.end_row();
            }
        });
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::tabs::admin_console::client_interface::TransportKind;
    use crate::ui_tools::{headless_render::Headless, metrics::Density, theme_config::PresetStyles};
    use chrono::FixedOffset;
    use database::schema::{Datetime, RecordId};
    use eframe::egui::{vec2, Panel};

    const CREATED: &str = "2026-10-07T19:45:04.302330730Z";

    /// Clients plus the channels and caches their rows read.
    pub(crate) struct Fixture {
        clients: Vec<ConnectedClient>,
        tx: Sender<ClientUiAction>,
        fk_tx: Sender<(String, ClientLinkHealth)>,
        task_tx: Sender<(String, Option<Vec<LiveTaskPayload>>)>,
        fk_cache: HashMap<String, ClientLinkHealth>,
        task_cache: HashMap<String, Option<Vec<LiveTaskPayload>>>,
    }

    impl Fixture {
        /// `count` linked clients whose link and task lookups are already cached.
        pub(crate) fn new(count: usize) -> Self {
            let created = DateTime::parse_from_rfc3339(CREATED).expect("valid time");
            let clients: Vec<ConnectedClient> = (0..count)
                .map(|i| ConnectedClient {
                    connection_string: format!("BENCH-{i:02}:abc123def"),
                    friendly_name: Some(format!("Bench PC {i}")),
                    connected: true,
                    local_ip: Some("10.0.0.5".into()),
                    tcp_port: Some(8000),
                    customer: Some(RecordId::new("customer", "riv_4412")),
                    computer: Some(RecordId::new("computer", "bench_pc")),
                    created_at: Some(Datetime::from(created.with_timezone(&Utc))),
                    ..Default::default()
                })
                .collect();
            let fk_cache = clients
                .iter()
                .map(|c| (c.connection_string.clone(), ClientLinkHealth::default()))
                .collect();
            let task_cache = clients
                .iter()
                .map(|c| (c.connection_string.clone(), Some(Vec::new())))
                .collect();
            Self {
                clients,
                tx: crossbeam::channel::unbounded().0,
                fk_tx: crossbeam::channel::unbounded().0,
                task_tx: crossbeam::channel::unbounded().0,
                fk_cache,
                task_cache,
            }
        }
    }

    /// Side panel `width` wide listing `groups` with client 0 expanded; returns the panel and row rects.
    pub(crate) fn side_panel(
        ui: &mut Ui,
        width: f32,
        fx: &Fixture,
        groups: &[(Option<String>, Vec<usize>)],
    ) -> (Rect, Vec<Rect>) {
        let frame = Frame::default()
            .fill(glass_card::recessed_fill(ui))
            .inner_margin(Margin::same(1))
            .outer_margin(Margin::same(1))
            .stroke(glass_card::card_stroke(ui))
            .corner_radius(eframe::egui::CornerRadius::same(
                ui.visuals().window_corner_radius.nw.max(2),
            ));
        let mut rows = Vec::new();
        let panel = Panel::left("admin_client_list_test")
            .frame(frame)
            .show_separator_line(false)
            .exact_size(width)
            .show(ui, |ui| {
                ui.with_layout(Layout::top_down(Align::Min), |ui| {
                    client_list(ui, f32::INFINITY, groups, |ui, index, row_w| {
                        let client = &fx.clients[index];
                        let first = index == 0;
                        if first {
                            let id = client_row_id(ui, &client.connection_string);
                            let mut state = CollapsingState::load_with_default_open(ui.ctx(), id, true);
                            state.set_open(true);
                            state.store(ui.ctx());
                        }
                        rows.push(AdminConsole::client_header(
                            ui,
                            row_w,
                            fx.tx.clone(),
                            client,
                            HashMap::new(),
                            None,
                            first,
                            &fx.fk_tx,
                            &fx.fk_cache,
                            &fx.task_tx,
                            &fx.task_cache,
                            None,
                            None,
                            first.then_some((TransportKind::Tcp, true)),
                            first.then_some("5.0.0+8d4bec94"),
                        ));
                    });
                });
            });
        (panel.response.rect, rows)
    }

    /// Each row's `(left, right)` gap to the panel edge, whether the rows overflow the panel, and the indent.
    fn gutters(width: f32, count: usize, grouped: bool) -> (Vec<(f32, f32)>, bool, f32) {
        let mut h = Headless::new(vec2(width + 100.0, 900.0), 1.0);
        h.theme(PresetStyles::MtechStudio, Density::Regular);
        let fixture = Fixture::new(count);
        let indices: Vec<usize> = (0..count).collect();
        let groups = if grouped {
            let (riv, ltn) = indices.split_at(count / 2);
            vec![(Some("LTN".to_string()), ltn.to_vec()), (Some("RIV".to_string()), riv.to_vec())]
        } else {
            vec![(None, indices)]
        };
        let mut drawn = (Rect::NOTHING, Vec::new());
        for _ in 0..3 {
            drawn = h.frame_root(Vec::new(), |ui| side_panel(ui, width, &fixture, &groups)).0;
        }
        let (panel, rows) = drawn;
        assert_eq!(rows.len(), count);
        let overflows = rows.iter().any(|r| r.bottom() > panel.bottom());
        let gaps = rows
            .iter()
            .map(|r| (r.left() - panel.left(), panel.right() - r.right()))
            .collect();
        (gaps, overflows, h.ctx.global_style().spacing.indent)
    }

    #[test]
    fn client_rows_keep_equal_gutters_at_any_width_with_or_without_a_scrollbar() {
        for width in [400.0, 500.0] {
            for grouped in [true, false] {
                for (count, scrolls) in [(2, false), (30, true)] {
                    let (gaps, overflows, indent) = gutters(width, count, grouped);
                    assert_eq!(overflows, scrolls, "width {width}, grouped {grouped}, {count} rows");
                    for (left, right) in gaps {
                        assert!(
                            (left - right).abs() <= 0.5,
                            "width {width}, grouped {grouped}, {count} rows: left {left} right {right}"
                        );
                        assert!(left >= indent, "width {width}: left gutter {left}");
                    }
                }
            }
        }
    }

    #[test]
    fn client_times_use_the_last_update_format() {
        assert_eq!(format_client_time(CREATED, &Utc), "2026/10/07 @ 07:45PM");
        let mountain = FixedOffset::west_opt(6 * 3600).expect("valid offset");
        assert_eq!(
            format_client_time("2026-10-13T18:01:00Z", &mountain),
            "2026/10/13 @ 12:01PM"
        );
        assert_eq!(client_time(CREATED), format_client_time(CREATED, &Local));
        assert_eq!(client_time("not a time"), "not a time");

        let stored = Datetime::from(
            DateTime::parse_from_rfc3339(CREATED)
                .expect("valid time")
                .with_timezone(&Utc),
        );
        assert_eq!(client_time(&stored.to_string()), client_time(CREATED));
    }

    #[test]
    fn version_cell_prefers_the_recorded_build() {
        let (text, tip) = version_cell(Some("5.0.1+aaaa"), Some("5.0.1+aaaa"), true);
        assert_eq!(text, "v5.0.1+aaaa");
        assert!(tip.contains("last check-in"));

        let (text, tip) = version_cell(Some("5.0.1+aaaa"), None, false);
        assert_eq!(text, "v5.0.1+aaaa");
        assert!(tip.contains("last check-in"));

        let (text, tip) = version_cell(Some("5.0.0+bbbb"), Some("5.0.1+aaaa"), true);
        assert_eq!(text, "v5.0.0+bbbb");
        assert!(tip.contains("v5.0.1+aaaa"));
    }

    #[test]
    fn version_cell_falls_back_to_the_session_then_a_dash() {
        let (text, tip) = version_cell(None, Some("5.0.0+bbbb"), true);
        assert_eq!(text, "v5.0.0+bbbb");
        assert!(tip.contains("open session"));

        let (text, tip) = version_cell(None, None, true);
        assert_eq!(text, "—");
        assert!(tip.contains("has not reported"));

        let (text, tip) = version_cell(None, None, false);
        assert_eq!(text, "—");
        assert!(tip.contains("Not recorded"));
    }
}

#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::tabs::admin_console::client_interface::{
        client_log_viewer::ClientLogViewer, event_log_viewer::EventLogViewer,
        installed_programs_viewer::InstalledProgramsViewer, registry_editor::RegistryEditor,
        remote_explorer::RemoteExplorer, remote_scripts_viewer::RemoteScriptsViewer,
        services_viewer::ServicesViewer, startup_apps_viewer::StartupAppsViewer,
        tabs::mcp_tool_log_viewer::McpToolLogViewer, TransportKind,
    };
    use crate::tabs::admin_console::{relink_popup::RelinkClientPopup, rename_popup::RenameClientPopup};
    use crate::ui_tools::{headless_render::Headless, metrics::Density, theme_config::PresetStyles};
    use eframe::egui::vec2;
    use super::tests::{side_panel, Fixture};

    fn save(h: &mut Headless, name: &str, f: impl FnMut(&mut Ui)) {
        let dir = std::env::var_os("MTECH_RENDER_DIR").expect("MTECH_RENDER_DIR");
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let (_, img) = h.render(5, f);
        img.save(dir.join(format!("{name}.png"))).expect("png");
    }

    #[test]
    #[ignore = "writes PNG previews"]
    fn render_admin_console_views() {
        let client = ConnectedClient {
            connection_string: "BENCH-PC:abc123def".into(),
            friendly_name: Some("Bench PC".into()),
            connected: true,
            local_ip: Some("10.0.0.5".into()),
            tcp_port: Some(8000),
            ..Default::default()
        };
        let (cmd_tx, _cmd_rx) = crossbeam::channel::unbounded::<crate::Cmd>();
        let (action_tx, _action_rx) = crossbeam::channel::unbounded();
        let (fk_tx, _fk_rx) = crossbeam::channel::unbounded();
        let (task_tx, _task_rx) = crossbeam::channel::unbounded();
        let presets = [("studio", PresetStyles::MtechStudio), ("classic", PresetStyles::ShippedClassic)];
        for (tag, preset) in presets {
            let mut h = Headless::new(vec2(420.0, 420.0), 1.0);
            h.theme(preset, Density::Regular);
            save(&mut h, &format!("{tag}_client_row"), |ui| {
                AdminConsole::client_header(
                    ui,
                    CLIENT_ROW_W,
                    action_tx.clone(),
                    &client,
                    HashMap::new(),
                    Some(client.connection_string.as_str()),
                    true,
                    &fk_tx,
                    &HashMap::new(),
                    &task_tx,
                    &HashMap::new(),
                    None,
                    None,
                    Some((TransportKind::Tcp, true)),
                    None,
                );
                let content_w = CLIENT_ROW_W - row_frame(ui).total_margin().sum().x;
                client_details_grid(
                    ui, content_w, &action_tx, &client, "2026/10/12 @ 09:00AM", "(none)", true,
                    None, None, Some(&[][..]), Some("5.0.0+8d4bec94"),
                );
            });

            for (width, count) in [(400.0, 3), (500.0, 3), (400.0, 12)] {
                let mut h = Headless::new(vec2(width, 640.0), 1.0);
                h.theme(preset, Density::Regular);
                let fixture = Fixture::new(count);
                let groups = [
                    (Some("RIV".to_string()), (0..count - 1).collect()),
                    (Some("LTN".to_string()), vec![count - 1]),
                ];
                let mut prims = Vec::new();
                for _ in 0..5 {
                    prims = h.frame_root(Vec::new(), |ui| side_panel(ui, width, &fixture, &groups)).1;
                }
                let dir = std::env::var_os("MTECH_RENDER_DIR").expect("MTECH_RENDER_DIR");
                let name = format!("{tag}_client_list_{width}_{count}.png");
                h.image(&prims).save(std::path::PathBuf::from(dir).join(name)).expect("png");
            }

            let mut h = Headless::new(vec2(900.0, 360.0), 1.0);
            h.theme(preset, Density::Regular);
            let mut services = ServicesViewer::new();
            save(&mut h, &format!("{tag}_services"), |ui| services.display(ui, &cmd_tx));

            let mut h = Headless::new(vec2(1000.0, 360.0), 1.0);
            h.theme(preset, Density::Regular);
            let mut events = EventLogViewer::new();
            save(&mut h, &format!("{tag}_event_log"), |ui| events.display(ui, &cmd_tx));

            let mut h = Headless::new(vec2(900.0, 360.0), 1.0);
            h.theme(preset, Density::Regular);
            let mut startup = StartupAppsViewer::new();
            save(&mut h, &format!("{tag}_startup"), |ui| startup.display(ui, &cmd_tx));

            let mut h = Headless::new(vec2(900.0, 420.0), 1.0);
            h.theme(preset, Density::Regular);
            let mut registry = RegistryEditor::new();
            save(&mut h, &format!("{tag}_registry"), |ui| registry.display(ui, &cmd_tx));

            let mut h = Headless::new(vec2(900.0, 420.0), 1.0);
            h.theme(preset, Density::Regular);
            let mut scripts = RemoteScriptsViewer::new();
            scripts.set_script_list(vec![(
                "Cleanup".into(),
                vec![
                    crate::RemoteScriptItem {
                        name: "Clear temp files".into(),
                        category: "Cleanup".into(),
                        content: None,
                    },
                    crate::RemoteScriptItem {
                        name: "custom.ps1".into(),
                        category: "Cleanup".into(),
                        content: Some("Write-Host hi".into()),
                    },
                ],
            )]);
            save(&mut h, &format!("{tag}_scripts"), |ui| scripts.display(ui, &cmd_tx));

            let mut h = Headless::new(vec2(1000.0, 300.0), 1.0);
            h.theme(preset, Density::Regular);
            let mut mcp = McpToolLogViewer::new();
            save(&mut h, &format!("{tag}_mcp_log"), |ui| mcp.display(ui, "BENCH-PC:abc123def"));

            let mut h = Headless::new(vec2(520.0, 300.0), 1.0);
            h.theme(preset, Density::Regular);
            let mut rename = RenameClientPopup::new(&client);
            save(&mut h, &format!("{tag}_rename"), |ui| {
                rename.ui(ui.ctx());
            });

            let mut h = Headless::new(vec2(1000.0, 300.0), 1.0);
            h.theme(preset, Density::Regular);
            let mut log = ClientLogViewer::new();
            log.set_response("12:00:01 INFO started\n12:00:02 WARN slow disk".into(), 2, 40);
            save(&mut h, &format!("{tag}_client_log"), |ui| log.display(ui, &cmd_tx));

            let mut h = Headless::new(vec2(900.0, 300.0), 1.0);
            h.theme(preset, Density::Regular);
            let mut programs = InstalledProgramsViewer::new();
            save(&mut h, &format!("{tag}_programs"), |ui| programs.display(ui, &cmd_tx));

            let mut h = Headless::new(vec2(640.0, 420.0), 1.0);
            h.theme(preset, Density::Regular);
            let mut relink = RelinkClientPopup::new(client.clone());
            save(&mut h, &format!("{tag}_relink"), |ui| {
                relink.ui(ui.ctx());
            });

            let mut h = Headless::new(vec2(1100.0, 420.0), 1.0);
            h.theme(preset, Density::Regular);
            let mut explorer = RemoteExplorer::new();
            explorer.set_drives(vec!["C:\\".into(), "D:\\".into()]);
            save(&mut h, &format!("{tag}_explorer"), |ui| explorer.display(ui, &cmd_tx));
        }
    }
}
