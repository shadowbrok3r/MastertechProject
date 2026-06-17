use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;
use egui::containers::menu::MenuConfig;
use egui::{Align, Layout, PopupCloseBehavior};
use egui_dock::{DockArea, DockState};

use crate::db;
use crate::fleet_client::{FleetClient, InboundCommandKind};
use crate::hw_monitor::HwMonitor;
use crate::hw_sampler::HwSampler;
use crate::mcp::{QcMcpState, spawn_mcp_servers};
use crate::oa3_sager::{self, H2oGeneration};
use crate::order_panel::OrderPanel;
use crate::reporting::ReportSink;
use crate::stress_panel::{StressPanel, StressPanelConfig};
use crate::telemetry::{Heartbeat, HwSnapshot, QcReport};

/// Stable `computer:<machine_id>` record for stress runs originating on this
/// machine. Cached for the process lifetime: the inputs (hostname + CPU brand)
/// don't change, and the disk read inside `machine_id()` was previously firing
/// on every UI frame. SurrealDB doesn't enforce FK existence, so the run row
/// carries the link either way; if the customer's computer record gets
/// re-keyed later, the `connected_client`-style hostname re-link query repairs
/// the reference.
fn local_computer_record() -> database::schema::RecordId {
    use std::sync::OnceLock;
    static CACHED: OnceLock<database::schema::RecordId> = OnceLock::new();
    CACHED
        .get_or_init(|| {
            let (hostname, cpu) = crate::reporting::host_name_and_cpu_brand();
            let key = stress_runner::computer_record_key(&hostname, &cpu);
            log::info!("qc-app: local computer record cached as computer:{key}");
            database::schema::RecordId::new(database::schema::COMPUTER_TABLE, key)
        })
        .clone()
}

fn init_arc_mutex_string() -> Arc<Mutex<String>> {
    Arc::new(Mutex::new(String::new()))
}

fn init_arc_atomic_bool() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

fn init_hw_monitor() -> Arc<Mutex<HwMonitor>> {
    Arc::new(Mutex::new(HwMonitor::default()))
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, serde::Serialize, serde::Deserialize)]
pub enum QcTab {
    OrderQc,
    SwiftDb,
    Oa3,
    Stress,
    Settings,
    Logs,
    BugReport,
    HardwareMonitor,
}

impl QcTab {
    const ALL: [QcTab; 8] = [
        QcTab::OrderQc,
        QcTab::SwiftDb,
        QcTab::Oa3,
        QcTab::Stress,
        QcTab::Settings,
        QcTab::Logs,
        QcTab::BugReport,
        QcTab::HardwareMonitor,
    ];

    fn title(self) -> &'static str {
        match self {
            QcTab::OrderQc => "Order QC",
            QcTab::SwiftDb => "Swift DB",
            QcTab::Oa3 => "OA3 Sager",
            QcTab::Stress => "Stress Test",
            QcTab::Settings => "Settings",
            QcTab::Logs => "Logs",
            QcTab::BugReport => "Bug Report",
            QcTab::HardwareMonitor => "Hardware",
        }
    }
}

