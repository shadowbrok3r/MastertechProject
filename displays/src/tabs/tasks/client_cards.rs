//! Card UI for connected clients shown in the My Tasks board.
//!
//! Each card represents a `ConnectedClient` row from SurrealDB enriched with
//! the latest `SystemInformation` snapshot from the admin-console websocket
//! (when present) and a flag indicating whether an AI agent is currently
//! running a `DiagnosticSession` against this connection. Cards expose
//! buttons to open the Admin Console, open a per-client diagnostics popup,
//! and (when linked) jump to the related service task.

use crate::TaskUiActions;
use crossbeam::channel::Sender;
use database::schema::{
    ComputerData, ConnectedClient, LiveTaskPayload, RecordIdExt, SystemInformation,
};
use eframe::egui::{
    Button, Color32, CornerRadius, Frame, Margin, ProgressBar, RichText, Stroke, Ui, Vec2, Widget,
};

/// Heartbeat-freshness threshold for the header dot.
///
/// The agent's heartbeat writer (`Mastertech4.0/src/tcp_listener.rs`,
/// task spawned alongside `accept_loop`) bumps `last_update = time::now()`
/// every 60 s. A heartbeat older than this window means we've missed
/// **three** consecutive writes — strong evidence the agent process is
/// stuck or the network path to SurrealDB is broken — so we surface the
/// client as "stale" even when its DB row still says `connected = true`.
/// The database-side sweep (axum_server cron, ~every minute) will flip
/// the flag to `false` shortly after; this threshold is the UI showing
/// the same conclusion a few seconds earlier.
const STALE_THRESHOLD_SECS: i64 = 180;

/// Include a client in My Tasks / Admin Console summaries only if the DB
/// `last_update` is newer than this **or** an admin transport session is live.
pub const CONNECTED_CLIENT_SUMMARY_MAX_STALE_SECS: i64 = 2 * 3600;

fn last_update_within_secs(client: &ConnectedClient, max_age_secs: i64) -> bool {
    let Some(ref dt) = client.last_update else {
        return false;
    };
    match chrono::DateTime::parse_from_rfc3339(&dt.to_string()) {
        Ok(t) => (chrono::Utc::now() - t.with_timezone(&chrono::Utc)).num_seconds()
            < max_age_secs,
        Err(_) => false,
    }
}

/// Whether this client should appear in My Tasks "Connected Clients" and the
/// Admin Console client list. Live TCP/WebSocket admin sessions always qualify.
///
/// **Visibility = "online" not "direct-TCP reachable".** Earlier
/// iterations gated this on whether the admin's TCP probe had
/// successfully connected to the client's advertised
/// `local_ip:tcp_port`. That turned out to be the wrong
/// semantic: a client whose direct TCP we can't reach (admin and
/// client on different LANs, NAT, firewall) is still fully
/// usable via the WebSocket *relay* path. Hiding it produced
/// the worst-of-both-worlds bug — a client that notified online
/// would never appear in the list even though we could in fact
/// open a session against it via relay.
///
/// We're back to two requirements:
///   1. Either a live admin session is already open, OR
///   2. The DB heartbeat is recent enough that we expect the
///      client to respond at all.
///
/// Direct-TCP reachability is still tracked (the per-admin
/// reachability prober) — it just feeds the UI as informational
/// metadata, and (in a follow-up) the `ConnectClient` handler
/// can use it to skip directly to relay when TCP is known to
/// fail. It no longer affects *visibility*.
#[must_use]
pub fn should_show_connected_client_in_summaries(
    client: &ConnectedClient,
    is_live_admin_transport: bool,
) -> bool {
    if is_live_admin_transport {
        return true;
    }
    last_update_within_secs(client, CONNECTED_CLIENT_SUMMARY_MAX_STALE_SECS)
}

fn recently_active(client: &ConnectedClient) -> bool {
    last_update_within_secs(client, STALE_THRESHOLD_SECS)
}

/// Snapshot of a connected client for rendering as a card on the My Tasks
/// board. Built each frame by `SharedContext::render_layout` from
/// `SharedContext::clients`, the admin-console `ws_clients` map, and the
/// `active_diagnostic_sessions` registry.
#[derive(Clone)]
pub struct ClientCardData {
    pub client: ConnectedClient,
    pub system_info: Option<SystemInformation>,
    pub ai_active: bool,
    pub active_session_id: Option<String>,
    /// The full computer record (if loaded). Used to surface a "Service #N"
    /// chip when this computer is checked in for a current task.
    pub computer_data: Option<ComputerData>,
    /// The task currently associated with this client's computer (if any).
    pub linked_task: Option<LiveTaskPayload>,
    /// Whether there is an active admin TCP/WebSocket session open to this
    /// client right now. Used to set the connection status dot.
    pub is_ws_connected: bool,
}

