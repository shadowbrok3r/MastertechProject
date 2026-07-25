//! Ordered [`ScenarioStage`] list: each stage runs one [`StressConfig`] for `duration_secs`.
//! Optional `total_wall_secs` + `repeat_until_total` re-run the list until the wall cap.
//! In stages, [`StressConfig::timeout`] is ignored; only `duration_secs` applies.
//! Supervisor sends [`ScenarioEvent`]s; UI drains with [`ScenarioRunner::try_recv_all`].

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::{Metrics, StressConfig};
use crate::stressors;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioStage {
    pub label: String,
    /// `timeout` ignored; use `duration_secs`.
    pub config: StressConfig,
    /// Stage length in seconds (minimum 1 enforced in supervisor).
    pub duration_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioDefinition {
    pub stages: Vec<ScenarioStage>,
    pub total_wall_secs: Option<u64>,
    /// With `total_wall_secs: Some`, loop stages until wall time is reached.
    pub repeat_until_total: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Completed,
    Cancelled,
    TotalTime,
}

impl FinishReason {
    pub fn label(self) -> &'static str {
        match self {
            Self::Completed => "Completed",
            Self::Cancelled => "Cancelled",
            Self::TotalTime => "Time limit reached",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScenarioEvent {
    StageStarted {
        index: usize,
        label: String,
        stage_count: usize,
    },
    Tick {
        stage_index: usize,
        metrics: Metrics,
    },
    StageFinished { index: usize },
    Finished {
        reason: FinishReason,
        total_elapsed_secs: f64,
    },
}

/// Active scenario; [`Drop`] calls [`ScenarioRunner::stop`].
pub struct ScenarioRunner {
    cancel: Arc<AtomicBool>,
    event_rx: mpsc::Receiver<ScenarioEvent>,
    started_at: Instant,
}

impl ScenarioRunner {
    pub fn start(def: ScenarioDefinition) -> Self {
        let cancel = Arc::new(AtomicBool::new(false));
        let (event_tx, event_rx) = mpsc::channel();
        let started_at = Instant::now();

        let cancel_clone = cancel.clone();
        thread::Builder::new()
            .name("stress-kit-scenario".into())
            .spawn(move || supervisor(def, cancel_clone, event_tx, started_at))
            .expect("stress-kit: failed to spawn scenario supervisor");

        Self { cancel, event_rx, started_at }
    }

    pub fn stop(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    /// Clone of the internal cancel flag. Useful when the host needs an
    /// external lever (e.g. an MCP `stop_stress_run` tool) without holding
    /// the runner itself.
    pub fn cancel_handle(&self) -> Arc<AtomicBool> {
        self.cancel.clone()
    }

    /// `true` after `stop`, completion, or wall cap.
    pub fn is_stopping(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    pub fn elapsed(&self) -> Duration {
        self.started_at.elapsed()
    }

    /// All pending events, FIFO.
    pub fn try_recv_all(&self) -> Vec<ScenarioEvent> {
        let mut events = Vec::new();
        while let Ok(e) = self.event_rx.try_recv() {
            events.push(e);
        }
        events
    }
}

impl Drop for ScenarioRunner {
    fn drop(&mut self) {
        self.stop();
    }
}

fn supervisor(
    def: ScenarioDefinition,
    cancel: Arc<AtomicBool>,
    event_tx: mpsc::Sender<ScenarioEvent>,
    started_at: Instant,
) {
    let stage_count = def.stages.len();
    if stage_count == 0 {
        event_tx
            .send(ScenarioEvent::Finished {
                reason: FinishReason::Completed,
                total_elapsed_secs: 0.0,
            })
            .ok();
        return;
    }

    'outer: loop {
        for (stage_idx, stage) in def.stages.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                send_finish(&event_tx, FinishReason::Cancelled, &started_at);
                return;
            }

            if is_total_time_exceeded(&def, &started_at) {
                send_finish(&event_tx, FinishReason::TotalTime, &started_at);
                return;
            }

            event_tx
                .send(ScenarioEvent::StageStarted {
                    index: stage_idx,
                    label: stage.label.clone(),
                    stage_count,
                })
                .ok();

            let stage_cancel = Arc::new(AtomicBool::new(false));
            let stage_started = Instant::now();
            let stage_duration = Duration::from_secs(stage.duration_secs.max(1));
            let stage_deadline = stage_started + stage_duration;

            let thread_count = if stage.config.threads == 0 {
                thread::available_parallelism()
                    .map(|n| n.get())
                    .unwrap_or(1)
            } else {
                stage.config.threads
            };

            let (metrics_tx, metrics_rx) = mpsc::channel::<Metrics>();
            let stage_cancel_clone = stage_cancel.clone();
            let config_clone = stage.config.clone();

            let stressor_handle = thread::Builder::new()
                .name(format!("stress-kit-stage-{stage_idx}"))
                .spawn(move || {
                    stressors::run_core(
                        &config_clone,
                        thread_count,
                        &stage_cancel_clone,
                        &metrics_tx,
                        stage_started,
                    );
                })
                .expect("stress-kit: failed to spawn stage stressor");

            let finish_reason = 'stage: loop {
                if cancel.load(Ordering::Relaxed) {
                    stage_cancel.store(true, Ordering::SeqCst);
                    let _ = stressor_handle.join();
                    drain_remaining_metrics(&metrics_rx, &event_tx, stage_idx);
                    event_tx.send(ScenarioEvent::StageFinished { index: stage_idx }).ok();
                    send_finish(&event_tx, FinishReason::Cancelled, &started_at);
                    return;
                }

                if is_total_time_exceeded(&def, &started_at) {
                    stage_cancel.store(true, Ordering::SeqCst);
                    let _ = stressor_handle.join();
                    drain_remaining_metrics(&metrics_rx, &event_tx, stage_idx);
                    event_tx.send(ScenarioEvent::StageFinished { index: stage_idx }).ok();
                    send_finish(&event_tx, FinishReason::TotalTime, &started_at);
                    return;
                }

                if Instant::now() >= stage_deadline {
                    stage_cancel.store(true, Ordering::SeqCst);
                    let _ = stressor_handle.join();
                    drain_remaining_metrics(&metrics_rx, &event_tx, stage_idx);
                    break 'stage ();
                }

                while let Ok(m) = metrics_rx.try_recv() {
                    event_tx
                        .send(ScenarioEvent::Tick {
                            stage_index: stage_idx,
                            metrics: m,
                        })
                        .ok();
                }

                // Stressor exited early (fatal error or chose to bail). Stop
                // wasting wall time waiting for the deadline and move to the
                // next stage. Final metrics still flow through `drain_remaining_metrics`.
                if stressor_handle.is_finished() {
                    let _ = stressor_handle.join();
                    drain_remaining_metrics(&metrics_rx, &event_tx, stage_idx);
                    break 'stage ();
                }

                thread::sleep(Duration::from_millis(50));
            };
            let _ = finish_reason;

            event_tx.send(ScenarioEvent::StageFinished { index: stage_idx }).ok();
        }

        if !def.repeat_until_total {
            break 'outer;
        }
        if def.total_wall_secs.is_none() {
            break 'outer;
        }
        if is_total_time_exceeded(&def, &started_at) {
            send_finish(&event_tx, FinishReason::TotalTime, &started_at);
            return;
        }
    }

    send_finish(&event_tx, FinishReason::Completed, &started_at);
}

#[inline]
fn drain_remaining_metrics(
    metrics_rx: &mpsc::Receiver<Metrics>,
    event_tx: &mpsc::Sender<ScenarioEvent>,
    stage_idx: usize,
) {
    while let Ok(m) = metrics_rx.try_recv() {
        event_tx
            .send(ScenarioEvent::Tick {
                stage_index: stage_idx,
                metrics: m,
            })
            .ok();
    }
}

#[inline]
fn is_total_time_exceeded(def: &ScenarioDefinition, started_at: &Instant) -> bool {
    def.total_wall_secs
        .is_some_and(|t| started_at.elapsed().as_secs() >= t)
}

#[inline]
fn send_finish(
    tx: &mpsc::Sender<ScenarioEvent>,
    reason: FinishReason,
    started_at: &Instant,
) {
    tx.send(ScenarioEvent::Finished {
        reason,
        total_elapsed_secs: started_at.elapsed().as_secs_f64(),
    })
    .ok();
}