fn default_dock() -> DockState<QcTab> {
    DockState::new(QcTab::ALL.to_vec())
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct QcApp {
    pub github_owner: String,
    pub github_repo: String,
    pub github_ref: String,
    /// Swift driver catalog SQLite path.
    pub sqlite_path: String,
    /// Root containing `H2O14\`, `H2O12\`, etc. for `H2OOAE-Wx64.exe`.
    pub oa3_wrapper_path: String,
    /// `.bin` passed to H2OOAE `-W` in inject preview.
    pub oa3_bin_path: String,
    pub h2o_generation: H2oGeneration,
    #[serde(default = "default_dock")]
    dock: DockState<QcTab>,
    /// Persisted stress panel config.
    pub stress_cfg: StressPanelConfig,

    #[serde(skip, default = "init_arc_mutex_string")]
    status_line: Arc<Mutex<String>>,
    #[serde(skip, default = "init_arc_atomic_bool")]
    github_in_flight: Arc<AtomicBool>,
    #[serde(skip, default = "init_arc_mutex_string")]
    db_line: Arc<Mutex<String>>,
    #[serde(skip)]
    stress_panel: StressPanel,
    /// Order QC tab (lookup, gate, spec check, sign-off, comments, report).
    #[serde(skip)]
    order_panel: OrderPanel,
    /// Undocked hardware monitor window visibility.
    #[serde(skip, default = "init_arc_atomic_bool")]
    show_hw_monitor: Arc<AtomicBool>,
    /// Shared multi-view monitor: sampler writes a snapshot here each frame,
    /// the undocked viewport reads it.
    #[serde(skip, default = "init_hw_monitor")]
    hw_monitor: Arc<Mutex<HwMonitor>>,
    /// Background sampler; created on first frame.
    #[serde(skip)]
    hw_sampler: Option<HwSampler>,
    /// Orchestrator HTTP sink; recreated when URL changes.
    #[serde(skip)]
    report_sink: Option<ReportSink>,
    /// Inbound fleet command client; recreated when URL changes.
    #[serde(skip)]
    fleet_client: Option<FleetClient>,
    /// Last heartbeat send time (30 s throttle).
    #[serde(skip)]
    last_heartbeat: Option<Instant>,
    /// State exposed to MCP tools.
    #[serde(skip)]
    mcp_state: Option<Arc<QcMcpState>>,
    /// Run report window (fresh verdicts + past runs).
    #[serde(skip)]
    report_view: crate::report_view::ReportView,
    #[serde(skip)]
    bug_report: crate::bug_report::BugReportPanel,
}

impl Default for QcApp {
    fn default() -> Self {
        Self {
            github_owner: "MacabreMage".into(),
            github_repo: "ConnecteamPythonBot".into(),
            github_ref: "main".into(),
            sqlite_path: db::default_sqlite_path().to_string_lossy().into_owned(),
            oa3_wrapper_path: String::new(),
            oa3_bin_path: String::new(),
            h2o_generation: H2oGeneration::H2O14,
            dock: default_dock(),
            stress_cfg: StressPanelConfig::default(),
            status_line: init_arc_mutex_string(),
            github_in_flight: init_arc_atomic_bool(),
            db_line: init_arc_mutex_string(),
            stress_panel: StressPanel::default(),
            order_panel: OrderPanel::default(),
            show_hw_monitor: init_arc_atomic_bool(),
            hw_monitor: init_hw_monitor(),
            hw_sampler: None,
            report_sink: None,
            fleet_client: None,
            last_heartbeat: None,
            mcp_state: None,
            report_view: crate::report_view::ReportView::default(),
            bug_report: crate::bug_report::BugReportPanel::default(),
        }
    }
}

impl QcApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        if let Some(storage) = cc.storage {
            if let Some(app) = eframe::get_value(storage, eframe::APP_KEY) {
                return app;
            }
        }
        Self::default()
    }

    /// Apply one inbound fleet command and ack it. The fleet_client handle
    /// is passed in (rather than read off `self`) so the caller can use
    /// `&mut self` for the work without re-locking the client option.
    fn dispatch_inbound_command(
        &mut self,
        cmd: crate::fleet_client::InboundCommand,
        client: &FleetClient,
        current_cores: &[crate::hw_sampler::CoreRow],
    ) {
        let id = cmd.id.clone();
        log::info!("qc-app: dispatching fleet command {id} ({:?})", cmd.kind);
        match cmd.kind {
            InboundCommandKind::SendReport => {
                let snapshot = HwSnapshot::from_cores(current_cores);
                let mid = client.machine_id.as_ref().clone();
                let report = QcReport::new(&mid, snapshot);
                if let Some(sink) = self.report_sink.as_ref() {
                    sink.send_report(report.clone());
                }
                if let Some(state) = self.mcp_state.as_ref() {
                    if let Ok(mut g) = state.last_report.lock() {
                        *g = Some(report);
                    }
                }
                client.ack(id);
            }
            InboundCommandKind::Custom { payload } => {
                match payload.get("op").and_then(|v| v.as_str()) {
                    Some("run_stress_preset") => {
                        let preset = payload
                            .get("preset")
                            .and_then(|v| v.as_str())
                            .unwrap_or("bronze")
                            .to_string();
                        let mult = payload
                            .get("duration_multiplier")
                            .and_then(|v| v.as_f64())
                            .unwrap_or(1.0) as f32;
                        match self.hw_sampler.as_ref().map(|s| s.agent()) {
                            Some(telemetry) => {
                                match self.stress_panel.start_certification_by_name(
                                    &preset,
                                    mult,
                                    telemetry,
                                    local_computer_record(),
                                ) {
                                    Ok(()) => log::info!(
                                        "qc-app: fleet command {id} started preset '{preset}' at {mult}x"
                                    ),
                                    Err(err) => log::warn!(
                                        "qc-app: fleet command {id} preset '{preset}' refused: {err}"
                                    ),
                                }
                            }
                            None => log::warn!(
                                "qc-app: fleet command {id} refused — telemetry sampler not ready"
                            ),
                        }
                    }
                    Some("cancel_stress_run") => {
                        self.stress_panel.stop_active_run();
                        log::info!("qc-app: fleet command {id} cancelled the active run");
                    }
                    other => {
                        log::warn!(
                            "qc-app: fleet command {id} unhandled custom op {other:?}; payload={payload}"
                        );
                    }
                }
                client.ack(id);
            }
        }
    }

    fn set_status(&self, msg: impl Into<String>) {
        if let Ok(mut g) = self.status_line.lock() {
            *g = msg.into();
        }
    }

    fn ui_database(&mut self, ui: &mut egui::Ui) {
        ui.heading("Swift driver DB (local SQLite)");
        ui.label("Schema from `db_creation_script.sql` (MySQL), adapted for SQLite.");
        ui.horizontal(|ui| {
            ui.label("Database file");
            ui.text_edit_singleline(&mut self.sqlite_path);
        });

        if ui.button("Open / create & migrate").clicked() {
            let path = Path::new(&self.sqlite_path);
            match db::open_or_create(path).and_then(|c| db::table_stats(&c)) {
                Ok(rows) => {
                    let summary = rows
                        .iter()
                        .map(|(n, c)| format!("{n}: {c}"))
                        .collect::<Vec<_>>()
                        .join("  ·  ");
                    if let Ok(mut g) = self.db_line.lock() {
                        *g = summary;
                    }
                }
                Err(e) => {
                    if let Ok(mut g) = self.db_line.lock() {
                        *g = format!("Error: {e:#}");
                    }
                }
            }
        }

        if let Ok(s) = self.db_line.lock() {
            if !s.is_empty() {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(s.as_str()).monospace().size(12.0));
            }
        }
    }

    fn ui_settings(&mut self, ui: &mut egui::Ui) {
        ui.heading("Settings");
        ui.add_space(8.0);

        ui.label(egui::RichText::new("Fleet orchestrator").strong());
        ui.label(
            egui::RichText::new(
                "Picked from .env at compile time. Rebuild after editing \
                 ORCHESTRATOR_URL / ORCHESTRATOR_URL_DEV to change.",
            )
            .small()
            .weak(),
        );
        ui.add_space(6.0);

        let active_url = database::orchestrator_url();
        let active_label = if cfg!(debug_assertions) {
            "ORCHESTRATOR_URL_DEV (debug build)"
        } else {
            "ORCHESTRATOR_URL (release build)"
        };
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Active key:").monospace().small().weak());
            ui.label(egui::RichText::new(active_label).monospace().small());
        });
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Resolved URL:").monospace().small().weak());
            if active_url.is_empty() {
                ui.colored_label(
                    egui::Color32::from_rgb(180, 140, 50),
                    egui::RichText::new("(empty — reporting disabled)").monospace().small(),
                );
            } else {
                ui.label(egui::RichText::new(active_url).monospace().small());
            }
        });

        ui.add_space(8.0);
        let status = if active_url.is_empty() {
            "Reporting disabled (env var unset)".to_string()
        } else if self.report_sink.is_some() {
            format!("Reporting active → {active_url}")
        } else {
            "Reporting will start on next frame".to_string()
        };
        ui.label(egui::RichText::new(status).monospace().small());
    }

    fn ui_oa3(&mut self, ui: &mut egui::Ui) {
        ui.heading("OA3 — Sager H2O helper");
        ui.label("Command preview for Sager H2OOAE + `oa3tool` (paths from fields above).");
        ui.horizontal(|ui| {
            ui.label("Wrapper root");
            ui.text_edit_singleline(&mut self.oa3_wrapper_path);
        });
        ui.horizontal(|ui| {
            ui.label("OA3 .bin for inject");
            ui.text_edit_singleline(&mut self.oa3_bin_path);
        });

        egui::ComboBox::from_id_salt("h2o_gen")
            .selected_text(self.h2o_generation.label())
            .show_ui(ui, |ui| {
                for g in H2oGeneration::all() {
                    ui.selectable_value(&mut self.h2o_generation, *g, g.label());
                }
            });

        let wrapper = Path::new(&self.oa3_wrapper_path);
        let bin = Path::new(&self.oa3_bin_path);
        let exe = oa3_sager::h2ooae_exe(wrapper, self.h2o_generation);

        ui.add_space(8.0);
        ui.label(egui::RichText::new("Resolved H2OOAE").strong());
        ui.label(exe.display().to_string());

        if wrapper.as_os_str().is_empty() || bin.as_os_str().is_empty() {
            ui.colored_label(egui::Color32::YELLOW, "Set wrapper root and OA3 .bin path for command preview.");
            return;
        }

        let inject = oa3_sager::inject_command_line(self.h2o_generation, wrapper, bin);
        let clear = oa3_sager::clear_command_line(self.h2o_generation, wrapper);

        ui.add_space(8.0);
        ui.collapsing("Inject (preview)", |ui| {
            let mut inject_buf = inject.clone();
            ui.add(
                egui::TextEdit::multiline(&mut inject_buf)
                    .desired_rows(6)
                    .interactive(false),
            );
            if ui.button("Copy inject preview").clicked() {
                ui.ctx().copy_text(inject);
            }
        });
        ui.collapsing("Clear (preview)", |ui| {
            let mut clear_buf = clear.clone();
            ui.add(
                egui::TextEdit::multiline(&mut clear_buf)
                    .desired_rows(8)
                    .interactive(false),
            );
            if ui.button("Copy clear preview").clicked() {
                ui.ctx().copy_text(clear);
            }
        });
    }

    fn ui_logs(ui: &mut egui::Ui) {
        mtech_ui::egui_logger::logger_ui()
            .log_levels([true, true, true, false, false])
            .enable_category("eframe".to_string(), false)
            .enable_category("eframe::native::glow_integration".to_string(), false)
            .enable_category("egui_glow::shader_version".to_string(), false)
            .enable_category("egui_glow::painter".to_string(), false)
            .enable_category("evtx::evtx_chunk".to_string(), false)
            .enable_category("evtx::evtx_parser".to_string(), false)
            .show(ui);
    }
}