impl ClientCardData {
    pub fn new(client: ConnectedClient) -> Self {
        Self {
            client,
            system_info: None,
            ai_active: false,
            active_session_id: None,
            computer_data: None,
            linked_task: None,
            is_ws_connected: false,
        }
    }

    pub fn display_client_card(&self, ui: &mut Ui, tx: &Sender<TaskUiActions>) {
        let card_frame = Frame::default()
            .fill(ui.style().visuals.faint_bg_color)
            .stroke(Stroke::new(0.7_f32, ui.style().visuals.weak_text_color()))
            .inner_margin(Margin::same(8))
            .corner_radius(CornerRadius::same(6));

        card_frame.show(ui, |ui| {
            ui.set_min_width(420.0);
            ui.vertical(|ui| {
                self.header_row(ui);
                ui.add_space(4.0);
                self.stats_row(ui);
                ui.add_space(6.0);
                self.button_row(ui, tx);
                if let Some(task) = self.linked_task.as_ref() {
                    ui.add_space(4.0);
                    self.linked_task_chip(ui, task, tx);
                }
            });
        });
    }

    fn header_row(&self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            // Heartbeat-driven three-state dot. The dot reflects whether
            // the *agent process* is alive — independent of whether the
            // operator currently has a session open to it. (For the
            // "session open" signal we render a separate SESSION chip
            // alongside, so a live session never masks a dying heartbeat.)
            //
            // - green  : `connected = true` AND `last_update` within
            //            STALE_THRESHOLD_SECS (3 min) — agent is heart-
            //            beating and the DB hasn't swept us yet.
            // - yellow : `connected = true` but heartbeat is stale —
            //            the sweep cron will soon flip us to `false`;
            //            the UI shouldn't lie about "online" in the
            //            meantime.
            // - gray   : `connected = false` (or no row) — offline.
            let fresh = recently_active(&self.client);
            let (dot_color, dot_tip) = if self.client.connected && fresh {
                (Color32::from_rgb(50, 205, 50), "Online (heartbeat fresh)")
            } else if self.client.connected && !fresh {
                (
                    Color32::from_rgb(255, 200, 0),
                    "Stale — no heartbeat for over 3 minutes",
                )
            } else {
                (Color32::from_rgb(110, 110, 118), "Offline")
            };
            let (rect, resp) = ui.allocate_exact_size(
                Vec2::splat(10.0),
                eframe::egui::Sense::hover(),
            );
            ui.painter().circle_filled(rect.center(), 5.0, dot_color);
            resp.on_hover_text(dot_tip);

            let title = self
                .client
                .friendly_name
                .clone()
                .unwrap_or_else(|| self.client.connection_string.clone());
            ui.label(RichText::new(title).strong());

            // Separate "live admin session" indicator. Distinct from the
            // dot above because we want operators to see at a glance both
            // (a) is the agent alive (dot), and (b) am I already wired in
            // to it (chip). Conflating the two would hide a dying
            // heartbeat behind a green dot just because we happen to have
            // a TCP session open from before the heartbeat went stale.
            if self.is_ws_connected {
                ui.add_space(6.0);
                ui.label(
                    RichText::new("• SESSION")
                        .color(Color32::from_rgb(120, 220, 140))
                        .strong()
                        .small(),
                )
                .on_hover_text("Admin transport session is currently open to this client");
            }

            if self.ai_active {
                ui.add_space(6.0);
                ui.label(
                    RichText::new("• AI ACTIVE")
                        .color(Color32::from_rgb(250, 180, 60))
                        .strong()
                        .small(),
                );
            }

            // Show time since last DB heartbeat at the right edge.
            if let Some(ref dt) = self.client.last_update {
                if let Ok(t) = chrono::DateTime::parse_from_rfc3339(&dt.to_string()) {
                    let secs = (chrono::Utc::now()
                        - t.with_timezone(&chrono::Utc))
                    .num_seconds()
                    .max(0);
                    let ago = if secs < 60 {
                        format!("{secs}s ago")
                    } else if secs < 3600 {
                        format!("{}m ago", secs / 60)
                    } else {
                        format!("{}h ago", secs / 3600)
                    };
                    ui.with_layout(
                        eframe::egui::Layout::right_to_left(eframe::egui::Align::Center),
                        |ui| {
                            ui.label(RichText::new(ago).weak().small());
                        },
                    );
                }
            }
        });

        // Sub-line: assigned username + IP (when available)
        let assigned_name: Option<String> = self.client.assigned_user.as_ref().and_then(|uid| {
            let users = crate::get_database_users();
            users
                .iter()
                .find(|u| u.get_id().key_string() == uid.key_string())
                .map(|u| u.get_name().to_string())
        });
        let ip_str = self.client.local_ip.as_deref().filter(|s| !s.is_empty());

        match (assigned_name.as_deref(), ip_str) {
            (Some(name), Some(ip)) => {
                ui.label(
                    RichText::new(format!("{name}  •  {ip}"))
                        .weak()
                        .small(),
                );
            }
            (Some(name), None) => {
                ui.label(RichText::new(name).weak().small());
            }
            (None, Some(ip)) => {
                ui.label(RichText::new(ip).weak().small());
            }
            _ => {}
        }

        if let Some(sysinfo) = self.system_info.as_ref() {
            ui.label(
                RichText::new(format!("{} • {}", sysinfo.hostname, sysinfo.os_version))
                    .weak()
                    .small(),
            );
        } else if !self.client.connection_string.is_empty() {
            ui.label(
                RichText::new(&self.client.connection_string)
                    .weak()
                    .small(),
            );
        }
    }

    fn stats_row(&self, ui: &mut Ui) {
        let Some(info) = self.system_info.as_ref() else {
            ui.label(RichText::new("No live stats").weak().small());
            return;
        };

        ui.horizontal(|ui| {
            ui.label(RichText::new("CPU").small());
            let cpu_pct = (info.cpu_percentage / 100.0).clamp(0.0, 1.0);
            ProgressBar::new(cpu_pct)
                .desired_width(120.0)
                .text(format!("{:.0}%", info.cpu_percentage))
                .ui(ui);

            ui.add_space(8.0);
            ui.label(RichText::new("RAM").small());
            let ram_pct = if info.total_memory > 0.0 {
                (info.used_memory / info.total_memory).clamp(0.0, 1.0)
            } else {
                0.0
            };
            ProgressBar::new(ram_pct)
                .desired_width(120.0)
                .text(format!(
                    "{:.1}/{:.1} GB",
                    info.used_memory / 1024.0,
                    info.total_memory / 1024.0
                ))
                .ui(ui);
        });

        if let Some(gpu) = info.gpu_info.usage.first() {
            ui.horizontal(|ui| {
                ui.label(RichText::new("GPU").small());
                let gpu_pct = (gpu.gpu as f32 / 100.0).clamp(0.0, 1.0);
                ProgressBar::new(gpu_pct)
                    .desired_width(120.0)
                    .text(format!("{}% • {}°C", gpu.gpu, gpu.temperature))
                    .ui(ui);
            });
        }
    }

    fn button_row(&self, ui: &mut Ui, tx: &Sender<TaskUiActions>) {
        ui.horizontal(|ui| {
            if Button::new("🔬 Diagnostics").small().ui(ui).clicked() {
                let _ = tx.try_send(TaskUiActions::OpenClientDiagnostics(
                    self.client.connection_string.clone(),
                ));
            }
            if Button::new("🖥 Open Console").small().ui(ui).clicked() {
                let _ = tx.try_send(TaskUiActions::OpenAdminConsole(
                    self.client.connection_string.clone(),
                ));
            }
        });
    }

    fn linked_task_chip(&self, ui: &mut Ui, task: &LiveTaskPayload, tx: &Sender<TaskUiActions>) {
        let label = match task.service_number.as_deref() {
            Some(s) if !s.is_empty() => format!("Service #{s}"),
            _ => format!("Task {}", task.id.key_string()),
        };
        let chip = Button::new(RichText::new(label).color(Color32::from_rgb(100, 200, 255)))
            .small()
            .ui(ui);
        if chip.clicked() {
            let _ = tx.try_send(TaskUiActions::OpenTaskModal(task.clone()));
        }
    }
}
