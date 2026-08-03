//! Per-client Home overview — RMM-style landing page.
//!
//! Top: hardware inventory + active stress runs (live indicators with
//! countdown to `duration_planned_secs`).
//!
//! Body: sub-tab nav with two options:
//!   - **Overview** — the consolidated live-telemetry view. Calls
//!     `ResourceMonitor::show_compact_overview` which renders the
//!     chart_board grid plus collapsible per-section tables (machine,
//!     GPUs, WHEA, memory, cores, disks, networks) — no combobox switch,
//!     everything visible on one scroll.
//!   - **Processes** — `ProcessTableViewer` rendered at full height
//!     because the table needs the space.
//!
//! Active stress-run detection layers two sources (per the operator's
//! design choice, "Both — prefer log stream, fall back to DB"):
//!   1. Parse the `RemoteScriptsViewer.log_messages` stream for the
//!      stress-runner status lines we know — `Starting: …`,
//!      `stress_test_run id: …`, `… PASSED in …s` / `… FAILED in …s`.
//!      This is the fastest signal and works the moment the script
//!      starts, before any DB write has landed.
//!   2. Poll `stress_test_run` rows for `result = 'in_progress'` against
//!      this client's computer id every ~1 s. Catches runs started
//!      outside this admin session and gives canonical truth.

use crate::ui_tools::{icons, theme};
use crate::Cmd;
use crate::{PlatformSpawner, Spawner};
use database::schema::{HardwareComponent, HardwareKind, RecordId, RecordIdExt, SystemInformation};
use database::db;
use eframe::egui::{
    Align, Button, Id, Label, Layout, ProgressBar, RichText, ScrollArea, Ui, Vec2, Window,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::command_shell::History;
use super::super::ui::WsDisplayState;
use super::super::WebSocketClient;

const ACTIVE_RUN_POLL_INTERVAL: Duration = Duration::from_secs(1);
const INVENTORY_POLL_INTERVAL: Duration = Duration::from_secs(5);
/// How often to re-check the live sysinfo for hardware that hasn't been
/// stress-tested yet and upsert a `hardware_component` row so it shows
/// up as a phantom inventory card. One minute is plenty — sysinfo only
/// changes when the operator swaps hardware between sessions.
const PHANTOM_ENSURE_INTERVAL: Duration = Duration::from_secs(60);
/// How many past runs we keep per component card. Keeps the inventory
/// from ballooning vertically while still showing enough history that
/// a regression is visible at a glance.
const HISTORY_PER_COMPONENT: usize = 5;
/// How long a `SetDriverProtections` may stay in flight before the row
/// stops waiting. Matches the transport's 45 s liveness window, after
/// which no reply is coming; the revert button must be usable again.
const DRIVER_PROTECTIONS_TIMEOUT: Duration = Duration::from_secs(45);
/// How long a successful driver-protections outcome stays on the toolbar.
const DRIVER_PROTECTIONS_STATUS_OK_TTL: Duration = Duration::from_secs(20);
/// How long a failed driver-protections outcome stays on the toolbar.
const DRIVER_PROTECTIONS_STATUS_ERR_TTL: Duration = Duration::from_secs(90);
/// Width cap for the toolbar's driver-protections status label.
const DRIVER_PROTECTIONS_STATUS_MAX_WIDTH: f32 = 300.0;
/// Share of the remaining toolbar width the status label may take.
const DRIVER_PROTECTIONS_STATUS_ROW_SHARE: f32 = 0.35;
/// How many driver-protections audit records the `computer` row keeps.
const DRIVER_PROTECTIONS_HISTORY_KEEP: i64 = 50;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HomeSubTab {
    Overview,
    Processes,
}

impl Default for HomeSubTab {
    fn default() -> Self {
        Self::Overview
    }
}

/// One in-progress stress run as the Home page sees it. Sourced from
/// the database poll OR the log-stream parser; both feed the same
/// shape so the UI is uniform.
#[derive(Clone, Debug)]
pub struct ActiveRun {
    pub run_id: String,
    pub tool_label: String,
    pub started_at: Instant,
    pub duration_planned_secs: Option<u64>,
    pub source: ActiveRunSource,
    /// `hardware_component` id this run targets, used to dock the
    /// progress bar inside the matching inventory card. Only the
    /// database-sourced rows populate this — log-stream entries
    /// never have a component link.
    pub target_component: Option<String>,
    /// Stage breakdown for multi-stage scenarios. Parsed from log
    /// lines like `Stage 3/4: gpu_vram`. None on single-stressor runs
    /// and on DB rows (the per-stage detail lives in stress_test_event).
    pub current_stage_idx: Option<u32>,
    pub current_stage_total: Option<u32>,
    pub current_stage_name: Option<String>,
    /// Every `hardware_component` id this run touched (target + the
    /// extras in `touched_components`). Lets the inventory dock the
    /// progress bar inside every card the run is exercising, not just
    /// the primary target — e.g. a `gpu_pcie` run touches CPU + GPU,
    /// and the bar should appear under both cards.
    pub touched_components: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActiveRunSource {
    LogStream,
    Database,
}

impl ActiveRunSource {
    fn label(self) -> &'static str {
        match self {
            Self::LogStream => "log stream",
            Self::Database => "DB",
        }
    }
}

/// User-driven actions collected while rendering the Home page. Drained
/// in `show_home` after layout so click handlers don't have to fight
/// the borrow checker over `&mut self.home_page` + `&mut self`.
#[derive(Default)]
pub struct HomeActions {
    /// `stress_test_run:<key>` ids to open in the MCP Tool Log
    /// breadcrumb. Same navigation pattern as the MCP viewer's
    /// `open_record` flow.
    pub open_records: Vec<String>,
    /// Tool labels the operator clicked "Re-run" on. The host (Web
    /// Console) flips to the Scripts tab so the operator can re-launch
    /// the matching script manually — full auto-rerun would need the
    /// script-name ↔ tool_label mapping which lives in the stress
    /// scripts catalog.
    pub reruns: Vec<String>,
}

/// One past run summary, used both in the inventory cards' history
/// strip and the standalone "recent activity" tail.
#[derive(Clone, Debug)]
pub struct PastRun {
    pub run_id: String,
    pub tool_label: String,
    pub result: String,
    pub started_at_label: String,
    pub started_at_unix_ms: i64,
}

/// One hardware_component card rendered on the Home page.
#[derive(Clone, Debug)]
pub struct InventoryCard {
    pub component_id: String,
    pub display_name: String,
    pub kind: String,
    pub vendor: String,
    pub model: String,
    pub recent_runs: Vec<PastRun>,
}

pub struct HomePage {
    sub_tab: HomeSubTab,
    /// Most recent active-run set polled from the DB. Populated by an
    /// async fetcher; cleared by the next poll.
    db_runs: Arc<Mutex<Vec<ActiveRun>>>,
    /// Active runs derived from RemoteScripts log parsing. Keyed by
    /// script name so a fresh `Starting:` line replaces a stale entry.
    log_runs: HashMap<String, ActiveRun>,
    /// Cursor into RemoteScriptsViewer.log_messages so we only parse
    /// new lines on each frame.
    log_cursor: usize,
    last_poll: Option<Instant>,
    /// Per-component inventory rolled up from the historical
    /// `stress_test_run` rows for this client's `computer`.
    inventory: Arc<Mutex<Vec<InventoryCard>>>,
    last_inventory_poll: Option<Instant>,
    /// When we last upserted phantom `hardware_component` rows from
    /// the client's `SystemInformation`. Throttled so the admin
    /// doesn't write the same rows every frame.
    last_phantom_ensure: Option<Instant>,
    /// Set by the phantom-upsert spawned task when it finishes a batch;
    /// the egui thread checks it once per frame and, if set, invalidates
    /// `last_inventory_poll` so the next render fires a fresh poll and
    /// the new phantom cards appear immediately.
    phantom_pending: Arc<std::sync::atomic::AtomicBool>,
    /// Driver-protections toggle awaiting the operator's confirm click;
    /// holds the requested `enable` value.
    driver_protections_pending: Option<bool>,
    /// Last driver-protections outcome.
    driver_protections_status: Option<DriverProtectionsStatus>,
    /// Set between sending `SetDriverProtections` and its result arriving.
    driver_protections_busy: bool,
    /// When the in-flight `SetDriverProtections` was sent; expires the busy
    /// flag after `DRIVER_PROTECTIONS_TIMEOUT`.
    driver_protections_sent_at: Option<Instant>,
    /// The `SetDriverProtections` still awaiting a reply; cleared on a matching
    /// result and on timeout.
    driver_protections_outstanding: Option<DriverProtectionsRequest>,
}

/// One `SetDriverProtections` awaiting its `DriverProtectionsResult`.
#[derive(Clone, Debug)]
struct DriverProtectionsRequest {
    id: String,
    /// True for a re-enable, false for a disable.
    enable: bool,
}

/// One driver-protections outcome: `terse` renders on the toolbar, `detail`
/// on hover.
#[derive(Clone, Debug)]
struct DriverProtectionsStatus {
    terse: String,
    detail: String,
    ok: bool,
    /// When the outcome landed; the toolbar drops it after its TTL.
    shown_at: Instant,
}

impl DriverProtectionsStatus {
    fn new(ok: bool, terse: String, detail: String) -> Self {
        Self {
            terse,
            detail,
            ok,
            shown_at: Instant::now(),
        }
    }

    fn expired(&self) -> bool {
        let ttl = if self.ok {
            DRIVER_PROTECTIONS_STATUS_OK_TTL
        } else {
            DRIVER_PROTECTIONS_STATUS_ERR_TTL
        };
        self.shown_at.elapsed() >= ttl
    }
}

/// How a `DriverProtectionsResult` lines up with the outstanding request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriverProtectionsAttribution {
    /// Echoed request id matches the outstanding request.
    Current,
    /// Result carries no request id; only the echoed direction could be checked.
    Uncorrelated,
    /// Echoed request id belongs to a different request than the outstanding one.
    Stale,
    /// Nothing outstanding on this page.
    Unsolicited,
}