impl eframe::App for QcApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.github_in_flight.load(Ordering::Relaxed) {
            ctx.request_repaint_after(Duration::from_millis(150));
        }

        // Sampler: first frame
        if self.hw_sampler.is_none() {
            self.hw_sampler = Some(HwSampler::start(1000));
        }

        // Copy sampler snapshot into the shared HwMonitor so the undocked
        // viewport always sees the latest tick, and feed the stress panel's
        // chart board so the Panel::bottom plots stay populated even when no
        // stress run is active. `current_cores` is also kept for the
        // heartbeat path below.
        let current_cores = if let Some(ref sampler) = self.hw_sampler {
            let snapshot = sampler.snapshot();
            let rows = snapshot.cores.clone();
            self.stress_panel.push_telemetry(&snapshot);
            if let Ok(mut monitor) = self.hw_monitor.lock() {
                monitor.update(snapshot);
            }
            rows
        } else {
            vec![]
        };

        // MCP state + listeners: first frame
        if self.mcp_state.is_none() {
            let state = Arc::new(QcMcpState {
                latest_cores: Arc::new(Mutex::new(vec![])),
                last_report: Arc::new(Mutex::new(None)),
                report_sink: Arc::new(Mutex::new(None)),
                telemetry: Arc::new(Mutex::new(None)),
                computer: local_computer_record(),
                run_slot: Arc::new(Mutex::new(crate::mcp::RunSlot::default())),
            });
            spawn_mcp_servers(state.clone());
            self.mcp_state = Some(state);
        }

        if let Some(ref state) = self.mcp_state {
            if let Ok(mut g) = state.latest_cores.lock() {
                *g = current_cores.clone();
            }
            if let Ok(mut g) = state.report_sink.lock() {
                *g = self.report_sink.clone();
            }
            if let Some(ref sampler) = self.hw_sampler {
                if let Ok(mut g) = state.telemetry.lock() {
                    if g.is_none() {
                        *g = Some(sampler.agent());
                    }
                }
            }
        }

        if self.report_sink.is_none() {
            let url = database::orchestrator_url();
            if !url.is_empty() {
                let mid = crate::reporting::machine_id();
                self.report_sink = Some(ReportSink::start(Some(url.to_string()), mid.clone()));
                // Twin-spawn the inbound command client. It auto-registers and
                // polls /commands; URL is compile-time so no reconfig path.
                self.fleet_client = Some(FleetClient::start(Some(url.to_string()), mid));
                log::info!("qc-app: fleet client + report sink wired to {url}");
            }
        }

        // Drain any orchestrator commands that landed since the last frame.
        if let Some(client) = self.fleet_client.clone() {
            for cmd in client.drain_commands(8) {
                self.dispatch_inbound_command(cmd, &client, &current_cores);
            }
        }

        const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
        if let Some(ref sink) = self.report_sink {
            let should_send = self
                .last_heartbeat
                .map(|t| t.elapsed() >= HEARTBEAT_INTERVAL)
                .unwrap_or(true);
            if should_send {
                let avg_pct = if current_cores.is_empty() {
                    0.0
                } else {
                    current_cores.iter().map(|c| c.usage_pct).sum::<f32>()
                        / current_cores.len() as f32
                };
                sink.send_heartbeat(Heartbeat::new(sink.machine_id.as_str(), avg_pct));
                self.last_heartbeat = Some(Instant::now());
            }
        }

        // New stress runs link to the active order session and signed-in tech.
        self.stress_panel.set_order_context(self.order_panel.run_context());

        self.stress_panel.tick(ctx);

        if let Some(run_id) = self.stress_panel.take_report_request() {
            self.report_view.open_run(run_id, ctx);
        }
        self.report_view.show(ctx, &local_computer_record());

        // Undocked HW monitor: clone `Arc`s so the viewport closure does not capture `self`.
        if self.show_hw_monitor.load(Ordering::Relaxed) {
            let hw_monitor = Arc::clone(&self.hw_monitor);
            let show_hw_monitor = Arc::clone(&self.show_hw_monitor);

            let viewport_id = egui::ViewportId::from_hash_of("qc_hw_monitor");
            let viewport_builder = egui::ViewportBuilder::default()
                .with_title("Hardware Monitor")
                .with_inner_size([960.0, 620.0]);

            #[allow(deprecated)]
            ctx.show_viewport_immediate(
                viewport_id,
                viewport_builder,
                move |ctx, _class| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        if let Ok(mut monitor) = hw_monitor.lock() {
                            monitor.show(ui);
                        }
                    });
                    if ctx.input(|i| i.viewport().close_requested()) {
                        show_hw_monitor.store(false, Ordering::Relaxed);
                    }
                },
            );
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let mut tree = std::mem::replace(&mut self.dock, DockState::new(Vec::new()));

        egui::Panel::top("qc_menu_bar").show_inside(ui, |ui| {
            egui::MenuBar::new()
                .config(
                    MenuConfig::default().close_behavior(PopupCloseBehavior::CloseOnClickOutside),
                )
                .ui(ui, |ui| {
                    ui.menu_button("View", |ui| {
                        for tab in QcTab::ALL {
                            let open = tree.find_tab(&tab).is_some();
                            if ui.selectable_label(open, tab.title()).clicked() {
                                match tree.find_tab(&tab) {
                                    Some(idx) => {
                                        tree.remove_tab(idx);
                                    }
                                    None => tree.push_to_focused_leaf(tab),
                                }
                            }
                        }
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(format!(" Mastertech QC - {}", database::version_with_build!()));
                    });
                });
        });

        let style = mtech_ui::dock_style::style(ui.ctx());
        DockArea::new(&mut tree)
            .style(style)
            .show_close_buttons(true)
            .draggable_tabs(true)
            .show_inside(ui, self);

        self.dock = tree;
    }


    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }
}

