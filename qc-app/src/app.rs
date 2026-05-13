use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;
use egui::containers::menu::MenuConfig;
use egui::{Align, Layout, PopupCloseBehavior};

use crate::db;
use crate::hw_sampler::HwSampler;
use crate::hw_table::HwTable;
use crate::mcp::{QcMcpState, spawn_mcp_servers};
use crate::oa3_sager::{self, H2oGeneration};
use crate::reporting::ReportSink;
use crate::stress_panel::{StressPanel, StressPanelConfig};
use crate::telemetry::{Heartbeat, HwSnapshot, QcReport};

fn init_arc_mutex_string() -> Arc<Mutex<String>> {
    Arc::new(Mutex::new(String::new()))
}

fn init_arc_atomic_bool() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

fn init_hw_table() -> Arc<Mutex<HwTable>> {
    Arc::new(Mutex::new(HwTable::new()))
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
    pub selected_tab: u8,
    /// Persisted stress panel config.
    pub stress_cfg: StressPanelConfig,
    /// Fleet orchestrator base URL; empty disables HTTP reporting.
    pub orchestrator_url: String,

    #[serde(skip, default = "init_arc_mutex_string")]
    status_line: Arc<Mutex<String>>,
    #[serde(skip, default = "init_arc_atomic_bool")]
    github_in_flight: Arc<AtomicBool>,
    #[serde(skip, default = "init_arc_mutex_string")]
    db_line: Arc<Mutex<String>>,
    #[serde(skip)]
    stress_panel: StressPanel,
    /// Undocked hardware monitor window visibility.
    #[serde(skip, default = "init_arc_atomic_bool")]
    show_hw_monitor: Arc<AtomicBool>,
    /// Shared CPU table: sampler writes, HW monitor reads.
    #[serde(skip, default = "init_hw_table")]
    hw_table: Arc<Mutex<HwTable>>,
    /// Background sampler; created on first frame.
    #[serde(skip)]
    hw_sampler: Option<HwSampler>,
    /// Orchestrator HTTP sink; recreated when URL changes.
    #[serde(skip)]
    report_sink: Option<ReportSink>,
    /// Last heartbeat send time (30 s throttle).
    #[serde(skip)]
    last_heartbeat: Option<Instant>,
    /// State exposed to MCP tools.
    #[serde(skip)]
    mcp_state: Option<Arc<QcMcpState>>,
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
            selected_tab: 0,
            stress_cfg: StressPanelConfig::default(),
            orchestrator_url: String::new(),
            status_line: init_arc_mutex_string(),
            github_in_flight: init_arc_atomic_bool(),
            db_line: init_arc_mutex_string(),
            stress_panel: StressPanel::default(),
            show_hw_monitor: init_arc_atomic_bool(),
            hw_table: init_hw_table(),
            hw_sampler: None,
            report_sink: None,
            last_heartbeat: None,
            mcp_state: None,
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
        ui.label("Fleet orchestrator URL");
        ui.label(
            egui::RichText::new(
                "Leave empty to disable network reporting.  Example: http://192.168.1.50:7700",
            )
            .small()
            .weak(),
        );
        let changed = ui.text_edit_singleline(&mut self.orchestrator_url).changed();
        if changed {
            self.report_sink = None;
            self.last_heartbeat = None;
        }
        ui.add_space(8.0);
        let status = if self.orchestrator_url.is_empty() {
            "Reporting disabled (no URL)".to_string()
        } else if self.report_sink.is_some() {
            format!("Reporting active → {}", self.orchestrator_url)
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

        // Copy sampler snapshot into `hw_table` for the monitor window.
        let current_cores = if let Some(ref sampler) = self.hw_sampler {
            let snapshot = sampler.snapshot();
            let rows = snapshot.cores.clone();
            if let Ok(mut table) = self.hw_table.lock() {
                table.update(snapshot);
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

        if self.report_sink.is_none() && !self.orchestrator_url.is_empty() {
            let mid = crate::reporting::machine_id();
            self.report_sink = Some(ReportSink::start(
                Some(self.orchestrator_url.clone()),
                mid,
            ));
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

        self.stress_panel.tick(ctx);

        // Undocked HW monitor: clone `Arc`s so the viewport closure does not capture `self`.
        if self.show_hw_monitor.load(Ordering::Relaxed) {
            let hw_table = Arc::clone(&self.hw_table);
            let show_hw_monitor = Arc::clone(&self.show_hw_monitor);

            let viewport_id = egui::ViewportId::from_hash_of("qc_hw_monitor");
            let viewport_builder = egui::ViewportBuilder::default()
                .with_title("Hardware Monitor")
                .with_inner_size([820.0, 480.0]);

            #[allow(deprecated)]
            ctx.show_viewport_immediate(
                viewport_id,
                viewport_builder,
                move |ctx, _class| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        if let Ok(mut table) = hw_table.lock() {
                            table.show(ui);
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
        eframe::egui::Panel::top("egui_dock::MenuBar").show_inside(ui, |ui| {
            eframe::egui::MenuBar::new()
            .config(
                MenuConfig::default().close_behavior(PopupCloseBehavior::CloseOnClickOutside),
            )
            .ui(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.selected_tab, 0u8, "Swift DB");
                    ui.separator();
                    ui.selectable_value(&mut self.selected_tab, 1u8, "OA3 Sager");
                    ui.separator();
                    ui.selectable_value(&mut self.selected_tab, 2u8, "Stress Test");
                    ui.separator();
                    ui.selectable_value(&mut self.selected_tab, 3u8, "Settings");

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(format!(" Mastertech QC - v{}", env!("CARGO_PKG_VERSION")));
                    });
                });
            });
        });
        egui::CentralPanel::default().show_inside(ui, |ui| {
            match self.selected_tab {
                1 => self.ui_database(ui),
                2 => self.ui_oa3(ui),
                4 => self.ui_settings(ui),
                _ => {
                    let mut open_hw = false;
                    self.stress_panel.ui(ui, &mut self.stress_cfg, &mut open_hw);
                    if open_hw {
                        self.show_hw_monitor.store(true, Ordering::Relaxed);
                    }
                }
            }
        });
    }


    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }
}