impl DriverProtectionsAttribution {
    pub fn label(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Uncorrelated => "uncorrelated",
            Self::Stale => "stale",
            Self::Unsolicited => "unsolicited",
        }
    }

    /// True when the result answers the request this page is waiting on.
    pub fn answers_outstanding(self) -> bool {
        matches!(self, Self::Current | Self::Uncorrelated)
    }
}

/// Process-unique correlation id for a `SetDriverProtections`.
fn next_driver_protections_request_id(conn: &str) -> String {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("dp-{conn}-{}-{n}", chrono::Utc::now().timestamp_millis())
}

impl Default for HomePage {
    fn default() -> Self {
        Self::new()
    }
}

impl HomePage {
    pub fn new() -> Self {
        Self {
            sub_tab: HomeSubTab::Overview,
            db_runs: Arc::new(Mutex::new(Vec::new())),
            log_runs: HashMap::new(),
            log_cursor: 0,
            last_poll: None,
            inventory: Arc::new(Mutex::new(Vec::new())),
            last_inventory_poll: None,
            last_phantom_ensure: None,
            phantom_pending: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            driver_protections_pending: None,
            driver_protections_status: None,
            driver_protections_busy: false,
            driver_protections_sent_at: None,
            driver_protections_outstanding: None,
        }
    }

    /// Re-export so the WebSocketClient host can reset our log parser
    /// when it clears the script-log buffer.
    pub fn reset_log_cursor(&mut self) {
        self.log_cursor = 0;
        self.log_runs.clear();
    }

    /// Ends any wait and shows `terse` on the toolbar with `detail` on hover.
    fn set_driver_protections_result(&mut self, success: bool, terse: String, detail: String) {
        self.driver_protections_busy = false;
        self.driver_protections_sent_at = None;
        self.driver_protections_pending = None;
        self.driver_protections_outstanding = None;
        self.driver_protections_status =
            Some(DriverProtectionsStatus::new(success, terse, detail));
    }

    /// Marks a `SetDriverProtections` as in flight under `request_id` and starts
    /// its timeout.
    fn begin_driver_protections_wait(&mut self, enable: bool, request_id: String) {
        self.driver_protections_busy = true;
        self.driver_protections_sent_at = Some(Instant::now());
        self.driver_protections_outstanding = Some(DriverProtectionsRequest {
            id: request_id,
            enable,
        });
        self.driver_protections_status = None;
    }

    /// Direction of the `SetDriverProtections` still awaiting a reply.
    pub fn outstanding_driver_protections_direction(&self) -> Option<bool> {
        self.driver_protections_outstanding.as_ref().map(|r| r.enable)
    }

    /// Matches a result's echoed correlation id and direction against the
    /// outstanding request.
    pub fn classify_driver_protections_result(
        &self,
        request_id: Option<&str>,
        requested_enable: Option<bool>,
    ) -> DriverProtectionsAttribution {
        let Some(outstanding) = self.driver_protections_outstanding.as_ref() else {
            return DriverProtectionsAttribution::Unsolicited;
        };
        match request_id {
            Some(id) if id == outstanding.id => DriverProtectionsAttribution::Current,
            Some(_) => DriverProtectionsAttribution::Stale,
            None => match requested_enable {
                Some(enable) if enable != outstanding.enable => {
                    DriverProtectionsAttribution::Stale
                }
                _ => DriverProtectionsAttribution::Uncorrelated,
            },
        }
    }

    /// Ends the wait only for a result that answers the outstanding request;
    /// anything else is reported without clearing the in-flight state.
    pub fn apply_driver_protections_result(
        &mut self,
        attribution: DriverProtectionsAttribution,
        success: bool,
        terse: String,
        detail: String,
    ) {
        if attribution.answers_outstanding() {
            self.set_driver_protections_result(success, terse, detail);
            return;
        }
        log::error!(
            "[home] driver protections: {} DriverProtectionsResult not attributed to a live \
             request — {detail}",
            attribution.label(),
        );
        self.driver_protections_status = Some(DriverProtectionsStatus::new(
            false,
            format!("{} reply — not this request: {terse}", attribution.label()),
            format!(
                "{} client reply — NOT attributed to a request from this page, so it does not \
                 describe the change you are waiting on: {detail}",
                attribution.label(),
            ),
        ));
    }

    /// Clears a wait that outlived `DRIVER_PROTECTIONS_TIMEOUT` so both the
    /// disable and the revert control become usable again.
    fn expire_driver_protections_wait(&mut self) {
        let Some(sent) = self.driver_protections_sent_at else { return };
        if sent.elapsed() < DRIVER_PROTECTIONS_TIMEOUT {
            return;
        }
        self.driver_protections_sent_at = None;
        self.driver_protections_busy = false;
        // Dropping the correlation id stops a late reply being read as the next request's.
        let expired = self.driver_protections_outstanding.take();
        let secs = DRIVER_PROTECTIONS_TIMEOUT.as_secs();
        log::warn!(
            "[home] driver protections: no DriverProtectionsResult within {secs}s for request {} \
             — clearing busy",
            expired.as_ref().map(|r| r.id.as_str()).unwrap_or("<none>"),
        );
        self.driver_protections_status = Some(DriverProtectionsStatus::new(
            false,
            format!("No reply in {secs}s — state unknown"),
            format!(
                "No response from client within {secs}s — the machine may be in EITHER state. \
                 Re-check it and re-send before it leaves the bench."
            ),
        ));
    }

    /// Drops a shown outcome once its TTL elapses.
    fn expire_driver_protections_status(&mut self) {
        if self
            .driver_protections_status
            .as_ref()
            .is_some_and(|s| s.expired())
        {
            self.driver_protections_status = None;
        }
    }
}

/// Terse driver-protections outcome for the toolbar status line and the toast.
/// Hostname (the panel is per-client) and request id stay in the durable detail.
pub fn terse_driver_protections_status(
    success: bool,
    hvci_enabled: Option<bool>,
    blocklist_enabled: Option<bool>,
    hvci_running: Option<bool>,
    policy_override: Option<&str>,
    reboot_required: bool,
    message: &str,
) -> String {
    if policy_override.is_some() {
        return if hvci_running == Some(true) {
            "POLICY OVERRIDE — HVCI still running".to_string()
        } else {
            "POLICY OVERRIDE — change will not apply".to_string()
        };
    }
    if !success {
        return format!("Failed: {}", short_reason(message));
    }
    let on_off = |v: bool| if v { "on" } else { "off" };
    let reboot = if reboot_required {
        " — reboot required"
    } else {
        ""
    };
    match (hvci_enabled, blocklist_enabled) {
        (Some(true), Some(true)) | (Some(false), Some(false)) => {
            let state = on_off(hvci_enabled == Some(true));
            if reboot_required {
                format!("Protections {state} — reboot required")
            } else {
                format!("Protections already {state}")
            }
        }
        (Some(h), Some(b)) => format!(
            "MIXED: HVCI {}, blocklist {}{reboot}",
            on_off(h),
            on_off(b)
        ),
        (Some(h), None) => {
            format!("PARTIAL: HVCI {}, blocklist unreadable{reboot}", on_off(h))
        }
        (None, Some(b)) => {
            format!("PARTIAL: HVCI unreadable, blocklist {}{reboot}", on_off(b))
        }
        (None, None) => "Protections state unreadable".to_string(),
    }
}

/// First clause of a client message, capped at 64 chars for a one-line status.
fn short_reason(message: &str) -> String {
    let first = message.split(';').next().unwrap_or(message).trim();
    if first.is_empty() {
        return "no reason reported".to_string();
    }
    if first.chars().count() <= 64 {
        return first.to_string();
    }
    let head: String = first.chars().take(64).collect();
    format!("{head}…")
}