impl egui_dock::TabViewer for QcApp {
    type Tab = QcTab;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        tab.title().into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        match tab {
            QcTab::SwiftDb => self.ui_database(ui),
            QcTab::Oa3 => self.ui_oa3(ui),
            QcTab::Settings => self.ui_settings(ui),
            QcTab::Logs => Self::ui_logs(ui),
            QcTab::BugReport => self.bug_report.ui(ui),
            QcTab::HardwareMonitor => {
                let undocked = self.show_hw_monitor.load(Ordering::Relaxed);
                ui.horizontal(|ui| {
                    if ui
                        .selectable_label(undocked, "Undock to window")
                        .on_hover_text("Show the hardware monitor in its own resizable window")
                        .clicked()
                    {
                        self.show_hw_monitor.store(!undocked, Ordering::Relaxed);
                    }
                });
                ui.separator();
                if undocked {
                    ui.add_space(8.0);
                    ui.colored_label(
                        egui::Color32::GRAY,
                        "Hardware monitor is shown in a separate window — untoggle \
                         \"Undock to window\" or close that window to dock it here.",
                    );
                } else if let Ok(mut monitor) = self.hw_monitor.lock() {
                    monitor.show(ui);
                }
            }
            QcTab::OrderQc => {
                let snapshot = self.hw_sampler.as_ref().map(|s| s.agent().snapshot());
                let last_verdict = self.stress_panel.last_verdict_ref().cloned();
                let last_preset = self.stress_panel.last_preset();
                self.order_panel
                    .ui(ui, snapshot.as_ref(), last_verdict.as_ref(), last_preset);
            }
            QcTab::Stress => {
                let mut open_hw = false;
                let telemetry = self.hw_sampler.as_ref().map(|s| s.agent());
                if let Some(telemetry) = telemetry {
                    let computer = local_computer_record();
                    self.stress_panel
                        .ui(ui, &mut self.stress_cfg, &mut open_hw, telemetry, computer);
                    if open_hw {
                        self.show_hw_monitor.store(true, Ordering::Relaxed);
                    }
                } else {
                    ui.colored_label(
                        egui::Color32::from_rgb(180, 140, 50),
                        "Telemetry sampler not initialized — open the Hardware Monitor first.",
                    );
                }
            }
        }
    }
}
