//! Renders the shared stress dashboard with synthetic data.
//!
//! `cargo run -p mtech-ui --features stress-ui --example stress_dashboard`
//!
//! Exercises the layout without a stress run or an elevated host, so the
//! columns, grouped picker, hover hints, and help page can be checked directly.

use eframe::egui;
use mtech_ui::stress_dashboard::{
    DashboardAction, LaneView, RecentRun, StageProgress, StageVerdictView, StressDashboard,
    StressLive, VerdictView,
};
use stress_runner::{
    PanelMode, RunResult, ScenarioStageConfig, StressPanelConfig, StressorChoice,
};

struct Demo {
    dash: StressDashboard,
    cfg: StressPanelConfig,
    live: StressLive,
    running: bool,
    frame: u64,
}

impl Default for Demo {
    fn default() -> Self {
        let mut cfg = StressPanelConfig::default();
        // MTECH_DASHBOARD_MODE=concurrent|scenario|cert|qc opens straight into that mode.
        match std::env::var("MTECH_DASHBOARD_MODE").unwrap_or_default().as_str() {
            "concurrent" => cfg.mode = PanelMode::Concurrent,
            "scenario" => {
                cfg.mode = PanelMode::Scenario;
                cfg.scenario.stages.push(ScenarioStageConfig::default_disk());
            }
            "cert" => cfg.mode = PanelMode::Certification,
            "qc" => cfg.mode = PanelMode::QcBenchmark,
            _ => {}
        }
        let mut dash = StressDashboard::default();
        if std::env::var("MTECH_DASHBOARD_MODE").as_deref() == Ok("help") {
            dash.open_help();
        }
        Self {
            dash,
            cfg,
            live: sample_live(),
            running: false,
            frame: 0,
        }
    }
}

fn sample_live() -> StressLive {
    StressLive {
        elapsed_secs: 137.0,
        throughput: 4821.6,
        throughput_unit: "GFLOPS",
        last_error: None,
        stage: Some(StageProgress {
            index: 2,
            label: "fp".into(),
            count: 8,
        }),
        lanes: vec![
            LaneView {
                index: 0,
                label: "CPU".into(),
                stressor: Some(StressorChoice::Cpu),
                throughput: 4821.6,
                unit: "Mop/s",
                errors: 0,
                last_error: None,
            },
            LaneView {
                index: 1,
                label: "Memory".into(),
                stressor: Some(StressorChoice::Memory),
                throughput: 2140.2,
                unit: "MiB/s",
                errors: 0,
                last_error: None,
            },
            LaneView {
                index: 2,
                label: "GPU Compute".into(),
                stressor: Some(StressorChoice::Gpu),
                throughput: 812.4,
                unit: "GFLOPS",
                errors: 2048,
                last_error: Some("GPU device failed: Device is lost".into()),
            },
        ],
        stage_verdicts: vec![
            StageVerdictView {
                label: "cpu".into(),
                pass: true,
                violations: vec![],
                peak_throughput: Some(5210.0),
            },
            StageVerdictView {
                label: "gpu_compute".into(),
                pass: false,
                violations: vec![
                    "throughput 812 GFLOPS below floor 100".into(),
                    "device lost before the stage completed".into(),
                ],
                peak_throughput: Some(812.4),
            },
        ],
        verdict: Some(VerdictView {
            result: RunResult::Fail,
            failure_kind: Some("data_mismatch".into()),
            duration_secs: 74.0,
            max_temp_c: Some(51.0),
            whea_delta: 0,
            tdr_count: 1,
            run_id: Some("stress_test_run:41c287c0".into()),
            planned_secs: Some(43_200),
        }),
        history: (0..90)
            .map(|i| 3000.0 + ((i as f32) * 0.7).sin() * 1200.0)
            .collect(),
        recent_runs: vec![
            RecentRun {
                label: "cert:platinum".into(),
                when: "today 09:14".into(),
                result: RunResult::Fail,
                duration_secs: 74.0,
            },
            RecentRun {
                label: "qc-benchmark".into(),
                when: "today 08:52".into(),
                result: RunResult::Pass,
                duration_secs: 162.0,
            },
        ],
    }
}

impl eframe::App for Demo {
    fn ui(&mut self, ui: &mut egui::Ui, _f: &mut eframe::Frame) {
        self.frame += 1;
        let action = self.dash.show(
            ui,
            &mut self.cfg,
            &self.live,
            self.running,
            None,
            |ui| {
                ui.label("(host telemetry charts render here)");
            },
        );
        match action {
            DashboardAction::Start => self.running = true,
            DashboardAction::Stop => self.running = false,
            DashboardAction::OpenHistory | DashboardAction::None => {}
        }

        // Headless smoke mode: paint every mode plus the help page, then exit.
        if std::env::var_os("MTECH_DASHBOARD_SMOKE").is_some() {
            let ctx = ui.ctx().clone();
            ctx.request_repaint();
            let modes = [
                PanelMode::Single,
                PanelMode::Scenario,
                PanelMode::QcBenchmark,
                PanelMode::Certification,
                PanelMode::Concurrent,
            ];
            // Three frames per mode, then three with help open.
            let step = (self.frame / 3) as usize;
            if step < modes.len() {
                self.cfg.mode = modes[step].clone();
                if self.cfg.mode == PanelMode::Scenario && self.cfg.scenario.stages.len() < 4 {
                    self.cfg.scenario.stages.push(ScenarioStageConfig::default_disk());
                    self.cfg.scenario.stages.push(ScenarioStageConfig::default_memory());
                }
                println!("rendered mode {:?}", self.cfg.mode);
            } else if step == modes.len() {
                if !self.dash.help_open() {
                    self.dash.open_help();
                }
                println!("rendered help page");
            } else {
                println!("all modes + help rendered cleanly over {} frames", self.frame);
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}

fn main() -> eframe::Result<()> {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1500.0, 900.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Stress Dashboard Preview",
        opts,
        Box::new(|cc| {
            let mut fonts = egui::FontDefinitions::default();
            mtech_ui::icons::install_fonts(&mut fonts);
            cc.egui_ctx.set_fonts(fonts);
            Ok(Box::new(Demo::default()))
        }),
    )
}