impl WebSocketClient {
    /// Home page entry — called from the main `match self.state` in
    /// `ui.rs`. Replaces the old `show_live_stats`.
    pub fn show_home(&mut self, ui: &mut Ui) {
        // Forward any pending process-table actions same as the old
        // `show_live_stats` did so right-click → Kill / Open in
        // Explorer keeps working.
        while let Some(action) = self.resource_monitor.process_table_viewer.try_recv_action() {
            match action {
                crate::tabs::resource_monitor::process_table::ProcessAction::Kill(pid) => {
                    let _ = self.send_cmd_tx.try_send(crate::Cmd::KillProcess(pid));
                }
                crate::tabs::resource_monitor::process_table::ProcessAction::OpenInExplorer(path) => {
                    let _ = self.send_cmd_tx.try_send(crate::Cmd::OpenProcessInExplorer(path));
                }
            }
        }

        // Parse any new log lines from the scripts viewer so the
        // log-stream half of active-run detection stays current.
        self.home_page.ingest_script_log(&self.remote_scripts_viewer.log_messages);

        // Maybe kick off a DB poll for in-progress runs on this client.
        let computer_id = self.client.computer.clone();
        self.home_page.maybe_poll_active_runs(computer_id.as_ref());
        // Inventory poll runs on a slower cadence (components rarely
        // change between frames; one run finishing only flips a status
        // glyph). Both polls share the same computer id.
        self.home_page.maybe_poll_inventory(computer_id.as_ref());
        // Once a minute, upsert hardware_component rows for whatever
        // CPU / GPUs the live sysinfo says are installed. Makes never-
        // stress-tested hardware show up as phantom cards in the
        // inventory grid right away.
        self.home_page
            .maybe_ensure_phantom_components(self.resource_monitor.latest_sysinfo.as_ref());

        let computer_label = computer_id
            .as_ref()
            .map(|c| c.key_string())
            .or_else(|| self.client.friendly_name.clone())
            .unwrap_or_else(|| self.client.connection_string.clone());

        // ── Top: status header + hardware inventory + active runs ──
        render_status_header(ui, &computer_label, &self.client.connection_string);
        let actions = self.home_page.render_inventory(ui);
        ui.separator();

        // Process click-throughs collected during the inventory render.
        // Click on a past-run row / id pill → switch to McpToolLog and
        // open the run in the breadcrumb.
        for run_id in actions.open_records {
            self.mcp_tool_log_viewer.open_record(run_id);
            let _ = self
                .display_state_channel
                .0
                .try_send(WsDisplayState::McpToolLog);
        }
        // Click "Re-run" on a failed past row → jump to the Scripts tab
        // so the operator can re-launch the matching script. Auto-launch
        // would need the script-name ↔ tool_label mapping which lives
        // in the stress catalog; manual launch keeps this v1 honest.
        if let Some(label) = actions.reruns.last() {
            log::info!(
                "[home] re-run requested for '{label}'; switching to Scripts tab"
            );
            let _ = self
                .display_state_channel
                .0
                .try_send(WsDisplayState::Scripts);
            if !(self.remote_scripts_viewer.loading || self.remote_scripts_viewer.running) {
                self.remote_scripts_viewer.loading = true;
                let _ = self.send_cmd_tx.try_send(Cmd::GetRemoteScriptList);
            }
        }

        // ── Sub-tab nav: Overview | Processes ──
        ui.horizontal(|ui| {
            let overview_active = self.home_page.sub_tab == HomeSubTab::Overview;
            let processes_active = self.home_page.sub_tab == HomeSubTab::Processes;
            if ui
                .selectable_label(overview_active, format!("{} Overview", icons::CHART))
                .clicked()
            {
                self.home_page.sub_tab = HomeSubTab::Overview;
            }
            if ui
                .selectable_label(processes_active, format!("{} Processes", icons::LIST))
                .clicked()
            {
                self.home_page.sub_tab = HomeSubTab::Processes;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if let Some(snap) = self.resource_monitor.latest_sysinfo.as_ref() {
                    ui.label(
                        RichText::new(format!(
                            "hostname: {}",
                            if snap.hostname.is_empty() {
                                "?"
                            } else {
                                &snap.hostname
                            }
                        ))
                        .small()
                        .weak(),
                    );
                }
            });
        });
        ui.separator();

        // ── Body ──
        match self.home_page.sub_tab {
            HomeSubTab::Overview => {
                ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .id_salt(format!("home-overview-{}", self.client.connection_string))
                    .show(ui, |ui| {
                        self.resource_monitor.show_compact_overview(ui);
                    });
            }
            HomeSubTab::Processes => {
                // Make sure the process table sees fresh sysinfo even
                // though we aren't going through resource_monitor.display.
                self.resource_monitor.pump_telemetry();
                ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .id_salt(format!("home-processes-{}", self.client.connection_string))
                    .show(ui, |ui| {
                        self.resource_monitor.process_table_viewer.show(ui);
                    });
            }
        }
    }

    /// Root-gated driver-protections menu button plus its terse status, for the
    /// client toolbar row. Sends on the frame the operator confirms.
    pub fn driver_protections_toolbar(&mut self, ui: &mut Ui) {
        let conn = self.client.connection_string.clone();
        if let Some(enable) = self.home_page.render_driver_protections_menu(ui, &conn) {
            self.send_driver_protections(enable);
        }
    }

    /// Queues a `SetDriverProtections`, audits the attempt, and arms the wait.
    fn send_driver_protections(&mut self, enable: bool) {
        let conn = self.client.connection_string.clone();
        // Enforced here, not only by hiding the UI, so no stray action can
        // lower a customer machine's security posture.
        if !crate::tabs::admin_console::current_user_is_root() {
            log::warn!("[home] SetDriverProtections refused for {conn}: signed-in user is not Root");
            self.home_page.set_driver_protections_result(
                false,
                "Root authorization required".to_string(),
                "Root authorization required to change driver protections".to_string(),
            );
            return;
        }
        let verb = if enable { "re-enable" } else { "disable" };
        let request_id = next_driver_protections_request_id(&conn);
        // Enqueued before anything is audited so no 'requested' record
        // outlives a command that never left the console.
        let queued = self
            .send_cmd_tx
            .try_send(Cmd::SetDriverProtections {
                enable,
                request_id: Some(request_id.clone()),
            })
            .is_ok();
        let audit = if queued {
            format!(
                "Driver protections: operator requested {verb} of Memory Integrity (HVCI) and the \
                 Vulnerable Driver Blocklist on {conn} (request {request_id}) — awaiting the \
                 client's read-back"
            )
        } else {
            format!(
                "Driver protections: {verb} of Memory Integrity (HVCI) and the Vulnerable Driver \
                 Blocklist on {conn} (request {request_id}) was NOT sent — this session's command \
                 channel is closed and the machine is unchanged"
            )
        };
        if queued {
            log::warn!("[home] {audit}");
        } else {
            log::error!("[home] {audit}");
        }
        self.history.push(History {
            from: "System".to_string(),
            message: audit.clone(),
            timestamp: chrono::Local::now().to_rfc3339(),
        });
        record_driver_protections_audit(DriverProtectionsAudit {
            connection_string: conn,
            hostname: self.client_hostname(),
            computer: self.client.computer.clone(),
            enable,
            stage: if queued {
                DriverProtectionsStage::Requested
            } else {
                DriverProtectionsStage::Failed
            },
            detail: audit.clone(),
            request_id: Some(request_id.clone()),
            ..Default::default()
        });
        if queued {
            self.home_page
                .begin_driver_protections_wait(enable, request_id);
        } else {
            self.home_page.set_driver_protections_result(
                false,
                "Not sent — command channel closed".to_string(),
                audit,
            );
        }
    }

    /// Live sysinfo hostname, else the client's friendly name.
    pub fn client_hostname(&self) -> Option<String> {
        self.resource_monitor
            .latest_sysinfo
            .as_ref()
            .map(|s| s.hostname.trim().to_string())
            .filter(|h| !h.is_empty())
            .or_else(|| self.client.friendly_name.clone())
    }
}

fn render_status_header(ui: &mut Ui, computer_label: &str, connection_string: &str) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!("{} Home", icons::HOME))
                .color(theme::accent(ui))
                .heading(),
        );
        ui.add_space(12.0);
        ui.label(
            RichText::new(computer_label)
                .strong()
                .color(theme::strong_text(ui)),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(format!("conn: {connection_string}"))
                    .small()
                    .color(theme::weak_text(ui))
                    .monospace(),
            );
        });
    });
}

/// How far a driver-protections change got.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DriverProtectionsStage {
    #[default]
    Requested,
    Applied,
    Failed,
}

