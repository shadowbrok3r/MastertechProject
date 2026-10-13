//! Root-only management console for the axum orchestrator.
//!
//! Reads `/api/v1/admin/*`, which the server gates on a SurrealDB record token
//! belonging to a `Root` user; the signed-in operator's cached JWT is the
//! bearer. Answers "what did the server actually receive" — the pre-boot
//! section attributes every console advertisement to a socket peer, a
//! User-Agent, and its raw body, including the ones that were rejected.
//!
//! The DTOs mirror `axum_server::routes::api::admin` and
//! `axum_server::routes::api::preboot`; they are duplicated rather than shared
//! so the client never links the server crate.

use std::collections::BTreeMap;
use std::time::Duration;

use crossbeam::channel::{Receiver, Sender};
use eframe::egui::{Align, Color32, Grid, Layout, RichText, ScrollArea, Ui};
use egui_extras::{Column, TableBuilder};
use serde::Deserialize;
use web_time::Instant;

use crate::tabs::admin_console::current_user_is_root;
use crate::ui_tools::controls::{
    self, BarKind, ButtonKind, IconBox, Metrics, SpinnerSize, TextArea, TextButton, TextField,
};
use crate::ui_tools::icons::svg::{self, Icon};
use crate::ui_tools::theme;
use crate::{PlatformSpawner, Spawner};

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CaptureConfig {
    pub enabled: bool,
    pub capacity: usize,
    pub max_body: usize,
    pub body_paths: Vec<String>,
    pub record_admin: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RootIdentity {
    pub email: String,
    pub name: String,
    pub user_id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerInfo {
    pub version: String,
    pub pid: u32,
    pub started_at: String,
    pub uptime_secs: u64,
    pub now: String,
    pub rust_log: String,
    pub db_url: String,
    pub db_connected: bool,
    pub capture: CaptureConfig,
    pub recorded: usize,
    pub next_seq: u64,
    pub preboot_sessions: usize,
    pub preboot_consoles: usize,
    pub preboot_rejects: usize,
    pub fleet_agents: usize,
    pub viewer: RootIdentity,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RequestRecord {
    pub seq: u64,
    pub req_id: String,
    pub at: String,
    pub method: String,
    pub path: String,
    pub query: Option<String>,
    pub version: String,
    pub peer: Option<String>,
    pub forwarded_for: Option<String>,
    pub real_ip: Option<String>,
    pub user_agent: Option<String>,
    pub content_type: Option<String>,
    pub content_length: Option<u64>,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
    pub body_bytes: usize,
    pub body_truncated: bool,
    pub status: u16,
    pub latency_ms: f64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RequestPage {
    pub total: usize,
    pub records: Vec<RequestRecord>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PathStat {
    pub key: String,
    pub count: u64,
    pub last_at: String,
    pub statuses: BTreeMap<u16, u64>,
    pub bytes_in: u64,
    pub latency_ms_total: f64,
    pub latency_ms_max: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PathPage {
    pub overflow: u64,
    pub paths: Vec<PathStat>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Origin {
    pub peer: Option<String>,
    pub forwarded_for: Option<String>,
    pub real_ip: Option<String>,
    pub user_agent: Option<String>,
    pub content_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConsoleDetail {
    pub addr: String,
    pub age_secs: u64,
    pub alive_secs: u64,
    pub advert_count: u64,
    pub last_interval_secs: Option<u64>,
    pub first_seen_at: String,
    pub last_seen_at: String,
    pub raw_body: String,
    pub origin: Origin,
    pub fresh: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SessionDetail {
    pub serial: String,
    pub idle_secs: u64,
    pub frame_seq: u64,
    pub has_frame: bool,
    pub frame_bytes: usize,
    pub streaming: bool,
    pub viewer: bool,
    pub queued_input: usize,
    pub log_lines: usize,
    pub last_seen_at: Option<String>,
    pub origin: Origin,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConsoleReject {
    pub at: String,
    pub reason: String,
    pub raw_body: String,
    pub body_bytes: usize,
    pub origin: Origin,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PreBootDetail {
    pub now: String,
    pub session_idle_secs: u64,
    pub console_ttl_secs: u64,
    pub advert_total: u64,
    pub sessions: Vec<SessionDetail>,
    pub consoles: Vec<ConsoleDetail>,
    pub rejects: Vec<ConsoleReject>,
    pub orphan_logs: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Section {
    #[default]
    Overview,
    PreBoot,
    Requests,
    Paths,
}

impl Section {
    fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::PreBoot => "Pre-Boot",
            Self::Requests => "Requests",
            Self::Paths => "Paths",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Self::Overview => svg::GAUGE,
            Self::PreBoot => svg::MT_FIRMWARE,
            Self::Requests => svg::LIST_MAGNIFYING_GLASS,
            Self::Paths => svg::TREE_STRUCTURE,
        }
    }
}

/// One completed background fetch.
enum Fetched {
    Info(Result<ServerInfo, String>),
    Requests(Result<RequestPage, String>),
    Paths(Result<PathPage, String>),
    PreBoot(Result<PreBootDetail, String>),
    Capture(Result<CaptureConfig, String>),
    Action(Result<String, String>),
}

pub struct ServerConsole {
    pub base_url: String,
    section: Section,
    auto_refresh: bool,
    interval_secs: u32,
    last_poll: Option<Instant>,
    inflight: u32,
    status: String,
    info: Option<ServerInfo>,
    requests: Vec<RequestRecord>,
    requests_total: usize,
    paths: Vec<PathStat>,
    paths_overflow: u64,
    preboot: Option<PreBootDetail>,
    selected_seq: Option<u64>,
    limit: usize,
    filter_path: String,
    filter_method: String,
    filter_status: String,
    filter_contains: String,
    capture_draft: Option<CaptureConfig>,
    body_paths_draft: String,
    /// Set when a mutating call succeeded; the next frame re-reads the section.
    refresh_after_action: bool,
    tx: Sender<Fetched>,
    rx: Receiver<Fetched>,
}

impl Default for ServerConsole {
    fn default() -> Self {
        let (tx, rx) = crossbeam::channel::unbounded();
        Self {
            base_url: database::orchestrator_url().to_string(),
            section: Section::default(),
            auto_refresh: false,
            interval_secs: 10,
            last_poll: None,
            inflight: 0,
            status: String::new(),
            info: None,
            requests: Vec::new(),
            requests_total: 0,
            paths: Vec::new(),
            paths_overflow: 0,
            preboot: None,
            selected_seq: None,
            limit: 200,
            filter_path: String::new(),
            filter_method: String::new(),
            filter_status: String::new(),
            filter_contains: String::new(),
            capture_draft: None,
            body_paths_draft: String::new(),
            refresh_after_action: false,
            tx,
            rx,
        }
    }
}

/// SurrealDB record token for the signed-in operator; the server re-verifies it.
fn auth_token() -> Option<String> {
    database::CACHED_AUTH
        .try_lock()
        .ok()
        .and_then(|g| g.as_ref().and_then(|c| c.jwt.clone()))
}

async fn get_json<T: serde::de::DeserializeOwned>(url: String, token: String) -> Result<T, String> {
    let resp = reqwest::Client::new()
        .get(&url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    decode(resp).await
}

async fn send_json<T: serde::de::DeserializeOwned>(
    method: reqwest::Method,
    url: String,
    token: String,
    body: Option<serde_json::Value>,
) -> Result<T, String> {
    let mut req = reqwest::Client::new()
        .request(method, &url)
        .header("Authorization", format!("Bearer {token}"));
    if let Some(b) = body {
        req = req.json(&b);
    }
    decode(req.send().await.map_err(|e| e.to_string())?).await
}

async fn decode<T: serde::de::DeserializeOwned>(resp: reqwest::Response) -> Result<T, String> {
    let status = resp.status();
    let text = resp.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        let detail = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string))
            .unwrap_or_else(|| text.trim().to_string());
        return Err(format!("HTTP {}: {detail}", status.as_u16()));
    }
    serde_json::from_str(&text).map_err(|e| format!("decode failed: {e}"))
}

impl ServerConsole {
    fn base(&self) -> String {
        self.base_url.trim().trim_end_matches('/').to_string()
    }

    /// Fetch whatever the active section needs, plus the always-visible header info.
    fn refresh(&mut self) {
        self.last_poll = Some(Instant::now());
        let Some(token) = auth_token() else {
            self.status = "No SurrealDB session token — sign in again.".to_string();
            return;
        };
        let base = self.base();
        if base.is_empty() {
            self.status = "No orchestrator URL configured.".to_string();
            return;
        }
        self.status.clear();

        self.spawn_info(&base, &token);
        match self.section {
            Section::Overview => {}
            Section::PreBoot => self.spawn_preboot(&base, &token),
            Section::Requests => self.spawn_requests(&base, &token),
            Section::Paths => self.spawn_paths(&base, &token),
        }
    }

    fn spawn_info(&mut self, base: &str, token: &str) {
        let (tx, url, token) = (self.tx.clone(), format!("{base}/api/v1/admin/info"), token.to_string());
        self.inflight += 1;
        PlatformSpawner::spawn(async move {
            let _ = tx.send(Fetched::Info(get_json(url, token).await));
        });
    }

    fn spawn_preboot(&mut self, base: &str, token: &str) {
        let (tx, url, token) =
            (self.tx.clone(), format!("{base}/api/v1/admin/preboot"), token.to_string());
        self.inflight += 1;
        PlatformSpawner::spawn(async move {
            let _ = tx.send(Fetched::PreBoot(get_json(url, token).await));
        });
    }

    fn spawn_paths(&mut self, base: &str, token: &str) {
        let (tx, url, token) =
            (self.tx.clone(), format!("{base}/api/v1/admin/paths"), token.to_string());
        self.inflight += 1;
        PlatformSpawner::spawn(async move {
            let _ = tx.send(Fetched::Paths(get_json(url, token).await));
        });
    }

    fn spawn_requests(&mut self, base: &str, token: &str) {
        let mut q: Vec<String> = vec![format!("limit={}", self.limit)];
        let mut add = |k: &str, v: &str| {
            if !v.trim().is_empty() {
                q.push(format!("{k}={}", urlencode(v.trim())));
            }
        };
        add("path", &self.filter_path);
        add("method", &self.filter_method);
        add("status", &self.filter_status);
        add("contains", &self.filter_contains);
        let url = format!("{base}/api/v1/admin/requests?{}", q.join("&"));
        let (tx, token) = (self.tx.clone(), token.to_string());
        self.inflight += 1;
        PlatformSpawner::spawn(async move {
            let _ = tx.send(Fetched::Requests(get_json(url, token).await));
        });
    }

    fn spawn_action(&mut self, method: reqwest::Method, path: String, label: String) {
        let Some(token) = auth_token() else {
            self.status = "No SurrealDB session token — sign in again.".to_string();
            return;
        };
        let url = format!("{}{path}", self.base());
        let tx = self.tx.clone();
        self.inflight += 1;
        PlatformSpawner::spawn(async move {
            let res = send_json::<serde_json::Value>(method, url, token, None)
                .await
                .map(|v| format!("{label}: {v}"));
            let _ = tx.send(Fetched::Action(res));
        });
    }

    fn spawn_capture_update(&mut self, cfg: CaptureConfig) {
        let Some(token) = auth_token() else {
            self.status = "No SurrealDB session token — sign in again.".to_string();
            return;
        };
        let url = format!("{}/api/v1/admin/capture", self.base());
        let body = serde_json::json!({
            "enabled": cfg.enabled,
            "capacity": cfg.capacity,
            "max_body": cfg.max_body,
            "body_paths": cfg.body_paths,
            "record_admin": cfg.record_admin,
        });
        let tx = self.tx.clone();
        self.inflight += 1;
        PlatformSpawner::spawn(async move {
            let res = send_json(reqwest::Method::POST, url, token, Some(body)).await;
            let _ = tx.send(Fetched::Capture(res));
        });
    }

    fn drain(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            self.inflight = self.inflight.saturating_sub(1);
            match msg {
                Fetched::Info(Ok(v)) => {
                    if self.capture_draft.is_none() {
                        self.body_paths_draft = v.capture.body_paths.join(", ");
                        self.capture_draft = Some(v.capture.clone());
                    }
                    self.info = Some(v);
                }
                Fetched::Requests(Ok(p)) => {
                    self.requests_total = p.total;
                    self.requests = p.records;
                }
                Fetched::Paths(Ok(p)) => {
                    self.paths_overflow = p.overflow;
                    self.paths = p.paths;
                }
                Fetched::PreBoot(Ok(p)) => self.preboot = Some(p),
                Fetched::Capture(Ok(c)) => {
                    self.body_paths_draft = c.body_paths.join(", ");
                    self.capture_draft = Some(c);
                    self.status = "Capture settings applied.".to_string();
                }
                Fetched::Action(Ok(msg)) => {
                    self.status = msg;
                    self.refresh_after_action = true;
                }
                Fetched::Info(Err(e))
                | Fetched::Requests(Err(e))
                | Fetched::Paths(Err(e))
                | Fetched::PreBoot(Err(e))
                | Fetched::Capture(Err(e))
                | Fetched::Action(Err(e)) => self.status = e,
            }
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        if !current_user_is_root() {
            controls::empty_state(
                ui,
                svg::LOCK_KEY,
                "Root authorization required",
                "This console exposes raw request data from the orchestrator.",
                &[],
            );
            return;
        }

        self.drain();
        if self.refresh_after_action && self.inflight == 0 {
            self.refresh_after_action = false;
            self.refresh();
        }
        if self.inflight > 0 {
            ui.ctx().request_repaint();
        }
        if self.auto_refresh {
            let due = self
                .last_poll
                .is_none_or(|t| t.elapsed().as_secs() >= self.interval_secs.max(2) as u64);
            if due && self.inflight == 0 {
                self.refresh();
            }
            ui.ctx().request_repaint_after(Duration::from_secs(1));
        }
        if self.info.is_none() && self.last_poll.is_none() {
            self.refresh();
        }

        self.toolbar(ui);
        match self.section {
            Section::Overview => self.overview_ui(ui),
            Section::PreBoot => self.preboot_ui(ui),
            Section::Requests => self.requests_ui(ui),
            Section::Paths => self.paths_ui(ui),
        }
    }

    fn toolbar(&mut self, ui: &mut Ui) {
        controls::bar(ui, BarKind::Tool, |ui| {
            for section in [Section::Overview, Section::PreBoot, Section::Requests, Section::Paths] {
                if controls::pill(ui, section.icon(), section.label(), self.section == section)
                    .clicked()
                {
                    self.section = section;
                    self.refresh();
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if controls::icon_text_button(ui, svg::ARROW_CLOCKWISE, "Refresh").clicked() {
                    self.refresh();
                }
                if self.auto_refresh {
                    controls::value_field(ui, &mut self.interval_secs, 2..=300, "s", 64.0)
                        .on_hover_text("Refresh interval");
                }
                controls::toggle(ui, &mut self.auto_refresh, "Auto");
                if self.inflight > 0 {
                    controls::spinner(ui, SpinnerSize::Sm);
                }
            });
        });
        controls::bar(ui, BarKind::Header, |ui| {
            TextField::new(&mut self.base_url)
                .icon(svg::GLOBE)
                .width(280.0)
                .hint("Relay URL")
                .show(ui)
                .on_hover_text("Relay");
            let base = self.base();
            let insecure = !base.is_empty()
                && !base.starts_with("https://")
                && !base.contains("localhost")
                && !base.contains("127.0.0.1");
            if insecure {
                controls::status_label(ui, svg::LOCK_OPEN, "plaintext relay — session token sent unencrypted", theme::warn(ui));
            }
        });
        if !self.status.is_empty() {
            let error = self.status.starts_with("HTTP") || self.status.contains("failed");
            let (icon, color) = if error {
                (svg::WARNING_CIRCLE, theme::error(ui))
            } else {
                (svg::INFO, theme::weak_text(ui))
            };
            controls::bar(ui, BarKind::Status, |ui| {
                controls::status_label(ui, icon, &self.status, color);
                if controls::icon_button(ui, svg::X, IconBox::Sm, false, "Dismiss").clicked() {
                    self.status.clear();
                }
            });
        }
    }

    fn overview_ui(&mut self, ui: &mut Ui) {
        let Some(info) = self.info.clone() else {
            controls::empty_state(ui, svg::HARD_DRIVES, "No server info yet", "", &[]);
            return;
        };
        let mut draft = self.capture_draft.clone().unwrap_or_else(|| info.capture.clone());
        let mut body_paths = self.body_paths_draft.clone();
        let mut apply = false;
        let mut clear = false;
        let m = Metrics::get(ui.ctx());

        ScrollArea::vertical().show(ui, |ui| {
            if controls::section_header(ui, "server_console_info", svg::HARD_DRIVES, "Server") {
                Grid::new("server_console_info").num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
                    let mut row = |icon: Icon, k: &str, v: String| {
                        controls::icon_label(ui, icon, k);
                        ui.label(v);
                        ui.end_row();
                    };
                    row(
                        svg::USER_CIRCLE,
                        "Signed in as",
                        format!("{} <{}>", info.viewer.name, info.viewer.email),
                    );
                    row(svg::TAG, "Version", info.version.clone());
                    row(svg::HASH, "PID", info.pid.to_string());
                    row(svg::CALENDAR, "Started", info.started_at.clone());
                    row(svg::TIMER, "Uptime", human_secs(info.uptime_secs));
                    row(svg::CLOCK, "Server time", info.now.clone());
                    row(svg::TERMINAL, "RUST_LOG", info.rust_log.clone());
                    row(
                        svg::DATABASE,
                        "SurrealDB",
                        format!(
                            "{} ({})",
                            info.db_url,
                            if info.db_connected { "connected" } else { "unreachable" }
                        ),
                    );
                    row(svg::ROBOT, "Fleet agents", info.fleet_agents.to_string());
                    row(svg::MT_FIRMWARE, "Pre-boot sessions", info.preboot_sessions.to_string());
                    row(svg::MONITOR, "Pre-boot consoles", info.preboot_consoles.to_string());
                    row(svg::PROHIBIT, "Rejected adverts", info.preboot_rejects.to_string());
                    row(
                        svg::LIST_NUMBERS,
                        "Recorded requests",
                        format!("{} (next seq {})", info.recorded, info.next_seq),
                    );
                });
            }

            ui.add_space(m.gap.y);
            if controls::section_header(
                ui,
                "server_console_capture",
                svg::TRAY_ARROW_DOWN,
                "Request capture",
            ) {
                ui.label(
                    RichText::new(
                        "Bodies are buffered only for the listed path prefixes; every other path records metadata only.",
                    )
                    .weak(),
                );
                Grid::new("server_console_capture").num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
                    controls::icon_label(ui, svg::PULSE, "Recording");
                    controls::toggle(ui, &mut draft.enabled, "enabled");
                    ui.end_row();
                    controls::icon_label(ui, svg::STACK, "Ring capacity");
                    controls::value_field(ui, &mut draft.capacity, 1..=10_000, "", 96.0);
                    ui.end_row();
                    controls::icon_label(ui, svg::FILE_TEXT, "Max body bytes");
                    controls::value_field(ui, &mut draft.max_body, 0..=1_048_576, "B", 120.0);
                    ui.end_row();
                    controls::icon_label(ui, svg::TREE_STRUCTURE, "Body path prefixes");
                    controls::text_field(
                        ui,
                        &mut body_paths,
                        "/api/v1/qc/preboot/console, * for all",
                        360.0,
                    );
                    ui.end_row();
                    controls::icon_label(ui, svg::SHIELD_CHECK, "Record admin API");
                    controls::toggle(ui, &mut draft.record_admin, "include /api/v1/admin traffic");
                    ui.end_row();
                });
                ui.add_space(m.gap.y);
                ui.horizontal(|ui| {
                    apply = TextButton::new("Apply")
                        .kind(ButtonKind::Primary)
                        .icon(svg::CHECK)
                        .show(ui)
                        .clicked();
                    clear = TextButton::new("Clear recorded requests")
                        .kind(ButtonKind::Danger)
                        .icon(svg::TRASH)
                        .show(ui)
                        .clicked();
                });
            }
        });

        self.body_paths_draft = body_paths;
        self.capture_draft = Some(draft.clone());
        if apply {
            let mut cfg = draft;
            cfg.body_paths = self
                .body_paths_draft
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect();
            self.spawn_capture_update(cfg);
        }
        if clear {
            self.spawn_action(
                reqwest::Method::DELETE,
                "/api/v1/admin/requests".to_string(),
                "Cleared".to_string(),
            );
        }
    }

    fn preboot_ui(&mut self, ui: &mut Ui) {
        let Some(pb) = self.preboot.clone() else {
            controls::empty_state(ui, svg::MT_FIRMWARE, "No pre-boot snapshot yet", "", &[]);
            return;
        };
        let mut evict: Option<String> = None;
        let m = Metrics::get(ui.ctx());
        ScrollArea::vertical().show(ui, |ui| {
            if controls::section_header(ui, "preboot_consoles", svg::BROADCAST, "Console advertisements") {
                ui.label(
                    RichText::new(format!(
                        "{} accepted since start · entries expire after {}s of silence",
                        pb.advert_total, pb.console_ttl_secs
                    ))
                    .weak(),
                );
                if pb.consoles.is_empty() {
                    controls::status_label(ui, svg::BROADCAST, "No console has advertised.", theme::faint_text(ui));
                } else {
                    TableBuilder::new(ui)
                        .id_salt("preboot_consoles")
                        .striped(true)
                        .resizable(true)
                        .cell_layout(Layout::left_to_right(Align::Center))
                        .column(Column::auto().at_least(150.0))
                        .column(Column::auto().at_least(150.0))
                        .column(Column::auto().at_least(110.0))
                        .column(Column::initial(200.0).at_least(120.0))
                        .column(Column::auto().at_least(60.0))
                        .column(Column::auto().at_least(70.0))
                        .column(Column::auto().at_least(60.0))
                        .column(Column::auto().at_least(170.0))
                        .column(Column::remainder().at_least(120.0))
                        .header(m.row_h, |mut h| {
                            for title in [
                                "Advertised addr",
                                "Socket peer",
                                "Forwarded for",
                                "User-Agent",
                                "Adverts",
                                "Interval",
                                "Age",
                                "First seen",
                                "Raw body",
                            ] {
                                h.col(|ui| {
                                    ui.label(RichText::new(title).strong());
                                });
                            }
                        })
                        .body(|mut body| {
                            for c in &pb.consoles {
                                body.row(m.row_h, |mut r| {
                                    r.col(|ui| {
                                        let (icon, color) = if c.fresh {
                                            (svg::BROADCAST, theme::info(ui))
                                        } else {
                                            (svg::HOURGLASS, theme::warn(ui))
                                        };
                                        controls::status_label(ui, icon, &c.addr, color);
                                    });
                                    r.col(|ui| {
                                        ui.label(opt(&c.origin.peer));
                                    });
                                    r.col(|ui| {
                                        ui.label(opt(&c.origin.forwarded_for));
                                    });
                                    r.col(|ui| {
                                        ui.label(opt(&c.origin.user_agent))
                                            .on_hover_text(opt(&c.origin.user_agent));
                                    });
                                    r.col(|ui| {
                                        ui.label(c.advert_count.to_string());
                                    });
                                    r.col(|ui| {
                                        ui.label(
                                            c.last_interval_secs
                                                .map(|s| format!("{s}s"))
                                                .unwrap_or_else(|| "—".to_string()),
                                        );
                                    });
                                    r.col(|ui| {
                                        ui.label(format!("{}s", c.age_secs));
                                    });
                                    r.col(|ui| {
                                        ui.label(&c.first_seen_at)
                                            .on_hover_text(format!("last {}", c.last_seen_at));
                                    });
                                    r.col(|ui| {
                                        ui.horizontal(|ui| {
                                            if controls::icon_button(ui, svg::COPY, IconBox::Sm, false, "Copy body")
                                                .clicked()
                                            {
                                                ui.ctx().copy_text(c.raw_body.clone());
                                            }
                                            if controls::icon_button(
                                                ui,
                                                svg::TRASH,
                                                IconBox::Sm,
                                                false,
                                                "Evict this endpoint",
                                            )
                                            .clicked()
                                            {
                                                evict = Some(c.addr.clone());
                                            }
                                            ui.label(RichText::new(&c.raw_body).monospace());
                                        });
                                    });
                                });
                            }
                        });
                }
            }

            ui.add_space(m.gap.y);
            if controls::section_header(ui, "preboot_rejects", svg::PROHIBIT, "Rejected advertisements") {
                if pb.rejects.is_empty() {
                    controls::status_label(ui, svg::CHECK_CIRCLE, "None — every POST parsed to a usable addr.", theme::faint_text(ui));
                } else {
                    TableBuilder::new(ui)
                        .id_salt("preboot_rejects")
                        .striped(true)
                        .resizable(true)
                        .cell_layout(Layout::left_to_right(Align::Center))
                        .column(Column::auto().at_least(170.0))
                        .column(Column::auto().at_least(150.0))
                        .column(Column::initial(200.0).at_least(120.0))
                        .column(Column::auto().at_least(180.0))
                        .column(Column::remainder().at_least(140.0))
                        .header(m.row_h, |mut h| {
                            for title in ["At", "Socket peer", "User-Agent", "Reason", "Raw body"] {
                                h.col(|ui| {
                                    ui.label(RichText::new(title).strong());
                                });
                            }
                        })
                        .body(|mut body| {
                            for j in &pb.rejects {
                                body.row(m.row_h, |mut r| {
                                    r.col(|ui| {
                                        ui.label(&j.at);
                                    });
                                    r.col(|ui| {
                                        ui.label(opt(&j.origin.peer));
                                    });
                                    r.col(|ui| {
                                        ui.label(opt(&j.origin.user_agent));
                                    });
                                    r.col(|ui| {
                                        controls::status_label(ui, svg::WARNING_CIRCLE, &j.reason, theme::error(ui));
                                    });
                                    r.col(|ui| {
                                        ui.label(
                                            RichText::new(format!("{} ({} bytes)", j.raw_body, j.body_bytes))
                                                .monospace(),
                                        );
                                    });
                                });
                            }
                        });
                }
            }

            ui.add_space(m.gap.y);
            if controls::section_header(ui, "preboot_sessions", svg::MT_FIRMWARE, "Firmware sessions") {
                ui.label(
                    RichText::new(format!("Swept after {}s without firmware traffic", pb.session_idle_secs))
                        .weak(),
                );
                if pb.sessions.is_empty() {
                    controls::status_label(ui, svg::MT_FIRMWARE, "No firmware is checking in.", theme::faint_text(ui));
                } else {
                    TableBuilder::new(ui)
                        .id_salt("preboot_sessions")
                        .striped(true)
                        .resizable(true)
                        .cell_layout(Layout::left_to_right(Align::Center))
                        .column(Column::auto().at_least(160.0))
                        .column(Column::auto().at_least(60.0))
                        .column(Column::auto().at_least(90.0))
                        .column(Column::auto().at_least(70.0))
                        .column(Column::auto().at_least(60.0))
                        .column(Column::auto().at_least(60.0))
                        .column(Column::auto().at_least(140.0))
                        .column(Column::remainder().at_least(140.0))
                        .header(m.row_h, |mut h| {
                            for title in [
                                "Serial", "Idle", "Frame", "Streaming", "Viewer", "Logs", "Socket peer",
                                "User-Agent",
                            ] {
                                h.col(|ui| {
                                    ui.label(RichText::new(title).strong());
                                });
                            }
                        })
                        .body(|mut body| {
                            for s in &pb.sessions {
                                body.row(m.row_h, |mut r| {
                                    r.col(|ui| {
                                        ui.label(&s.serial);
                                    });
                                    r.col(|ui| {
                                        ui.label(format!("{}s", s.idle_secs));
                                    });
                                    r.col(|ui| {
                                        ui.label(format!("#{} / {}B", s.frame_seq, s.frame_bytes));
                                    });
                                    r.col(|ui| {
                                        yes_no(ui, s.streaming);
                                    });
                                    r.col(|ui| {
                                        yes_no(ui, s.viewer);
                                    });
                                    r.col(|ui| {
                                        ui.label(s.log_lines.to_string());
                                    });
                                    r.col(|ui| {
                                        ui.label(opt(&s.origin.peer));
                                    });
                                    r.col(|ui| {
                                        ui.label(opt(&s.origin.user_agent));
                                    });
                                });
                            }
                        });
                }
            }

            if !pb.orphan_logs.is_empty() {
                ui.add_space(m.gap.y);
                controls::status_label(ui, svg::SCROLL, &format!("Retained logs with no live session: {}", pb.orphan_logs.join(", ")), theme::faint_text(ui));
            }
        });

        if let Some(addr) = evict {
            self.spawn_action(
                reqwest::Method::DELETE,
                format!("/api/v1/admin/preboot/console/{}", urlencode(&addr)),
                format!("Evicted {addr}"),
            );
        }
    }

    fn requests_ui(&mut self, ui: &mut Ui) {
        let m = Metrics::get(ui.ctx());
        controls::bar(ui, BarKind::Tool, |ui| {
            TextField::new(&mut self.filter_path)
                .icon(svg::TREE_STRUCTURE)
                .hint("/api/v1/qc")
                .width(180.0)
                .show(ui)
                .on_hover_text("Path");
            TextField::new(&mut self.filter_method)
                .hint("POST")
                .width(64.0)
                .show(ui)
                .on_hover_text("Method");
            TextField::new(&mut self.filter_status)
                .hint("400")
                .width(56.0)
                .show(ui)
                .on_hover_text("Status");
            controls::search_field(ui, &mut self.filter_contains, "body / UA / peer", 180.0)
                .on_hover_text("Contains");
            controls::icon_label(ui, svg::LIST_NUMBERS, "Limit");
            controls::value_field(ui, &mut self.limit, 10..=2000, "", 64.0);
            if controls::icon_text_button(ui, svg::FUNNEL, "Apply").clicked() {
                self.refresh();
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(format!(
                        "{} shown of {} recorded",
                        self.requests.len(),
                        self.requests_total
                    ))
                    .weak(),
                );
            });
        });

        if self.requests.is_empty() {
            controls::empty_state(
                ui,
                svg::TRAY,
                "No recorded requests",
                "Recording is configured under Overview, Request capture.",
                &[],
            );
            return;
        }
        let selected = self.selected_seq;
        let mut clicked: Option<u64> = None;
        let available = ui.available_height();
        let table_height = if selected.is_some() { available * 0.55 } else { available };
        ScrollArea::vertical()
            .id_salt("requests_table_scroll")
            .max_height(table_height)
            .show(ui, |ui| {
                TableBuilder::new(ui)
                    .id_salt("requests_table")
                    .striped(true)
                    .resizable(true)
                    .cell_layout(Layout::left_to_right(Align::Center))
                    .column(Column::auto().at_least(50.0))
                    .column(Column::auto().at_least(170.0))
                    .column(Column::auto().at_least(56.0))
                    .column(Column::initial(240.0).at_least(140.0))
                    .column(Column::auto().at_least(50.0))
                    .column(Column::auto().at_least(60.0))
                    .column(Column::auto().at_least(150.0))
                    .column(Column::remainder().at_least(140.0))
                    .header(m.row_h, |mut h| {
                        for title in
                            ["Seq", "At", "Method", "Path", "Status", "ms", "Socket peer", "User-Agent"]
                        {
                            h.col(|ui| {
                                ui.label(RichText::new(title).strong());
                            });
                        }
                    })
                    .body(|mut body| {
                        for rec in self.requests.iter().rev() {
                            body.row(m.row_h, |mut r| {
                                r.set_selected(selected == Some(rec.seq));
                                r.col(|ui| {
                                    ui.label(rec.seq.to_string());
                                });
                                r.col(|ui| {
                                    ui.label(&rec.at);
                                });
                                r.col(|ui| {
                                    ui.label(&rec.method);
                                });
                                r.col(|ui| {
                                    let full = match &rec.query {
                                        Some(q) => format!("{}?{q}", rec.path),
                                        None => rec.path.clone(),
                                    };
                                    ui.label(&full).on_hover_text(full.clone());
                                });
                                r.col(|ui| {
                                    ui.colored_label(status_color(ui, rec.status), rec.status.to_string());
                                });
                                r.col(|ui| {
                                    ui.label(format!("{:.1}", rec.latency_ms));
                                });
                                r.col(|ui| {
                                    ui.label(opt(&rec.peer));
                                });
                                r.col(|ui| {
                                    ui.label(opt(&rec.user_agent));
                                });
                                if r.response().clicked() {
                                    clicked = Some(rec.seq);
                                }
                            });
                        }
                    });
            });

        if let Some(seq) = clicked {
            self.selected_seq = if self.selected_seq == Some(seq) { None } else { Some(seq) };
        }

        let Some(seq) = self.selected_seq else { return };
        let Some(rec) = self.requests.iter().find(|r| r.seq == seq).cloned() else {
            return;
        };
        controls::bar(ui, BarKind::Header, |ui| {
            let title = format!("#{} {} {}", rec.seq, rec.method, rec.path);
            controls::subheading(ui, svg::FILE_MAGNIFYING_GLASS, &title);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if controls::icon_button(ui, svg::X, IconBox::Sm, false, "Close").clicked() {
                    self.selected_seq = None;
                }
                if controls::icon_text_button(ui, svg::COPY, "Copy body").clicked() {
                    ui.ctx().copy_text(rec.body.clone().unwrap_or_default());
                }
            });
        });
        ScrollArea::vertical().id_salt("request_detail").show(ui, |ui| {
            Grid::new("request_detail_grid").num_columns(2).spacing([16.0, 2.0]).show(ui, |ui| {
                let mut row = |k: &str, v: String| {
                    ui.label(RichText::new(k).strong());
                    ui.label(v);
                    ui.end_row();
                };
                row("Request id", rec.req_id.clone());
                row("At", rec.at.clone());
                row("HTTP", format!("{} → {}", rec.version, rec.status));
                row("Latency", format!("{:.2} ms", rec.latency_ms));
                row("Socket peer", opt(&rec.peer));
                row("X-Forwarded-For", opt(&rec.forwarded_for));
                row("X-Real-IP", opt(&rec.real_ip));
                row("User-Agent", opt(&rec.user_agent));
                row("Content-Type", opt(&rec.content_type));
                row(
                    "Content-Length",
                    rec.content_length.map(|v| v.to_string()).unwrap_or_else(|| "—".to_string()),
                );
                if let Some(err) = &rec.error {
                    row("Handler error", err.clone());
                }
            });
            ui.add_space(m.gap.y);
            controls::section_label(ui, "Headers");
            Grid::new("request_headers_grid").num_columns(2).spacing([16.0, 2.0]).show(ui, |ui| {
                for (k, v) in &rec.headers {
                    ui.label(RichText::new(k).monospace());
                    ui.label(RichText::new(v).monospace());
                    ui.end_row();
                }
            });
            ui.add_space(m.gap.y);
            match &rec.body {
                Some(body) => {
                    controls::section_label(
                        ui,
                        &format!(
                            "Body ({} bytes{})",
                            rec.body_bytes,
                            if rec.body_truncated { ", truncated" } else { "" }
                        ),
                    );
                    TextArea::new(&mut body.as_str()).mono(true).show(ui);
                }
                None => {
                    controls::status_label(ui, svg::INFO, "Body not captured — add this path prefix under Overview → Request capture.", theme::faint_text(ui));
                }
            }
        });
    }

    fn paths_ui(&mut self, ui: &mut Ui) {
        let m = Metrics::get(ui.ctx());
        if self.paths_overflow > 0 {
            ui.horizontal(|ui| {
                controls::status_label(ui, svg::WARNING, &format!(
                        "{} requests not counted — distinct path limit reached",
                        self.paths_overflow
                    ), theme::warn(ui));
            });
        }
        if self.paths.is_empty() {
            controls::empty_state(ui, svg::TREE_STRUCTURE, "No paths counted yet", "", &[]);
            return;
        }
        ScrollArea::vertical().show(ui, |ui| {
            TableBuilder::new(ui)
                .id_salt("paths_table")
                .striped(true)
                .resizable(true)
                .cell_layout(Layout::left_to_right(Align::Center))
                .column(Column::initial(300.0).at_least(180.0))
                .column(Column::auto().at_least(60.0))
                .column(Column::auto().at_least(140.0))
                .column(Column::auto().at_least(80.0))
                .column(Column::auto().at_least(80.0))
                .column(Column::remainder().at_least(170.0))
                .header(m.row_h, |mut h| {
                    for title in ["Method + path", "Count", "Statuses", "Avg ms", "Max ms", "Last seen"] {
                        h.col(|ui| {
                            ui.label(RichText::new(title).strong());
                        });
                    }
                })
                .body(|mut body| {
                    for p in &self.paths {
                        body.row(m.row_h, |mut r| {
                            r.col(|ui| {
                                ui.label(&p.key);
                            });
                            r.col(|ui| {
                                ui.label(p.count.to_string());
                            });
                            r.col(|ui| {
                                let s = p
                                    .statuses
                                    .iter()
                                    .map(|(k, v)| format!("{k}×{v}"))
                                    .collect::<Vec<_>>()
                                    .join(" ");
                                ui.label(s);
                            });
                            r.col(|ui| {
                                let avg = if p.count > 0 {
                                    p.latency_ms_total / p.count as f64
                                } else {
                                    0.0
                                };
                                ui.label(format!("{avg:.1}"));
                            });
                            r.col(|ui| {
                                ui.label(format!("{:.1}", p.latency_ms_max));
                            });
                            r.col(|ui| {
                                ui.label(&p.last_at);
                            });
                        });
                    }
                });
        });
    }
}

fn opt(v: &Option<String>) -> String {
    v.clone().unwrap_or_else(|| "—".to_string())
}

/// Success-tinted check for `true`, a dash otherwise.
fn yes_no(ui: &mut Ui, v: bool) {
    if v {
        let size = Metrics::get(ui.ctx()).icon_sm;
        ui.add(svg::CHECK.image(size).tint(theme::success(ui)));
    } else {
        ui.label("—");
    }
}

fn status_color(ui: &Ui, status: u16) -> Color32 {
    match status {
        200..=299 => theme::info(ui),
        300..=399 => theme::weak_text(ui),
        400..=499 => theme::warn(ui),
        _ => theme::error(ui),
    }
}

fn human_secs(secs: u64) -> String {
    let (d, h, m, s) = (secs / 86_400, (secs % 86_400) / 3600, (secs % 3600) / 60, secs % 60);
    if d > 0 {
        format!("{d}d {h}h {m}m")
    } else if h > 0 {
        format!("{h}h {m}m {s}s")
    } else {
        format!("{m}m {s}s")
    }
}

/// Percent-encode everything outside the unreserved set.
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
