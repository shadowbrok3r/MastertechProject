
use ratatui::widgets::{ScrollbarState, TableState};
use crate::filesystem::system_info::get_sysinfo_no_gpu; // fx::{effect::UniqueEffectId, EffectStage}
use std::{collections::HashMap, time::Instant};
use crossbeam::channel::{Receiver, Sender};
use database::schema::{Process, SystemInformation};

pub mod action_handler;
pub mod render;

pub struct SysinfoTab {
    system: SystemInformation,
    first_run: bool,
    process_scroll_state: ScrollbarState,
    process_table_state: TableState,
    cpu_history: Vec<Sample>,
    mem_history: Vec<Sample>,
    gpu_history: Vec<Sample>,
    processes: Vec<Process>,
    component_temp_history: HashMap<String, Vec<Sample>>,

    tx: Sender<SystemInformation>,
    rx: Receiver<SystemInformation>,
    stop_rx: tokio::sync::broadcast::Receiver<()>,
    stop_tx: tokio::sync::broadcast::Sender<()>,
    start_time: Instant,
    // pub effect_stage: EffectStage<UniqueEffectId>,
}

/// A sample that records the elapsed time (in seconds) and the value.
#[derive(Debug)]
struct Sample {
    time: f64,  // seconds since start
    value: f64,
}

impl SysinfoTab {
    pub fn new() -> Self {
        let (tx, rx) = crossbeam::channel::unbounded();
        let (stop_tx, stop_rx) = tokio::sync::broadcast::channel(1);
        Self {
            system: Default::default(),
            first_run: true,
            process_table_state: TableState::default(),
            process_scroll_state: ScrollbarState::default(),

            cpu_history: Vec::new(),
            mem_history: Vec::new(),
            gpu_history: Vec::new(),
            processes: Vec::new(),
            component_temp_history: HashMap::new(),

            start_time: Instant::now(),
            // effect_stage: EffectStage::default(),

            tx, rx,
            stop_tx, stop_rx,
        }
    }

    fn get_sysinfo(&mut self, mut shutdown_rx: tokio::sync::broadcast::Receiver<()>) {
        let tx = self.tx.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown_rx.recv() => {
                        log::warn!("Received shutdown signal, restarting sysinfo task");
                        break;
                    }
                    _ = tokio::time::sleep(std::time::Duration::from_secs_f32(0.2)) => {
                        let _ = tx.try_send(get_sysinfo_no_gpu().await.unwrap_or_default());
                    }
                }
            }
        });
    }

    /// Call this on every update (or in your draw loop) to record the latest value.
    fn update_history(&mut self, system: SystemInformation) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.start_time).as_secs_f64();

        self.system = system;

        self.processes = self.system.processes.clone();
        
        // CPU history
        self.cpu_history.push(Sample {
            time: elapsed,
            value: self.system.cpu_percentage as f64,
        });

        // Memory history
        let mem_percent = if self.system.total_memory > 0.0 {
            self.system.used_memory / self.system.total_memory * 100.0
        } else {
            0.0
        };

        self.mem_history.push(Sample {
            time: elapsed,
            value: mem_percent as f64,
        });

        // GPU history
        let gpu_percent = self.system
            .gpu_info
            .usage
            .get(0)
            .map(|u| u.gpu as f64)
            .unwrap_or(0.0);

        self.gpu_history.push(Sample {
            time: elapsed,
            value: gpu_percent,
        });

        // log::info!("self.system.component_temps: {:?}", self.system.component_temps);
        // Component temperatures: update history for each component.
        for (comp, &temp) in self.system.component_temps.iter() {
            self.component_temp_history
                .entry(comp.clone())
                .or_insert_with(Vec::new)
                .push(Sample {
                    time: elapsed,
                    value: temp as f64,
                });
        }

        log::info!("self.component_temp_history: {:?}", self.component_temp_history.len());
        // (Optionally, trim histories if they exceed a desired maximum length.)
    }
}