impl DriverProtectionsStage {
    fn label(self) -> &'static str {
        match self {
            Self::Requested => "requested",
            Self::Applied => "applied",
            Self::Failed => "failed",
        }
    }
}

/// One durable driver-protections audit record: direction, target, operator, time.
#[derive(Clone, Debug, Default)]
pub struct DriverProtectionsAudit {
    pub connection_string: String,
    pub hostname: Option<String>,
    pub computer: Option<RecordId>,
    /// True for a re-enable, false for a disable.
    pub enable: bool,
    pub stage: DriverProtectionsStage,
    pub detail: String,
    pub hvci_enabled: Option<bool>,
    pub blocklist_enabled: Option<bool>,
    pub hvci_running: Option<bool>,
    pub policy_override: Option<String>,
    /// HVCI in `Win32_DeviceGuard.SecurityServicesConfigured`.
    pub hvci_configured: Option<bool>,
    /// `Win32_DeviceGuard.VirtualizationBasedSecurityStatus`: 0 off, 1 configured, 2 running.
    pub vbs_status: Option<u32>,
    pub reboot_required: Option<bool>,
    /// Correlation id of the `SetDriverProtections` this record belongs to.
    pub request_id: Option<String>,
    /// How a client result lined up with the outstanding request; `None` on
    /// records written by the console itself.
    pub attribution: Option<DriverProtectionsAttribution>,
    /// True when `direction` came from the client's echoed `requested_enable`
    /// rather than from console state.
    pub direction_echoed: bool,
}

/// Persists `audit` in the background.
pub fn record_driver_protections_audit(audit: DriverProtectionsAudit) {
    PlatformSpawner::spawn(async move { write_driver_protections_audit(audit).await });
}

/// Writes the audit to the client's open diagnostic session and to its
/// `computer` row. Warns loudly when neither target exists so a change is
/// never silently unrecorded.
async fn write_driver_protections_audit(audit: DriverProtectionsAudit) {
    use database::schema::{
        DiagnosticCategory, DiagnosticEntry, DiagnosticSession, DIAGNOSTIC_SESSION_TABLE,
    };

    let direction = if audit.enable { "re-enabled" } else { "disabled" };
    let stage = audit.stage.label();
    let operator = crate::get_current_user_from_auth()
        .map(|u| format!("{} <{}>", u.get_name(), u.get_email()))
        .unwrap_or_else(|| "unknown operator".to_string());
    let at = chrono::Utc::now().to_rfc3339();
    let host = audit.hostname.clone().unwrap_or_default();

    let record = serde_json::json!({
        "direction": direction,
        "stage": stage,
        "connection_string": audit.connection_string,
        "hostname": host,
        "operator": operator,
        "at": at,
        "request_id": audit.request_id,
        "attribution": audit.attribution.map(|a| a.label()),
        "direction_echoed": audit.direction_echoed,
        "hvci_enabled": audit.hvci_enabled,
        "blocklist_enabled": audit.blocklist_enabled,
        "hvci_running": audit.hvci_running,
        "hvci_configured": audit.hvci_configured,
        "vbs_status": audit.vbs_status,
        "reboot_required": audit.reboot_required,
        "policy_override": audit.policy_override,
        "detail": audit.detail,
    });

    let mut written: Vec<String> = Vec::new();

    let session_key = DiagnosticSession::latest_open_for_connection(
        &audit.connection_string,
        audit.computer.as_ref(),
    )
    .await
    .ok()
    .flatten()
    .map(|s| s.id.key_string());

    if let Some(key) = session_key.as_deref() {
        let entry = DiagnosticEntry {
            session_ref: RecordId::new(DIAGNOSTIC_SESSION_TABLE, key),
            // A disable lowers the machine's security posture; the revert is a plain action.
            category: if audit.enable {
                DiagnosticCategory::Action
            } else {
                DiagnosticCategory::SecurityAlert
            },
            title: format!(
                "Memory Integrity + vulnerable-driver blocklist {direction} ({stage}){}",
                match audit.attribution {
                    Some(a) if !a.answers_outstanding() =>
                        format!(" [{} client reply — unattributed]", a.label()),
                    _ => String::new(),
                },
            ),
            detail: format!(
                "{}\nTarget: {} ({})\nOperator: {operator}\nAt: {at}",
                audit.detail,
                audit.connection_string,
                if host.is_empty() { "hostname unknown" } else { host.as_str() },
            ),
            data: Some(record.clone()),
            ..Default::default()
        };
        match DiagnosticEntry::create(&entry).await {
            Ok(id) => written.push(format!("diagnostic_entry {}", id.key_string())),
            Err(e) => log::error!("[home] driver-protections diagnostic entry failed: {e}"),
        }
    }

    if let Some(id) = audit.computer.clone() {
        // Appends to the row's history instead of replacing it, so a
        // disable/reboot/re-enable pass keeps every state it went through.
        // `array::flatten` folds a legacy single-object value into the first
        // element and an absent field into an empty history.
        let res: Result<_, surrealdb::Error> = db()
            .query(
                "UPDATE $id SET driver_protections = array::slice(\
                     array::flatten([driver_protections ?? [], [$record]]), $tail\
                 ) RETURN id",
            )
            .bind(("id", id.clone()))
            .bind(("record", record))
            // Negative start keeps the newest entries.
            .bind(("tail", -DRIVER_PROTECTIONS_HISTORY_KEEP))
            .await;
        match res {
            Ok(mut resp) => {
                let rows: Result<Vec<serde_json::Value>, surrealdb::Error> = resp.take(0);
                let matched = match rows {
                    Ok(rows) => !rows.is_empty(),
                    Err(e) => {
                        log::warn!("[home] driver-protections history read-back failed: {e}");
                        true
                    }
                };
                if matched {
                    written.push(format!("computer {} history", id.key_string()));
                } else {
                    log::error!(
                        "[home] driver-protections history matched no computer row {}",
                        id.key_string(),
                    );
                }
            }
            Err(e) => log::error!("[home] driver-protections computer history append failed: {e}"),
        }
    }

    if written.is_empty() {
        log::error!(
            "[home] driver protections {direction} ({stage}) on {} NOT recorded — no open \
             diagnostic session and no linked computer",
            audit.connection_string,
        );
        let _ = crate::get_toast_sender().try_send(crate::ToastMessage::Warning(format!(
            "Driver protections {direction} on {} was NOT recorded — link a computer or open a \
             diagnostic session for this client",
            audit.connection_string,
        )));
        return;
    }
    log::warn!(
        "[home] driver protections {direction} ({stage}) on {} by {operator} recorded to {}",
        audit.connection_string,
        written.join(", "),
    );
}

impl HomePage {
    fn ingest_script_log(&mut self, lines: &[String]) {
        if lines.len() < self.log_cursor {
            // Log buffer was reset (e.g. cleared by the operator).
            // Re-parse from the start — the existing entries in
            // `log_runs` are no longer trustworthy.
            self.log_runs.clear();
            self.log_cursor = 0;
        }
        for line in &lines[self.log_cursor..] {
            self.parse_log_line(line);
        }
        self.log_cursor = lines.len();
        // Reap log-stream cards whose planned budget (plus grace) expired without a completion marker.
        self.log_runs.retain(|_, run| {
            let allowed = run.duration_planned_secs.unwrap_or(600) + 300;
            run.started_at.elapsed().as_secs() <= allowed
        });
    }

    /// Status lines we recognise from the stress-runner remote scripts:
    ///   `Starting: <name>`                    → new active run
    ///   `stress_test_run id: <uuid>`          → attach run id to most-recent start
    ///   `<name> PASSED in <secs>s …`          → drop the run
    ///   `<name> FAILED in <secs>s …`          → drop the run
    fn parse_log_line(&mut self, line: &str) {
        let trimmed = line.trim();
        if let Some(name) = trimmed.strip_prefix("Starting: ") {
            self.log_runs.insert(
                name.to_string(),
                ActiveRun {
                    run_id: String::new(),
                    tool_label: name.to_string(),
                    started_at: Instant::now(),
                    duration_planned_secs: Some(
                        crate::scripts::default_remote_script_timeout_secs(name),
                    ),
                    source: ActiveRunSource::LogStream,
                    // Log stream never knows which hardware_component a
                    // run targets — the DB poll fills that in once it
                    // catches up.
                    target_component: None,
                    current_stage_idx: None,
                    current_stage_total: None,
                    current_stage_name: None,
                    touched_components: Vec::new(),
                },
            );
            return;
        }
        if let Some(rest) = trimmed.strip_prefix("stress_test_run id: ") {
            // Attach to the most recently-started run that's missing
            // an id. Simple but matches the script log's strict order.
            if let Some(entry) = self
                .log_runs
                .values_mut()
                .filter(|e| e.run_id.is_empty())
                .max_by_key(|e| e.started_at)
            {
                entry.run_id = rest.trim().to_string();
            }
            return;
        }
        // Multi-stage scenario heartbeat: `Stage <N>/<M>: <name>` lands
        // for every stage boundary. Attach to the most-recent active
        // run so the UI can show e.g. "stage 3/4 · gpu_vram".
        if let Some(rest) = trimmed.strip_prefix("Stage ") {
            if let Some((counts, name)) = rest.split_once(": ") {
                if let Some((idx_str, total_str)) = counts.split_once('/') {
                    if let (Ok(idx), Ok(total)) =
                        (idx_str.trim().parse::<u32>(), total_str.trim().parse::<u32>())
                    {
                        if let Some(entry) = self
                            .log_runs
                            .values_mut()
                            .max_by_key(|e| e.started_at)
                        {
                            entry.current_stage_idx = Some(idx);
                            entry.current_stage_total = Some(total);
                            entry.current_stage_name = Some(name.trim().to_string());
                        }
                        return;
                    }
                }
            }
        }
        // Termination — look for `NAME PASSED in …` or `NAME FAILED in …`.
        // The stress-runner emits both shapes; we drop the entry either way.
        for marker in [" PASSED in ", " FAILED in "] {
            if let Some(name_end) = trimmed.find(marker) {
                let name = trimmed[..name_end].trim().to_string();
                self.log_runs.remove(&name);
                return;
            }
        }
    }

    fn maybe_poll_active_runs(&mut self, computer: Option<&RecordId>) {
        let Some(computer) = computer else { return };
        let should_poll = match self.last_poll {
            None => true,
            Some(t) => t.elapsed() >= ACTIVE_RUN_POLL_INTERVAL,
        };
        if !should_poll {
            return;
        }
        self.last_poll = Some(Instant::now());
        let sink = self.db_runs.clone();
        let cid = computer.clone();
        PlatformSpawner::spawn(async move {
            let rows = fetch_active_runs(&cid).await.unwrap_or_default();
            if let Ok(mut g) = sink.lock() {
                *g = rows;
            }
        });
    }

    fn maybe_ensure_phantom_components(&mut self, sysinfo: Option<&SystemInformation>) {
        // If the previous batch finished, force the next inventory
        // poll so the freshly-upserted phantom rows appear right away
        // rather than waiting out the 5 s cadence.
        if self
            .phantom_pending
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            self.last_inventory_poll = None;
        }

        let Some(snap) = sysinfo else { return };
        let due = match self.last_phantom_ensure {
            None => true,
            Some(t) => t.elapsed() >= PHANTOM_ENSURE_INTERVAL,
        };
        if !due {
            return;
        }
        self.last_phantom_ensure = Some(Instant::now());

        // Collect every CPU/GPU we can name. CPU brand strings tend
        // to be wordy ("AMD Ryzen 5 5600G with Radeon Graphics") so we
        // keep them as-is — `canonical_id` hashes after lowercase +
        // trim, which collapses cross-machine casing diffs.
        let mut wanted: Vec<(HardwareKind, String, String, Option<serde_json::Value>)> = Vec::new();
        let cpu = snap.cpu.trim().to_string();
        if !cpu.is_empty() {
            let mut cpu_specs = serde_json::Map::new();
            let phys = snap.number_of_cpus.trim();
            if !phys.is_empty() {
                cpu_specs.insert("physical_cpus".into(), serde_json::json!(phys));
            }
            if !snap.cpu_cores.is_empty() {
                cpu_specs.insert("logical_cores".into(), serde_json::json!(snap.cpu_cores.len()));
            }
            let specs = (!cpu_specs.is_empty()).then(|| serde_json::Value::Object(cpu_specs));
            wanted.push((HardwareKind::Cpu, classify_cpu_vendor(&cpu), cpu, specs));
        }
        for card in &snap.gpu_info.card {
            let name = card.name.trim();
            if name.is_empty() {
                continue;
            }
            let vendor = if !card.brand.trim().is_empty() {
                classify_gpu_vendor(card.brand.trim())
            } else {
                classify_gpu_vendor(name)
            };
            let mut gpu_specs = serde_json::Map::new();
            if card.memory > 0 {
                gpu_specs.insert("vram_mb".into(), serde_json::json!(card.memory / (1024 * 1024)));
            }
            let drv = card.nvidia_info.driver_version.trim();
            if !drv.is_empty() {
                gpu_specs.insert("driver_version".into(), serde_json::json!(drv));
            }
            let specs = (!gpu_specs.is_empty()).then(|| serde_json::Value::Object(gpu_specs));
            wanted.push((HardwareKind::Gpu, vendor, name.to_string(), specs));
        }

        if wanted.is_empty() {
            return;
        }

        let pending = self.phantom_pending.clone();
        PlatformSpawner::spawn(async move {
            for (kind, vendor, model, specs) in wanted {
                let mut component = HardwareComponent::new(kind, &vendor, &model);
                component.specs = specs;
                match HardwareComponent::upsert_seen(&component).await {
                    Ok(id) => {
                        log::debug!(
                            "[home/phantom] ensured {kind:?} / {vendor} / {model} -> {id:?}"
                        );
                    }
                    Err(e) => log::warn!(
                        "[home/phantom] upsert {kind:?} / {vendor} / {model} failed: {e}"
                    ),
                }
            }
            pending.store(true, std::sync::atomic::Ordering::SeqCst);
        });
    }

    fn maybe_poll_inventory(&mut self, computer: Option<&RecordId>) {
        let Some(computer) = computer else { return };
        let should_poll = match self.last_inventory_poll {
            None => true,
            Some(t) => t.elapsed() >= INVENTORY_POLL_INTERVAL,
        };
        if !should_poll {
            return;
        }
        self.last_inventory_poll = Some(Instant::now());
        let sink = self.inventory.clone();
        let cid = computer.clone();
        PlatformSpawner::spawn(async move {
            let cards = fetch_inventory(&cid).await.unwrap_or_default();
            if let Ok(mut g) = sink.lock() {
                *g = cards;
            }
        });
    }

    fn merged_active_runs(&self) -> Vec<ActiveRun> {
        let db_snapshot: Vec<ActiveRun> = self
            .db_runs
            .lock()
            .ok()
            .map(|g| g.clone())
            .unwrap_or_default();
        let mut merged: Vec<ActiveRun> = Vec::new();
        for run in self.log_runs.values() {
            merged.push(run.clone());
        }
        for run in db_snapshot {
            // When the same run_id shows up in both sources, layer DB
            // metadata (target_component, touched_components, planned
            // duration) onto the log-stream entry rather than discarding
            // it. The log entry wins on `started_at` (more accurate to
            // wall-clock) and on stage info (DB row doesn't carry that).
            if !run.run_id.is_empty() {
                if let Some(existing) = merged.iter_mut().find(|m| m.run_id == run.run_id) {
                    if existing.target_component.is_none() {
                        existing.target_component = run.target_component;
                    }
                    if existing.duration_planned_secs.is_none() {
                        existing.duration_planned_secs = run.duration_planned_secs;
                    }
                    if existing.touched_components.is_empty() {
                        existing.touched_components = run.touched_components;
                    }
                    continue;
                }
            }
            merged.push(run);
        }
        merged
    }

    /// Renders the toolbar Protections menu, its terse status, and the confirm
    /// popup. Returns `Some(enable)` on the frame the operator confirms.
    fn render_driver_protections_menu(&mut self, ui: &mut Ui, conn: &str) -> Option<bool> {
        // Polled panel, so an elapsed-check on repaint bounds the wait.
        self.expire_driver_protections_wait();
        self.expire_driver_protections_status();
        let is_root = crate::tabs::admin_console::current_user_is_root();
        let busy = self.driver_protections_busy;
        if busy {
            ui.ctx().request_repaint_after(Duration::from_millis(500));
        }

        let menu_color = if is_root {
            theme::warn(ui)
        } else {
            theme::weak_text(ui)
        };
        ui.menu_button(
            RichText::new(format!("{} {}", icons::LOCK, icons::menu_label("Protections")))
                .color(menu_color)
                .strong(),
            |ui| {
                if !is_root {
                    ui.label(
                        RichText::new(format!("{} Root authorization required", icons::LOCK))
                            .color(theme::warn(ui))
                            .small(),
                    );
                }
                if ui
                    .add_enabled(
                        !busy && is_root,
                        Button::new(
                            RichText::new(format!(
                                "{} Disable Memory Integrity",
                                icons::STATUS_WARN
                            ))
                            .color(theme::warn(ui)),
                        ),
                    )
                    .on_hover_text(
                        "Turns OFF Memory Integrity (HVCI) and the Vulnerable Driver Blocklist. \
                         Only the legacy WinRing0 sensor backend needs this — a signed backend \
                         reads CPU temperature and board voltages with protections ON, so check \
                         the client's reported sensor backend first.\nLowers this machine's \
                         security posture. Requires a reboot. Revert it before the machine leaves \
                         the bench.",
                    )
                    .clicked()
                {
                    self.driver_protections_pending = Some(false);
                    ui.close();
                }
                if ui
                    .add_enabled(
                        !busy && is_root,
                        Button::new(
                            RichText::new(format!("{} Re-enable protections", icons::CHECK))
                                .color(theme::success(ui)),
                        ),
                    )
                    .on_hover_text(
                        "Turns Memory Integrity (HVCI) and the Vulnerable Driver Blocklist back \
                         ON. Requires a reboot.",
                    )
                    .clicked()
                {
                    self.driver_protections_pending = Some(true);
                    ui.close();
                }
            },
        );

        if busy {
            ui.spinner();
        }

        if let Some(status) = self.driver_protections_status.as_ref() {
            let color = if status.ok {
                theme::success(ui)
            } else {
                theme::error(ui)
            };
            // Width capped to a fraction of the row.
            let cell = Vec2::new(
                (ui.available_width() * DRIVER_PROTECTIONS_STATUS_ROW_SHARE)
                    .clamp(0.0, DRIVER_PROTECTIONS_STATUS_MAX_WIDTH),
                ui.spacing().interact_size.y,
            );
            ui.allocate_ui_with_layout(cell, Layout::left_to_right(Align::Center), |ui| {
                ui.add(
                    Label::new(RichText::new(status.terse.as_str()).color(color).small())
                        .truncate(),
                )
                .on_hover_text(status.detail.as_str());
            });
            // Repaint so the TTL drops the label without waiting for input.
            ui.ctx().request_repaint_after(Duration::from_secs(1));
        }

        if !is_root {
            self.driver_protections_pending = None;
            return None;
        }

        let enable = self.driver_protections_pending?;
        let mut confirmed = None;

        Window::new(format!("{} Confirm driver protections", icons::STATUS_WARN))
            .id(Id::new(("driver_protections_confirm", conn)))
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                ui.set_max_width(460.0);
                if enable {
                    ui.label(
                        RichText::new("Re-enable Windows driver protections on this machine?")
                            .color(theme::strong_text(ui))
                            .strong(),
                    );
                    ui.label(
                        "Sets Memory Integrity (HVCI) and the Vulnerable Driver Blocklist back to \
                         Enabled. Protection is only active again after the machine reboots.",
                    );
                } else {
                    ui.label(
                        RichText::new(format!(
                            "{} This disables Memory Integrity and the Vulnerable Driver Blocklist",
                            icons::STATUS_WARN
                        ))
                        .color(theme::warn(ui))
                        .strong(),
                    );
                    ui.label(
                        "It lowers this machine's security posture so the legacy WinRing0 sensor \
                         backend can load. This is a fallback: only do it when the client's \
                         telemetry reports no signed backend available and you still need CPU \
                         temperature or board voltages.",
                    );
                    ui.label(
                        "The change only takes effect after a reboot. Re-enable protections and \
                         reboot again before the machine goes back to the customer.",
                    );
                }

                let target = u32::from(enable);
                ui.add_space(4.0);
                ui.label(
                    RichText::new(format!(
                        "DeviceGuard\\Scenarios\\HypervisorEnforcedCodeIntegrity\\Enabled = {target}\n\
                         CI\\Config\\VulnerableDriverBlocklistEnable = {target}"
                    ))
                    .monospace()
                    .small()
                    .color(theme::weak_text(ui)),
                );
                ui.add_space(6.0);

                ui.horizontal(|ui| {
                    let (label, color) = if enable {
                        ("Re-enable protections", theme::success(ui))
                    } else {
                        ("Disable protections", theme::error(ui))
                    };
                    if ui.button(RichText::new(label).color(color).strong()).clicked() {
                        confirmed = Some(enable);
                    }
                    if ui.button("Cancel").clicked() {
                        self.driver_protections_pending = None;
                    }
                });
            });

        // The send site owns the busy flag; it only arms the wait once the
        // Root gate has passed.
        if confirmed.is_some() {
            self.driver_protections_pending = None;
        }
        confirmed
    }

    fn render_inventory(&self, ui: &mut Ui) -> HomeActions {
        let mut actions = HomeActions::default();
        let active = self.merged_active_runs();
        let cards: Vec<InventoryCard> = self
            .inventory
            .lock()
            .ok()
            .map(|g| g.clone())
            .unwrap_or_default();

        ui.add_space(6.0);

        // Top summary line: total components + total active runs.
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Hardware inventory")
                    .color(theme::accent(ui))
                    .strong(),
            );
            ui.label(
                RichText::new(format!("({} component(s))", cards.len()))
                    .color(theme::weak_text(ui))
                    .small(),
            );
            ui.separator();
            if active.is_empty() {
                ui.colored_label(theme::weak_text(ui), icons::STATUS_IDLE);
                ui.colored_label(theme::weak_text(ui), "no stress runs in progress");
            } else {
                ui.colored_label(theme::warn(ui), icons::STATUS_WAIT);
                ui.colored_label(
                    theme::warn(ui),
                    format!("{} stress run(s) in progress", active.len()),
                );
            }
        });

        if cards.is_empty() {
            ui.add_space(4.0);
            ui.colored_label(
                theme::weak_text(ui),
                "No hardware_component history yet for this machine. Run any stress test to populate.",
            );
        } else {
            let card_cols = if ui.available_width() < 760.0 { 1 } else { 2 };
            ui.columns(card_cols, |cols| {
                for (idx, card) in cards.iter().enumerate() {
                    let col = &mut cols[idx % card_cols];
                    // A run "belongs" to this card if its target OR any
                    // of its touched_components matches. Same rubric the
                    // inventory bucketing uses, so the inline progress
                    // bar shows under every card the run is exercising
                    // (e.g. gpu_pcie touches both CPU + GPU).
                    let linked: Vec<&ActiveRun> = active
                        .iter()
                        .filter(|r| run_touches_component(r, &card.component_id))
                        .collect();
                    render_component_card(col, card, &linked, &mut actions);
                }
            });
        }

        // Unlinked = no card on this page would render this run. With
        // the touched-components matching, this is rare — only fires
        // for log-stream entries (no DB metadata yet) and for runs whose
        // component links don't intersect the cards we have cached.
        let unlinked: Vec<&ActiveRun> = active
            .iter()
            .filter(|r| !cards.iter().any(|c| run_touches_component(r, &c.component_id)))
            .collect();
        if !unlinked.is_empty() {
            ui.add_space(6.0);
            ui.label(
                RichText::new("Active runs not yet linked to a component")
                    .color(theme::weak_text(ui))
                    .small(),
            );
            for run in unlinked {
                render_active_run_card(ui, run, &mut actions);
            }
        }
        actions
    }
}

fn render_component_card(
    ui: &mut Ui,
    card: &InventoryCard,
    active: &[&ActiveRun],
    actions: &mut HomeActions,
) {
    let kind_glyph = match card.kind.as_str() {
        "cpu" => icons::CHART,           // CPU/compute → bar chart icon
        "gpu" => icons::MONITOR,         // GPU → monitor icon
        "ram_module" | "ram_kit" => icons::PACKAGE,
        "ssd" | "hdd" => icons::HARD_DRIVE,
        "motherboard" => icons::GRID,
        "psu" => icons::POWER,
        "cooler" => icons::WRENCH,
        _ => icons::PACKAGE,
    };
    ui.group(|ui| {
        ui.horizontal(|ui| {
            ui.colored_label(theme::accent(ui), kind_glyph);
            ui.label(
                RichText::new(&card.display_name)
                    .strong()
                    .color(theme::strong_text(ui)),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("{} runs", card.recent_runs.len()))
                        .small()
                        .color(theme::weak_text(ui)),
                );
                ui.label(
                    RichText::new(card.kind.to_uppercase())
                        .small()
                        .monospace()
                        .color(theme::weak_text(ui)),
                );
            });
        });

        // Inline progress for any active run targeting this component.
        for run in active {
            render_inline_active_run(ui, run);
        }

        // History strip: latest N runs as compact rows. Each row is
        // clickable (opens the run in the MCP Tool Log breadcrumb).
        // Failed rows get an extra "Re-run" button for quick retry.
        if card.recent_runs.is_empty() {
            ui.colored_label(theme::weak_text(ui), "no run history yet");
        } else {
            for past in &card.recent_runs {
                render_past_run_row(ui, past, actions);
            }
        }
        ui.allocate_space(Vec2::new(ui.available_width(), 0.0));
    });
}

fn render_inline_active_run(ui: &mut Ui, run: &ActiveRun) {
    let elapsed = run.started_at.elapsed().as_secs_f64();
    let planned = run.duration_planned_secs.map(|s| s as f64);
    let (progress, remaining_label) = match planned {
        Some(p) if p > 0.0 => {
            let frac = (elapsed / p).clamp(0.0, 1.0);
            let remaining = (p - elapsed).max(0.0);
            (Some(frac as f32), format!("{} remaining", fmt_secs(remaining)))
        }
        _ => (None, "in progress".to_string()),
    };
    ui.horizontal(|ui| {
        ui.colored_label(theme::warn(ui), icons::STATUS_WAIT);
        ui.label(
            RichText::new(&run.tool_label)
                .strong()
                .color(theme::warn(ui)),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(format!("{} elapsed", fmt_secs(elapsed)))
                    .small()
                    .color(theme::weak_text(ui)),
            );
            ui.label(
                RichText::new(remaining_label)
                    .small()
                    .strong()
                    .color(theme::warn(ui)),
            );
        });
    });
    // Stage breakdown for multi-stage scenarios. Only renders when the
    // log stream supplied stage info (DB-only entries leave this None).
    if let (Some(idx), Some(total)) = (run.current_stage_idx, run.current_stage_total) {
        let name = run
            .current_stage_name
            .as_deref()
            .unwrap_or("");
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("stage {idx}/{total}"))
                    .small()
                    .monospace()
                    .color(theme::weak_text(ui)),
            );
            if !name.is_empty() {
                ui.label(icons::CARET_RIGHT);
                ui.label(
                    RichText::new(name)
                        .small()
                        .monospace()
                        .color(theme::info(ui)),
                );
            }
        });
    }
    if let Some(frac) = progress {
        ui.add(
            ProgressBar::new(frac)
                .desired_width(ui.available_width().max(120.0))
                .desired_height(6.0),
        );
    } else {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.colored_label(theme::weak_text(ui), "no planned timeout");
        });
    }
}

fn render_past_run_row(ui: &mut Ui, run: &PastRun, actions: &mut HomeActions) {
    let (icon, color) = match run.result.as_str() {
        "pass" => (icons::STATUS_ON, theme::success(ui)),
        "fail" => (icons::STATUS_ERR, theme::error(ui)),
        "aborted" => (icons::STATUS_OFF, theme::warn(ui)),
        "in_progress" => (icons::STATUS_WAIT, theme::warn(ui)),
        _ => (icons::STATUS_IDLE, theme::weak_text(ui)),
    };
    let failed = run.result == "fail" || run.result == "aborted";
    ui.horizontal(|ui| {
        ui.colored_label(color, icon);
        // Whole-row selectable label opens this run in the MCP Tool
        // Log breadcrumb (same pattern as RecordID chips there).
        let row = ui.selectable_label(
            false,
            RichText::new(&run.tool_label)
                .small()
                .monospace()
                .color(theme::strong_text(ui)),
        );
        if row
            .on_hover_text(format!("Open {} in the MCP Tool Log breadcrumb", run.run_id))
            .clicked()
            && !run.run_id.is_empty()
        {
            actions.open_records.push(run.run_id.clone());
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            // Retry only makes sense on a completed failed/aborted run.
            if failed
                && ui
                    .small_button(format!("{} Re-run", icons::REFRESH))
                    .on_hover_text(format!("Open the Scripts tab to re-run '{}'", run.tool_label))
                    .clicked()
            {
                actions.reruns.push(run.tool_label.clone());
            }
            ui.label(
                RichText::new(&run.started_at_label)
                    .small()
                    .color(theme::weak_text(ui)),
            );
        });
    });
}

fn render_active_run_card(ui: &mut Ui, run: &ActiveRun, actions: &mut HomeActions) {
    let elapsed = run.started_at.elapsed().as_secs_f64();
    let planned = run.duration_planned_secs.map(|s| s as f64);
    let (progress, remaining_label, accent) = match planned {
        Some(p) if p > 0.0 => {
            let frac = (elapsed / p).clamp(0.0, 1.0);
            let remaining = (p - elapsed).max(0.0);
            (
                Some(frac as f32),
                format!("{} remaining", fmt_secs(remaining)),
                theme::warn(ui),
            )
        }
        _ => (None, "unbounded".to_string(), theme::info(ui)),
    };

    ui.group(|ui| {
        ui.horizontal(|ui| {
            ui.colored_label(accent, icons::STATUS_WAIT);
            ui.label(
                RichText::new(&run.tool_label)
                    .strong()
                    .color(theme::strong_text(ui)),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("source: {}", run.source.label()))
                        .small()
                        .color(theme::weak_text(ui)),
                );
                if !run.run_id.is_empty() {
                    let pill = ui
                        .selectable_label(
                            false,
                            RichText::new(format!("id={}", short(&run.run_id, 16)))
                                .small()
                                .monospace()
                                .color(theme::weak_text(ui)),
                        );
                    if pill
                        .on_hover_text(format!("Open {} in the MCP Tool Log breadcrumb", run.run_id))
                        .clicked()
                    {
                        actions.open_records.push(run.run_id.clone());
                    }
                }
            });
        });
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{} elapsed", fmt_secs(elapsed)))
                    .color(theme::weak_text(ui))
                    .small(),
            );
            ui.separator();
            ui.label(
                RichText::new(remaining_label)
                    .color(accent)
                    .small()
                    .strong(),
            );
        });
        if let Some(frac) = progress {
            ui.add(
                ProgressBar::new(frac)
                    .desired_width(ui.available_width().max(120.0))
                    .desired_height(8.0),
            );
        } else {
            // Unbounded run → indeterminate-style bar via spinner + label.
            ui.horizontal(|ui| {
                ui.spinner();
                ui.colored_label(theme::weak_text(ui), "in progress (no planned timeout)");
            });
        }
        // Stop the layout from auto-shrinking when many runs stack.
        ui.allocate_space(Vec2::new(ui.available_width(), 0.0));
    });
}

fn fmt_secs(secs: f64) -> String {
    let secs = secs.max(0.0);
    if secs < 60.0 {
        format!("{secs:.0}s")
    } else if secs < 3600.0 {
        let m = (secs / 60.0).floor();
        let s = (secs - m * 60.0).floor();
        format!("{m:.0}m {s:.0}s")
    } else {
        let h = (secs / 3600.0).floor();
        let m = ((secs - h * 3600.0) / 60.0).floor();
        format!("{h:.0}h {m:.0}m")
    }
}

fn short(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}…", &s[..n])
    }
}

#[cfg(not(target_arch = "wasm32"))]
async fn fetch_active_runs(computer: &RecordId) -> Result<Vec<ActiveRun>, String> {
    // Project `touched_components` as `array<string>` via
    // `array::map(... |$c| type::string($c))` so the JSON we get back
    // is a flat array of canonical "table:key" strings rather than
    // SurrealDB's nested RecordId-as-object shape.
    let rows: Vec<serde_json::Value> = db()
        .query(
            "SELECT type::string(id) AS id, tool_label, started_at, \
                 duration_planned_secs, \
                 type::string(target_component) AS target_component, \
                 array::map(touched_components ?? [], |$c| type::string($c)) AS touched_strs \
             FROM stress_test_run \
             WHERE computer = $cid AND result = 'in_progress' \
             ORDER BY started_at DESC LIMIT 20",
        )
        .bind(("cid", computer.clone()))
        .await
        .map_err(|e| e.to_string())?
        .take(0)
        .map_err(|e| e.to_string())?;
    let now_inst = Instant::now();
    let now_utc = chrono::Utc::now();
    Ok(rows
        .into_iter()
        .map(|row| {
            let run_id = row
                .get("id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default();
            let tool_label = row
                .get("tool_label")
                .and_then(|v| v.as_str())
                .unwrap_or("?")
                .to_string();
            let duration_planned_secs = row
                .get("duration_planned_secs")
                .and_then(|v| v.as_u64());
            let target_component = row
                .get("target_component")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty() && *s != "NONE")
                .map(|s| s.to_string());
            let touched_components: Vec<String> = row
                .get("touched_strs")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str())
                        .filter(|s| !s.is_empty() && *s != "NONE")
                        .map(|s| s.to_string())
                        .collect()
                })
                .unwrap_or_default();
            let elapsed_ms = row
                .get("started_at")
                .and_then(|v| v.as_str())
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| {
                    let delta = now_utc.signed_duration_since(dt.with_timezone(&chrono::Utc));
                    delta.num_milliseconds().max(0) as u64
                })
                .unwrap_or(0);
            ActiveRun {
                run_id,
                tool_label,
                started_at: now_inst
                    .checked_sub(Duration::from_millis(elapsed_ms))
                    .unwrap_or(now_inst),
                duration_planned_secs,
                source: ActiveRunSource::Database,
                target_component,
                touched_components,
                current_stage_idx: None,
                current_stage_total: None,
                current_stage_name: None,
            }
        })
        .collect())
}

#[cfg(target_arch = "wasm32")]
async fn fetch_active_runs(_computer: &RecordId) -> Result<Vec<ActiveRun>, String> {
    Ok(Vec::new())
}

/// Inventory poll. Pulls every distinct hardware_component this client's
/// `computer` has stress-tested before (target OR touched), then a slice
/// of recent runs per component. Two queries, both small. Runs every
/// 5 s so the cards stay fresh without hammering the DB.
#[cfg(not(target_arch = "wasm32"))]
async fn fetch_inventory(computer: &RecordId) -> Result<Vec<InventoryCard>, String> {
    // 1. Pull recent runs for this computer with target_component AND
    //    every entry in touched_components flattened to a string array
    //    via `array::map(... |$c| type::string($c))`. We bucket per
    //    component in Rust to avoid SurrealQL gymnastics — `IN` on a
    //    flattened nested array doesn't have a clean SurrealDB 3.x
    //    expression and previous attempts (`touched_components[*]`)
    //    silently returned empty.
    let run_rows: Vec<serde_json::Value> = db()
        .query(
            "SELECT type::string(id) AS id, tool_label, result, started_at, \
                 type::string(target_component) AS target_component, \
                 array::map(touched_components ?? [], |$c| type::string($c)) AS touched_strs \
             FROM stress_test_run \
             WHERE computer = $cid \
             ORDER BY started_at DESC LIMIT 500",
        )
        .bind(("cid", computer.clone()))
        .await
        .map_err(|e| e.to_string())?
        .take(0)
        .map_err(|e| e.to_string())?;

    let now_utc = chrono::Utc::now();

    // Each entry: (PastRun, list of all component ids this run touched).
    let runs: Vec<(PastRun, Vec<String>)> = run_rows
        .into_iter()
        .map(|row| {
            let mut comps: Vec<String> = Vec::new();
            if let Some(t) = row
                .get("target_component")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty() && *s != "NONE")
            {
                comps.push(t.to_string());
            }
            if let Some(arr) = row.get("touched_strs").and_then(|v| v.as_array()) {
                for v in arr {
                    if let Some(s) = v.as_str() {
                        let trimmed = s.trim();
                        if !trimmed.is_empty() && trimmed != "NONE" {
                            comps.push(trimmed.to_string());
                        }
                    }
                }
            }
            // Dedupe within a single run (target often duplicates a touched entry).
            comps.sort();
            comps.dedup();

            let started_at_str = row
                .get("started_at")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let (unix_ms, label) = match chrono::DateTime::parse_from_rfc3339(started_at_str) {
                Ok(dt) => {
                    let utc = dt.with_timezone(&chrono::Utc);
                    (utc.timestamp_millis(), fmt_relative(&now_utc, &utc))
                }
                Err(_) => (0, "?".to_string()),
            };
            (
                PastRun {
                    run_id: row
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    tool_label: row
                        .get("tool_label")
                        .and_then(|v| v.as_str())
                        .unwrap_or("?")
                        .to_string(),
                    result: row
                        .get("result")
                        .and_then(|v| v.as_str())
                        .unwrap_or("?")
                        .to_string(),
                    started_at_label: label,
                    started_at_unix_ms: unix_ms,
                },
                comps,
            )
        })
        .collect();

    // 2. Distinct component IDs across every run we just pulled.
    let mut wanted_ids: Vec<String> = runs
        .iter()
        .flat_map(|(_, comps)| comps.iter().cloned())
        .collect();
    wanted_ids.sort();
    wanted_ids.dedup();
    if wanted_ids.is_empty() {
        return Ok(Vec::new());
    }

    // 3. Parse the canonical strings back into RecordIds for binding.
    //    `id IN $ids` expects a Vec<RecordId>, not strings.
    let wanted_rids: Vec<RecordId> = wanted_ids
        .iter()
        .filter_map(|s| {
            let (tbl, key) = s.split_once(':')?;
            if tbl.is_empty() || key.is_empty() {
                return None;
            }
            // Strip backticks SurrealDB adds when keys contain hyphens.
            let key = key.trim_matches('`');
            Some(RecordId::new(tbl, key))
        })
        .collect();

    // 4. Fetch the hardware_component rows in one query.
    let comp_rows: Vec<serde_json::Value> = db()
        .query(
            "SELECT type::string(id) AS id, kind, vendor, model, display_name \
             FROM hardware_component \
             WHERE id IN $ids \
             ORDER BY kind ASC, display_name ASC",
        )
        .bind(("ids", wanted_rids))
        .await
        .map_err(|e| e.to_string())?
        .take(0)
        .map_err(|e| e.to_string())?;

    Ok(comp_rows
        .into_iter()
        .map(|c| {
            let component_id = c.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let kind = c.get("kind").and_then(|v| v.as_str()).unwrap_or("?").to_string();
            let vendor = c.get("vendor").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let model = c.get("model").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let display_name = c
                .get("display_name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| format!("{vendor} {model}").trim().to_string());

            // Bucket: any run whose component list contains this card's id.
            let mut recent_runs: Vec<PastRun> = runs
                .iter()
                .filter(|(_, comps)| comps.iter().any(|c| c == &component_id))
                .map(|(r, _)| r.clone())
                .collect();
            recent_runs.sort_by(|a, b| b.started_at_unix_ms.cmp(&a.started_at_unix_ms));
            recent_runs.dedup_by(|a, b| a.run_id == b.run_id);
            recent_runs.truncate(HISTORY_PER_COMPONENT);

            InventoryCard {
                component_id,
                display_name,
                kind,
                vendor,
                model,
                recent_runs,
            }
        })
        .collect())
}

#[cfg(target_arch = "wasm32")]
async fn fetch_inventory(_computer: &RecordId) -> Result<Vec<InventoryCard>, String> {
    Ok(Vec::new())
}

/// True when this active run targets the given component OR has it
/// listed under `touched_components`. Used by the inventory render to
/// dock the inline progress bar inside every card a run is exercising
/// — `gpu_pcie`, for instance, lists both CPU + GPU under
/// `touched_components` and should appear under both cards.
fn run_touches_component(run: &ActiveRun, component_id: &str) -> bool {
    if run
        .target_component
        .as_deref()
        .map(|t| t == component_id)
        .unwrap_or(false)
    {
        return true;
    }
    run.touched_components.iter().any(|c| c == component_id)
}

/// Best-effort CPU vendor pulled from the brand string. Mirrors
/// `stress-runner::hardware::classify_cpu_vendor` so phantom rows we
/// upsert here collide with stress-run-derived rows on `canonical_id`.
fn classify_cpu_vendor(brand: &str) -> String {
    let b = brand.to_lowercase();
    if b.contains("amd") || b.contains("ryzen") || b.contains("epyc") || b.contains("threadripper") {
        "AMD".into()
    } else if b.contains("intel") || b.contains("xeon") || b.contains("core(tm)") || b.contains("pentium") {
        "Intel".into()
    } else if b.contains("apple") {
        "Apple".into()
    } else if b.contains("arm") {
        "ARM".into()
    } else {
        "Unknown".into()
    }
}

fn classify_gpu_vendor(label: &str) -> String {
    let l = label.to_lowercase();
    if l.contains("nvidia") || l.contains("geforce") || l.contains("quadro") || l.contains("rtx") || l.contains("gtx") {
        "NVIDIA".into()
    } else if l.contains("amd") || l.contains("radeon") || l.contains("amdgpu") {
        "AMD".into()
    } else if l.contains("intel") || l.contains("i915") || l.contains("arc") {
        "Intel".into()
    } else {
        "Unknown".into()
    }
}

/// "Just now" / "12s ago" / "4m ago" / "2h ago" / "3d ago"
fn fmt_relative(now: &chrono::DateTime<chrono::Utc>, then: &chrono::DateTime<chrono::Utc>) -> String {
    let delta = now.signed_duration_since(*then);
    let secs = delta.num_seconds();
    if secs < 5 {
        "just now".to_string()
    } else if secs < 60 {
        format!("{secs}s ago")
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86_400 {
        format!("{}h ago", secs / 3600)
    } else {
        format!("{}d ago", secs / 86_400)
    }
}

#[cfg(test)]
mod tests {
    use super::terse_driver_protections_status;

    fn terse(hvci: Option<bool>, blocklist: Option<bool>) -> String {
        terse_driver_protections_status(true, hvci, blocklist, None, None, true, "")
    }

    /// A half-applied change must not read as an unreadable box.
    #[test]
    fn mixed_protection_states_are_named() {
        assert_eq!(
            terse(Some(true), Some(true)),
            "Protections on — reboot required"
        );
        assert_eq!(
            terse(Some(false), Some(true)),
            "MIXED: HVCI off, blocklist on — reboot required"
        );
        assert_eq!(
            terse(Some(false), None),
            "PARTIAL: HVCI off, blocklist unreadable — reboot required"
        );
        assert_eq!(
            terse(None, Some(true)),
            "PARTIAL: HVCI unreadable, blocklist on — reboot required"
        );
        assert_eq!(terse(None, None), "Protections state unreadable");
    }
}
