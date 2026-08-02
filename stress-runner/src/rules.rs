//! Per-stage verdict rules: configurable pass/fail policy evaluated from
//! 1 Hz tick data. Absent telemetry (WHEA/TDR/temps off-Windows) never
//! violates a rule, so dev runs pass vacuously. Signals that the stage's load
//! never ran — a fatal abort, an `inconclusive -` message, no measured
//! throughput — are rule-independent and fail under every policy.

use serde::{Deserialize, Serialize};
use stress_kit::telemetry::TelemetrySnapshot;
use stress_kit::{Metrics, Stressor};

/// Sustained temperature breach: `limit_c` exceeded for `consecutive_ticks`+ ticks.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TempRule {
    pub limit_c: f32,
    #[serde(default = "default_temp_ticks")]
    pub consecutive_ticks: u32,
}

fn default_temp_ticks() -> u32 {
    5
}

/// Sustained clock drop below a fraction of the stage's max while under load.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ClockCollapseRule {
    pub below_pct_of_stage_max: f32,
    #[serde(default = "default_collapse_ticks")]
    pub consecutive_ticks: u32,
    #[serde(default = "default_collapse_usage")]
    pub min_cpu_usage_pct: f32,
}

fn default_collapse_ticks() -> u32 {
    10
}

fn default_collapse_usage() -> f32 {
    50.0
}

/// Sustained rail droop: reading stayed below `floor_v` for `consecutive_ticks`+ ticks.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RailDroopRule {
    pub floor_v: f32,
    #[serde(default = "default_rail_ticks")]
    pub consecutive_ticks: u32,
}

fn default_rail_ticks() -> u32 {
    5
}

/// Post-warmup throughput stability: coefficient of variation must stay under `max_cv`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ThroughputCvRule {
    #[serde(default = "default_cv_warmup")]
    pub warmup_ticks: u32,
    pub max_cv: f64,
    #[serde(default = "default_cv_min_samples")]
    pub min_samples: u32,
}

fn default_cv_warmup() -> u32 {
    10
}

fn default_cv_min_samples() -> u32 {
    30
}

/// Pass/fail policy for a run; `None` sub-rules are skipped.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VerdictRules {
    pub whea_fails: bool,
    pub tdr_fails: bool,
    pub stressor_errors_fail: bool,
    pub max_cpu_temp_c: Option<TempRule>,
    pub max_gpu_temp_c: Option<TempRule>,
    pub clock_collapse: Option<ClockCollapseRule>,
    pub throughput_cv: Option<ThroughputCvRule>,
    /// +12V droop floor. Stays `None` in every built-in policy: SuperIO rails
    /// are read through an assumed, board-specific divider and publish
    /// `calibrated: false`, so the number cannot gate a certification run.
    /// Only set this per-board after verifying against a meter.
    pub min_v12_v: Option<RailDroopRule>,
}

impl Default for VerdictRules {
    /// Matches the legacy whole-run verdict: WHEA + stressor errors fail, nothing else.
    fn default() -> Self {
        Self {
            whea_fails: true,
            tdr_fails: false,
            stressor_errors_fail: true,
            max_cpu_temp_c: None,
            max_gpu_temp_c: None,
            clock_collapse: None,
            throughput_cv: None,
            min_v12_v: None,
        }
    }
}

impl VerdictRules {
    /// Full certification policy with shop-default thresholds.
    pub fn certification() -> Self {
        Self {
            whea_fails: true,
            tdr_fails: true,
            stressor_errors_fail: true,
            max_cpu_temp_c: Some(TempRule {
                limit_c: 95.0,
                consecutive_ticks: 5,
            }),
            max_gpu_temp_c: Some(TempRule {
                limit_c: 90.0,
                consecutive_ticks: 5,
            }),
            clock_collapse: Some(ClockCollapseRule {
                below_pct_of_stage_max: 0.60,
                consecutive_ticks: 10,
                min_cpu_usage_pct: 50.0,
            }),
            throughput_cv: Some(ThroughputCvRule {
                warmup_ticks: 10,
                max_cv: 0.20,
                min_samples: 30,
            }),
            min_v12_v: None,
        }
    }
}

/// One rule breach with enough detail for an operator-readable line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuleViolation {
    Whea { corrected: u32, fatal: u32 },
    Tdr { delta: u32 },
    StressorErrors { count: u64 },
    CpuTemp { limit_c: f32, peak_c: f32, sustained_ticks: u32 },
    GpuTemp { limit_c: f32, peak_c: f32, sustained_ticks: u32 },
    ClockCollapse { below_pct: f32, ticks: u32 },
    ThroughputUnstable { cv: f64, max_cv: f64 },
    RailDroop { rail: String, floor_v: f32, min_v: f32, sustained_ticks: u32 },
    /// The stressor gave up before its intended load finished, so nothing this
    /// stage measured proves the hardware is healthy.
    FatalAbort { reason: Option<String> },
    /// No throughput sample was ever measured, over enough ticks to have shown one.
    NoThroughput { ticks: u32 },
    /// The stressor named a load it could not apply.
    Inconclusive { reason: String },
    /// A GPU rule was configured but no GPU telemetry was ever sampled, so the
    /// limit could not be evaluated. An ungradeable limit is not a pass.
    GpuTelemetryMissing { rule: String, ticks: u32 },
}

impl RuleViolation {
    pub fn describe(&self) -> String {
        match self {
            Self::Whea { corrected, fatal } => match (*fatal, *corrected) {
                (0, c) => format!("{c} corrected WHEA hardware error(s)"),
                (f, 0) => format!("{f} fatal WHEA hardware error(s)"),
                (f, c) => format!(
                    "{} WHEA hardware error(s): {f} fatal, {c} corrected",
                    f + c
                ),
            },
            Self::Tdr { delta } => format!("{delta} GPU driver reset(s) (TDR)"),
            Self::StressorErrors { count } => format!("{count} stressor data error(s)"),
            Self::CpuTemp { limit_c, peak_c, sustained_ticks } => format!(
                "CPU over {limit_c:.0}C for {sustained_ticks}s (peak {peak_c:.1}C)"
            ),
            Self::GpuTemp { limit_c, peak_c, sustained_ticks } => format!(
                "GPU over {limit_c:.0}C for {sustained_ticks}s (peak {peak_c:.1}C)"
            ),
            Self::GpuTelemetryMissing { rule, ticks } => format!(
                "{rule} could not be evaluated: no GPU telemetry in {ticks} ticks"
            ),
            Self::ClockCollapse { below_pct, ticks } => format!(
                "clock under {:.0}% of stage max for {ticks}s",
                below_pct * 100.0
            ),
            Self::ThroughputUnstable { cv, max_cv } => format!(
                "throughput CV {cv:.3} over band {max_cv:.3}"
            ),
            Self::RailDroop { rail, floor_v, min_v, sustained_ticks } => format!(
                "{rail} under {floor_v:.2}V for {sustained_ticks}s (min {min_v:.2}V, uncalibrated)"
            ),
            Self::FatalAbort { reason } => match reason {
                Some(r) => format!(
                    "inconclusive: stressor aborted before its load completed — {r}"
                ),
                None => "inconclusive: stressor aborted before its load completed".to_string(),
            },
            Self::NoThroughput { ticks } => format!(
                "inconclusive - no throughput measured over {ticks} sampled tick(s); \
                 the stage's load never ran"
            ),
            Self::Inconclusive { reason } => reason.clone(),
        }
    }
}

/// Per-stage verdict produced by [`evaluate_stage`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StageVerdict {
    pub index: u32,
    pub label: String,
    pub pass: bool,
    pub violations: Vec<RuleViolation>,
    /// Non-failing advisories (e.g. WHEA source unavailable) surfaced so a
    /// pass isn't mistaken for a fully-monitored run.
    #[serde(default)]
    pub warnings: Vec<String>,
}

impl StageVerdict {
    pub fn violation_lines(&self) -> Vec<String> {
        self.violations.iter().map(|v| v.describe()).collect()
    }
}

/// Tick-by-tick rollup for one stage.
#[derive(Debug, Clone)]
pub struct StageStats {
    pub index: u32,
    pub label: String,
    pub stressor: Stressor,
    pub ticks: u32,
    pub max_cpu_temp_c: Option<f32>,
    pub sum_cpu_temp: f64,
    pub cpu_temp_samples: u32,
    pub max_gpu_temp_c: Option<f32>,
    /// Ticks that carried at least one GPU telemetry sample. Zero means every
    /// GPU rule below was unevaluable.
    pub gpu_temp_samples: u32,
    /// Lowest +12V sample this stage; `None` when no SuperIO rail was readable.
    pub min_v12_v: Option<f32>,
    pub max_avg_clock_mhz: Option<u32>,
    cpu_temp_over_run: u32,
    pub worst_cpu_temp_over: u32,
    gpu_temp_over_run: u32,
    pub worst_gpu_temp_over: u32,
    rail_under_run: u32,
    pub worst_rail_under: u32,
    collapse_run: u32,
    pub worst_collapse_run: u32,
    pub peak_throughput: Option<f64>,
    tp_sum: f64,
    tp_sum_sq: f64,
    pub tp_samples: u32,
    /// A sample with throughput above zero was folded this stage, from a
    /// periodic tick or an off-cadence final sample. Latched.
    pub observed_throughput: bool,
    pub errors: u64,
    /// Newest `Metrics.last_error` folded in this stage.
    pub last_error: Option<String>,
    /// First `inconclusive -` message folded this stage, device-loss text
    /// excluded so it stays hardware-classified. Latched.
    pub inconclusive_reason: Option<String>,
    /// A stressor reported `Metrics.fatal` at least once this stage. Latched:
    /// later clean ticks never clear it.
    pub fatal_abort: bool,
    /// `last_error` carried by the first fatal sample.
    pub fatal_reason: Option<String>,
    whea_baseline: u32,
    pub whea_delta: u32,
    whea_corrected_baseline: u32,
    pub whea_corrected_delta: u32,
    whea_fatal_baseline: u32,
    pub whea_fatal_delta: u32,
    /// WHEA source was unavailable at any point this stage (Windows only).
    pub whea_unavailable: bool,
    tdr_baseline: u32,
    pub tdr_delta: u32,
    pub gpu_throttle_ticks: u32,
}

impl StageStats {
    /// Start a stage, capturing WHEA/TDR baselines from the freshest snapshot.
    pub fn begin(index: u32, label: &str, stressor: Stressor, snapshot: &TelemetrySnapshot) -> Self {
        Self {
            index,
            label: label.to_string(),
            stressor,
            ticks: 0,
            max_cpu_temp_c: None,
            sum_cpu_temp: 0.0,
            cpu_temp_samples: 0,
            max_gpu_temp_c: None,
            gpu_temp_samples: 0,
            min_v12_v: None,
            max_avg_clock_mhz: None,
            cpu_temp_over_run: 0,
            worst_cpu_temp_over: 0,
            gpu_temp_over_run: 0,
            worst_gpu_temp_over: 0,
            rail_under_run: 0,
            worst_rail_under: 0,
            collapse_run: 0,
            worst_collapse_run: 0,
            peak_throughput: None,
            tp_sum: 0.0,
            tp_sum_sq: 0.0,
            tp_samples: 0,
            observed_throughput: false,
            errors: 0,
            last_error: None,
            inconclusive_reason: None,
            fatal_abort: false,
            fatal_reason: None,
            whea_baseline: whea_count(snapshot),
            whea_delta: 0,
            whea_corrected_baseline: whea_corrected(snapshot),
            whea_corrected_delta: 0,
            whea_fatal_baseline: whea_fatal(snapshot),
            whea_fatal_delta: 0,
            whea_unavailable: snapshot.whea_unavailable,
            tdr_baseline: tdr_count(snapshot),
            tdr_delta: 0,
            gpu_throttle_ticks: 0,
        }
    }

    /// Fold the stressor-reported side of one sample: cumulative errors, newest
    /// `last_error`, and the latched throughput-seen, inconclusive, fatal flags.
    fn absorb_metrics(&mut self, metrics: &Metrics) {
        // `Metrics.errors` is cumulative within the stage's stressor.
        self.errors = self.errors.max(metrics.errors);
        if metrics.throughput > 0.0 {
            self.observed_throughput = true;
        }
        if let Some(msg) = &metrics.last_error {
            self.last_error = Some(msg.clone());
            if self.inconclusive_reason.is_none()
                && is_inconclusive_message(msg)
                && !is_device_loss_message(msg)
            {
                self.inconclusive_reason = Some(msg.clone());
            }
        }
        if metrics.fatal {
            self.fatal_abort = true;
            if self.fatal_reason.is_none() {
                self.fatal_reason = metrics
                    .last_error
                    .clone()
                    .or_else(|| self.last_error.clone());
            }
        }
    }

    /// Fold a sample that arrived outside the ~1 Hz cadence without counting
    /// it as a tick, so a final or fatal sample is never dropped.
    pub fn absorb_final(&mut self, metrics: &Metrics) {
        self.absorb_metrics(metrics);
    }

    /// Fold one ~1 Hz tick of stressor metrics + telemetry.
    pub fn absorb_tick(
        &mut self,
        metrics: &Metrics,
        snapshot: &TelemetrySnapshot,
        rules: &VerdictRules,
    ) {
        self.ticks = self.ticks.saturating_add(1);
        self.absorb_metrics(metrics);

        self.whea_delta = whea_count(snapshot).saturating_sub(self.whea_baseline);
        self.whea_corrected_delta =
            whea_corrected(snapshot).saturating_sub(self.whea_corrected_baseline);
        self.whea_fatal_delta = whea_fatal(snapshot).saturating_sub(self.whea_fatal_baseline);
        self.whea_unavailable |= snapshot.whea_unavailable;
        self.tdr_delta = tdr_count(snapshot).saturating_sub(self.tdr_baseline);

        let tick_max_cpu_temp = snapshot.cpu_package_temp_c().or_else(|| {
            snapshot
                .cores
                .iter()
                .filter_map(|c| c.temp_c)
                .fold(None::<f32>, |a, t| Some(a.map_or(t, |m| m.max(t))))
        });
        if let Some(t) = tick_max_cpu_temp {
            self.max_cpu_temp_c = Some(self.max_cpu_temp_c.map_or(t, |m| m.max(t)));
            self.sum_cpu_temp += t as f64;
            self.cpu_temp_samples = self.cpu_temp_samples.saturating_add(1);
        }
        if let Some(rule) = &rules.max_cpu_temp_c {
            track_over_run(
                tick_max_cpu_temp,
                rule.limit_c,
                &mut self.cpu_temp_over_run,
                &mut self.worst_cpu_temp_over,
            );
        }

        let tick_max_gpu_temp = snapshot
            .gpus
            .iter()
            .filter_map(|g| g.temp_c)
            .fold(None::<f32>, |acc, t| Some(acc.map_or(t, |m| m.max(t))));
        if let Some(t) = tick_max_gpu_temp {
            self.max_gpu_temp_c = Some(self.max_gpu_temp_c.map_or(t, |m| m.max(t)));
            self.gpu_temp_samples = self.gpu_temp_samples.saturating_add(1);
        }
        if let Some(rule) = &rules.max_gpu_temp_c {
            track_over_run(
                tick_max_gpu_temp,
                rule.limit_c,
                &mut self.gpu_temp_over_run,
                &mut self.worst_gpu_temp_over,
            );
        }

        let tick_v12 = snapshot.rail_12v();
        if let Some(v) = tick_v12 {
            self.min_v12_v = Some(self.min_v12_v.map_or(v, |m| m.min(v)));
        }
        if let Some(rule) = &rules.min_v12_v {
            track_under_run(
                tick_v12,
                rule.floor_v,
                &mut self.rail_under_run,
                &mut self.worst_rail_under,
            );
        }

        if snapshot.gpus.iter().any(|g| {
            g.throttle_reasons.iter().any(|r| {
                let r = r.to_ascii_lowercase();
                r.contains("thermal") || r.contains("hw_slowdown") || r.contains("power_brake")
            })
        }) {
            self.gpu_throttle_ticks = self.gpu_throttle_ticks.saturating_add(1);
        }

        let clocks: Vec<u64> = snapshot
            .cores
            .iter()
            .map(|c| c.freq_mhz)
            .filter(|&f| f > 0)
            .collect();
        let avg_usage = if snapshot.cores.is_empty() {
            0.0
        } else {
            snapshot.cores.iter().map(|c| c.usage_pct).sum::<f32>() / snapshot.cores.len() as f32
        };
        if !clocks.is_empty() {
            let avg_clock = (clocks.iter().sum::<u64>() / clocks.len() as u64) as u32;
            self.max_avg_clock_mhz =
                Some(self.max_avg_clock_mhz.map_or(avg_clock, |m| m.max(avg_clock)));
            if let Some(rule) = &rules.clock_collapse {
                let stage_max = self.max_avg_clock_mhz.unwrap_or(avg_clock) as f32;
                let collapsed = (avg_clock as f32) < stage_max * rule.below_pct_of_stage_max
                    && avg_usage >= rule.min_cpu_usage_pct;
                if collapsed {
                    self.collapse_run = self.collapse_run.saturating_add(1);
                    self.worst_collapse_run = self.worst_collapse_run.max(self.collapse_run);
                } else {
                    self.collapse_run = 0;
                }
            }
        }

        if metrics.throughput > 0.0 {
            self.peak_throughput = Some(
                self.peak_throughput
                    .map_or(metrics.throughput, |p| p.max(metrics.throughput)),
            );
            let warmup = rules
                .throughput_cv
                .map(|r| r.warmup_ticks)
                .unwrap_or(0);
            if self.ticks > warmup {
                self.tp_sum += metrics.throughput;
                self.tp_sum_sq += metrics.throughput * metrics.throughput;
                self.tp_samples = self.tp_samples.saturating_add(1);
            }
        }
    }

    /// Close WHEA/TDR deltas against the freshest snapshot at stage end.
    pub fn finish(&mut self, snapshot: &TelemetrySnapshot) {
        self.whea_delta = whea_count(snapshot).saturating_sub(self.whea_baseline);
        self.whea_corrected_delta =
            whea_corrected(snapshot).saturating_sub(self.whea_corrected_baseline);
        self.whea_fatal_delta = whea_fatal(snapshot).saturating_sub(self.whea_fatal_baseline);
        self.whea_unavailable |= snapshot.whea_unavailable;
        self.tdr_delta = tdr_count(snapshot).saturating_sub(self.tdr_baseline);
    }

    /// Post-warmup coefficient of variation; `None` until two samples exist.
    pub fn throughput_cv(&self) -> Option<f64> {
        if self.tp_samples < 2 {
            return None;
        }
        let n = self.tp_samples as f64;
        let mean = self.tp_sum / n;
        if mean <= 0.0 {
            return None;
        }
        let var = (self.tp_sum_sq / n - mean * mean).max(0.0);
        Some(var.sqrt() / mean)
    }

    pub fn avg_cpu_temp_c(&self) -> Option<f32> {
        if self.cpu_temp_samples == 0 {
            return None;
        }
        Some((self.sum_cpu_temp / self.cpu_temp_samples as f64) as f32)
    }

    /// `true` when the stage was sampled long enough to have shown throughput
    /// and never did: no work unit completed, so no load ran.
    pub fn produced_no_work(&self) -> bool {
        self.ticks >= NO_WORK_MIN_TICKS && !self.observed_throughput
    }

    /// `true` when something about this stage says its load did not run: the
    /// stressor aborted, named a load it could not apply, or measured nothing.
    pub fn load_unproven(&self) -> bool {
        self.fatal_abort || self.inconclusive_reason.is_some() || self.produced_no_work()
    }
}

/// Consecutive-breach run tracking for one temp reading against a limit.
fn track_over_run(temp: Option<f32>, limit_c: f32, run: &mut u32, worst: &mut u32) {
    match temp {
        Some(t) if t > limit_c => {
            *run = run.saturating_add(1);
            *worst = (*worst).max(*run);
        }
        Some(_) => *run = 0,
        None => {}
    }
}

/// Consecutive-breach run tracking for one reading falling below a floor.
fn track_under_run(value: Option<f32>, floor: f32, run: &mut u32, worst: &mut u32) {
    match value {
        Some(v) if v < floor => {
            *run = run.saturating_add(1);
            *worst = (*worst).max(*run);
        }
        Some(_) => *run = 0,
        None => {}
    }
}

fn whea_count(snapshot: &TelemetrySnapshot) -> u32 {
    snapshot
        .whea
        .as_ref()
        .map(|w| w.delta_since_program_start as u32)
        .unwrap_or(0)
}

fn whea_corrected(snapshot: &TelemetrySnapshot) -> u32 {
    snapshot
        .whea
        .as_ref()
        .map(|w| w.corrected_delta as u32)
        .unwrap_or(0)
}

fn whea_fatal(snapshot: &TelemetrySnapshot) -> u32 {
    snapshot
        .whea
        .as_ref()
        .map(|w| w.fatal_delta as u32)
        .unwrap_or(0)
}

fn tdr_count(snapshot: &TelemetrySnapshot) -> u32 {
    snapshot
        .tdr
        .as_ref()
        .map(|t| t.delta_since_program_start as u32)
        .unwrap_or(0)
}

/// Sampled ticks a stage needs before zero throughput reads as "no load ran".
/// Every stressor derives throughput from a work counter it advances inside its
/// worker loop, so ten seconds of zero means no work unit completed; below that
/// a stage can still be waiting on its first sample (allocation, warm-up) or be
/// shorter than the sampling cadence.
const NO_WORK_MIN_TICKS: u32 = 10;

/// Device-loss vocabulary: hardware evidence that outranks an inconclusive marker.
pub(crate) fn is_device_loss_message(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    m.contains("device is lost")
        || m.contains("device lost")
        || m.contains("device removed")
        || m.contains("dxgi_error")
}

/// The `inconclusive -` marker stress-kit stamps on messages that report a load
/// it could not apply, so the stage proves nothing about any component.
pub(crate) fn is_inconclusive_message(msg: &str) -> bool {
    msg.to_ascii_lowercase().contains("inconclusive -")
}

/// Stressors whose tick throughput is bursty or pattern-phased by design;
/// CV never applies.
fn cv_exempt(stressor: Stressor) -> bool {
    matches!(
        stressor,
        Stressor::Psu
            | Stressor::PsuTransient
            | Stressor::Disk
            | Stressor::MemTest
            | Stressor::GpuVram
    )
}

/// Stressors that put load on the GPU, so a GPU rule is expected to be
/// evaluable during them. A CPU-only stage is not graded on GPU telemetry.
fn touches_gpu(stressor: Stressor) -> bool {
    matches!(
        stressor,
        Stressor::Gpu
            | Stressor::GpuMatmul
            | Stressor::GpuVram
            | Stressor::GpuPcie
            | Stressor::GpuDisplay
            | Stressor::Psu
            | Stressor::PsuTransient
            | Stressor::Combined
    )
}

/// Evaluate one finished stage against the rules.
pub fn evaluate_stage(stats: &StageStats, rules: &VerdictRules) -> StageVerdict {
    let mut violations = Vec::new();
    let mut warnings = Vec::new();

    // Rule-independent: a load that never ran cannot clear any policy.
    if stats.fatal_abort {
        violations.push(RuleViolation::FatalAbort {
            reason: stats.fatal_reason.clone().or_else(|| stats.last_error.clone()),
        });
    }
    if stats.produced_no_work() {
        violations.push(RuleViolation::NoThroughput { ticks: stats.ticks });
    }
    if let Some(reason) = &stats.inconclusive_reason {
        violations.push(RuleViolation::Inconclusive {
            reason: reason.clone(),
        });
    }
    if stats.ticks == 0 {
        warnings.push(
            "stage produced no monitored ticks — temperature, clock, and throughput \
             rules not checked"
                .to_string(),
        );
    }

    if rules.whea_fails {
        if stats.whea_corrected_delta > 0 || stats.whea_fatal_delta > 0 {
            violations.push(RuleViolation::Whea {
                corrected: stats.whea_corrected_delta,
                fatal: stats.whea_fatal_delta,
            });
        } else if stats.whea_delta > 0 {
            // Counter moved but severity wasn't classified: still a hit.
            violations.push(RuleViolation::Whea {
                corrected: stats.whea_delta,
                fatal: 0,
            });
        }
        if stats.whea_unavailable {
            warnings.push(
                "WHEA monitoring unavailable — machine-check errors not checked".to_string(),
            );
        }
    }
    if rules.tdr_fails && stats.tdr_delta > 0 {
        violations.push(RuleViolation::Tdr { delta: stats.tdr_delta });
    }
    if rules.stressor_errors_fail && stats.errors > 0 {
        violations.push(RuleViolation::StressorErrors { count: stats.errors });
    }
    if let Some(rule) = &rules.max_cpu_temp_c {
        if stats.worst_cpu_temp_over >= rule.consecutive_ticks {
            violations.push(RuleViolation::CpuTemp {
                limit_c: rule.limit_c,
                peak_c: stats.max_cpu_temp_c.unwrap_or(rule.limit_c),
                sustained_ticks: stats.worst_cpu_temp_over,
            });
        }
    }
    if let Some(rule) = &rules.max_gpu_temp_c {
        if stats.gpu_temp_samples == 0 && stats.ticks > 0 && touches_gpu(stats.stressor) {
            violations.push(RuleViolation::GpuTelemetryMissing {
                rule: format!("max_gpu_temp_c {:.0}C", rule.limit_c),
                ticks: stats.ticks,
            });
        } else if stats.worst_gpu_temp_over >= rule.consecutive_ticks {
            violations.push(RuleViolation::GpuTemp {
                limit_c: rule.limit_c,
                peak_c: stats.max_gpu_temp_c.unwrap_or(rule.limit_c),
                sustained_ticks: stats.worst_gpu_temp_over,
            });
        }
    }
    if let Some(rule) = &rules.min_v12_v {
        if stats.worst_rail_under >= rule.consecutive_ticks {
            violations.push(RuleViolation::RailDroop {
                rail: "+12V".to_string(),
                floor_v: rule.floor_v,
                min_v: stats.min_v12_v.unwrap_or(rule.floor_v),
                sustained_ticks: stats.worst_rail_under,
            });
        }
    }
    if let Some(rule) = &rules.clock_collapse {
        if stats.worst_collapse_run >= rule.consecutive_ticks {
            violations.push(RuleViolation::ClockCollapse {
                below_pct: rule.below_pct_of_stage_max,
                ticks: stats.worst_collapse_run,
            });
        }
    }
    if let Some(rule) = &rules.throughput_cv {
        if !cv_exempt(stats.stressor) && stats.tp_samples >= rule.min_samples {
            if let Some(cv) = stats.throughput_cv() {
                if cv > rule.max_cv {
                    violations.push(RuleViolation::ThroughputUnstable {
                        cv,
                        max_cv: rule.max_cv,
                    });
                }
            }
        }
    }

    StageVerdict {
        index: stats.index,
        label: stats.label.clone(),
        pass: violations.is_empty(),
        violations,
        warnings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stress_kit::telemetry::{CoreSample, TdrCounters, VoltageReading, WheaCounters};

    fn snapshot(temp_c: f32, freq_mhz: u64, usage: f32) -> TelemetrySnapshot {
        TelemetrySnapshot {
            captured_at_unix_ms: 1,
            cores: vec![CoreSample {
                index: 0,
                brand: "test".into(),
                usage_pct: usage,
                freq_mhz,
                temp_c: Some(temp_c),
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    /// Snapshot carrying a GPU reading, for stages that load the GPU.
    fn snapshot_with_gpu(temp_c: f32, freq_mhz: u64, usage: f32, gpu_temp_c: f32) -> TelemetrySnapshot {
        let mut snap = snapshot(temp_c, freq_mhz, usage);
        snap.gpus = vec![stress_kit::telemetry::GpuSample {
            index: 0,
            vendor: "test".into(),
            name: "test gpu".into(),
            temp_c: Some(gpu_temp_c),
            ..Default::default()
        }];
        snap
    }

    fn metrics(throughput: f64, errors: u64) -> Metrics {
        Metrics {
            elapsed_secs: 1.0,
            throughput,
            errors,
            ..Default::default()
        }
    }

    fn warn_metrics(throughput: f64, reason: &str) -> Metrics {
        Metrics {
            elapsed_secs: 1.0,
            throughput,
            last_error: Some(reason.to_string()),
            fatal: false,
            errors: 0,
        }
    }

    fn fatal_metrics(reason: &str) -> Metrics {
        Metrics {
            elapsed_secs: 1.0,
            throughput: 0.0,
            last_error: Some(reason.to_string()),
            fatal: true,
            errors: 0,
        }
    }

    fn stats_for(_rules: &VerdictRules) -> StageStats {
        StageStats::begin(0, "test", Stressor::Cpu, &snapshot(50.0, 4000, 90.0))
    }

    #[test]
    fn clean_run_passes() {
        let rules = VerdictRules::certification();
        let mut stats = stats_for(&rules);
        for _ in 0..60 {
            stats.absorb_tick(&metrics(100.0, 0), &snapshot(70.0, 4000, 95.0), &rules);
        }
        stats.finish(&snapshot(70.0, 4000, 95.0));
        let verdict = evaluate_stage(&stats, &rules);
        assert!(verdict.pass, "violations: {:?}", verdict.violations);
    }

    #[test]
    fn stressor_errors_fail() {
        let rules = VerdictRules::certification();
        let mut stats = stats_for(&rules);
        stats.absorb_tick(&metrics(100.0, 3), &snapshot(70.0, 4000, 95.0), &rules);
        let verdict = evaluate_stage(&stats, &rules);
        assert!(!verdict.pass);
        assert!(matches!(
            verdict.violations[0],
            RuleViolation::StressorErrors { count: 3 }
        ));
    }

    #[test]
    fn sustained_temp_fails_but_spike_passes() {
        let rules = VerdictRules::certification();

        let mut spike = stats_for(&rules);
        for i in 0..20 {
            let t = if i == 5 { 99.0 } else { 70.0 };
            spike.absorb_tick(&metrics(100.0, 0), &snapshot(t, 4000, 95.0), &rules);
        }
        assert!(evaluate_stage(&spike, &rules).pass);

        let mut sustained = stats_for(&rules);
        for _ in 0..6 {
            sustained.absorb_tick(&metrics(100.0, 0), &snapshot(99.0, 4000, 95.0), &rules);
        }
        let verdict = evaluate_stage(&sustained, &rules);
        assert!(!verdict.pass);
        assert!(matches!(
            verdict.violations[0],
            RuleViolation::CpuTemp { .. }
        ));
    }

    #[test]
    fn whea_delta_fails_and_vacuous_passes_without_counters() {
        let rules = VerdictRules::certification();

        let mut stats = stats_for(&rules);
        let mut snap = snapshot(70.0, 4000, 95.0);
        snap.whea = Some(WheaCounters {
            delta_since_program_start: 2,
            fatal_delta: 1,
            corrected_delta: 1,
            ..Default::default()
        });
        stats.absorb_tick(&metrics(100.0, 0), &snap, &rules);
        stats.finish(&snap);
        let verdict = evaluate_stage(&stats, &rules);
        assert!(!verdict.pass);
        assert!(matches!(
            verdict.violations[0],
            RuleViolation::Whea { corrected: 1, fatal: 1 }
        ));

        // No whea/tdr source at all: vacuous pass.
        let mut bare = stats_for(&rules);
        bare.absorb_tick(&metrics(100.0, 0), &snapshot(70.0, 4000, 95.0), &rules);
        bare.finish(&snapshot(70.0, 4000, 95.0));
        assert!(evaluate_stage(&bare, &rules).pass);
    }

    #[test]
    fn whea_unavailable_warns_without_failing() {
        let rules = VerdictRules::certification();
        let mut snap = snapshot(70.0, 4000, 95.0);
        snap.whea = None;
        snap.whea_unavailable = true;

        let mut stats = stats_for(&rules);
        stats.absorb_tick(&metrics(100.0, 0), &snap, &rules);
        stats.finish(&snap);
        let verdict = evaluate_stage(&stats, &rules);
        assert!(verdict.pass, "unavailable WHEA must not fail the stage");
        assert!(
            verdict.warnings.iter().any(|w| w.contains("WHEA monitoring unavailable")),
            "warnings: {:?}",
            verdict.warnings
        );
    }

    #[test]
    fn tdr_only_fails_when_enabled() {
        let mut snap = snapshot(70.0, 4000, 95.0);
        snap.tdr = Some(TdrCounters {
            delta_since_program_start: 1,
            ..Default::default()
        });

        let cert = VerdictRules::certification();
        let mut stats = stats_for(&cert);
        stats.absorb_tick(&metrics(100.0, 0), &snap, &cert);
        assert!(!evaluate_stage(&stats, &cert).pass);

        let legacy = VerdictRules::default();
        let mut stats = stats_for(&legacy);
        stats.absorb_tick(&metrics(100.0, 0), &snap, &legacy);
        assert!(evaluate_stage(&stats, &legacy).pass);
    }

    #[test]
    fn clock_collapse_detects_sustained_drop_and_resets() {
        let rules = VerdictRules::certification();
        let mut stats = stats_for(&rules);
        // Establish stage max clock.
        for _ in 0..5 {
            stats.absorb_tick(&metrics(100.0, 0), &snapshot(70.0, 5000, 95.0), &rules);
        }
        // 9 collapsed ticks, one recovery, then 9 more: run resets, no violation.
        for _ in 0..9 {
            stats.absorb_tick(&metrics(100.0, 0), &snapshot(70.0, 2000, 95.0), &rules);
        }
        stats.absorb_tick(&metrics(100.0, 0), &snapshot(70.0, 5000, 95.0), &rules);
        for _ in 0..9 {
            stats.absorb_tick(&metrics(100.0, 0), &snapshot(70.0, 2000, 95.0), &rules);
        }
        assert!(evaluate_stage(&stats, &rules).pass);

        // 10 consecutive collapsed ticks: violation.
        for _ in 0..10 {
            stats.absorb_tick(&metrics(100.0, 0), &snapshot(70.0, 2000, 95.0), &rules);
        }
        let verdict = evaluate_stage(&stats, &rules);
        assert!(!verdict.pass);
        assert!(matches!(
            verdict.violations[0],
            RuleViolation::ClockCollapse { .. }
        ));
    }

    #[test]
    fn collapse_ignored_when_idle() {
        let rules = VerdictRules::certification();
        let mut stats = stats_for(&rules);
        for _ in 0..5 {
            stats.absorb_tick(&metrics(100.0, 0), &snapshot(70.0, 5000, 95.0), &rules);
        }
        // Low usage: collapse gate requires load.
        for _ in 0..20 {
            stats.absorb_tick(&metrics(100.0, 0), &snapshot(70.0, 1000, 5.0), &rules);
        }
        assert!(evaluate_stage(&stats, &rules).pass);
    }

    #[test]
    fn cv_violation_and_min_samples_skip() {
        let rules = VerdictRules::certification();

        // Wild throughput swings past warmup, enough samples: violation.
        let mut wild = stats_for(&rules);
        for i in 0..60 {
            let tp = if i % 2 == 0 { 10.0 } else { 1000.0 };
            wild.absorb_tick(&metrics(tp, 0), &snapshot(70.0, 4000, 95.0), &rules);
        }
        let verdict = evaluate_stage(&wild, &rules);
        assert!(!verdict.pass);
        assert!(matches!(
            verdict.violations[0],
            RuleViolation::ThroughputUnstable { .. }
        ));

        // Same swings but too few samples: rule skipped.
        let mut short = stats_for(&rules);
        for i in 0..20 {
            let tp = if i % 2 == 0 { 10.0 } else { 1000.0 };
            short.absorb_tick(&metrics(tp, 0), &snapshot(70.0, 4000, 95.0), &rules);
        }
        assert!(evaluate_stage(&short, &rules).pass);
    }

    #[test]
    fn cv_exempt_stressors_skip_the_band() {
        let rules = VerdictRules::certification();
        let mut stats = StageStats::begin(0, "disk", Stressor::Disk, &snapshot(50.0, 4000, 90.0));
        for i in 0..60 {
            let tp = if i % 2 == 0 { 10.0 } else { 1000.0 };
            stats.absorb_tick(&metrics(tp, 0), &snapshot(70.0, 4000, 95.0), &rules);
        }
        assert!(evaluate_stage(&stats, &rules).pass);
    }

    #[test]
    fn default_rules_match_legacy_behavior() {
        let legacy = VerdictRules::default();
        assert!(legacy.whea_fails);
        assert!(!legacy.tdr_fails);
        assert!(legacy.stressor_errors_fail);
        assert!(legacy.max_cpu_temp_c.is_none());
        assert!(legacy.clock_collapse.is_none());
        assert!(legacy.throughput_cv.is_none());
        assert!(legacy.min_v12_v.is_none());
    }

    /// The rail divider is uncalibrated and board-specific, so no built-in
    /// policy may fail a run on it.
    #[test]
    fn rail_droop_rule_is_off_in_every_builtin_policy() {
        assert!(VerdictRules::default().min_v12_v.is_none());
        assert!(VerdictRules::certification().min_v12_v.is_none());
    }

    /// A stage whose ticks include a fatal can never report pass, under any
    /// policy — this is the false-green the PSU GPU-leg bug produced.
    #[test]
    fn fatal_tick_cannot_pass_under_any_policy() {
        let reason = "psu: inconclusive - GPU unavailable, GPU leg never ran";
        for rules in [VerdictRules::certification(), VerdictRules::default()] {
            let mut stats = StageStats::begin(
                0,
                "psu",
                Stressor::Psu,
                &snapshot(50.0, 4000, 90.0),
            );
            for _ in 0..30 {
                stats.absorb_tick(&metrics(100.0, 0), &snapshot(70.0, 4000, 95.0), &rules);
            }
            stats.absorb_tick(&fatal_metrics(reason), &snapshot(70.0, 4000, 95.0), &rules);
            // Clean ticks after the fatal must not clear the latch.
            for _ in 0..30 {
                stats.absorb_tick(&metrics(100.0, 0), &snapshot(70.0, 4000, 95.0), &rules);
            }
            stats.finish(&snapshot(70.0, 4000, 95.0));

            let verdict = evaluate_stage(&stats, &rules);
            assert!(!verdict.pass, "fatal stage reported pass");
            assert!(verdict
                .violations
                .iter()
                .any(|v| matches!(v, RuleViolation::FatalAbort { .. })));
            assert!(
                verdict.violation_lines().iter().any(|l| l.contains(reason)),
                "reason missing from operator lines: {:?}",
                verdict.violation_lines()
            );
        }
    }

    /// A stage shorter than every warmup and min-sample window still fails when
    /// the fatal lands on its final tick — the psu grace-window shape.
    #[test]
    fn short_stage_with_a_fatal_final_tick_fails() {
        let reason = "psu: inconclusive - GPU unavailable, GPU leg never ran";
        for rules in [VerdictRules::certification(), VerdictRules::default()] {
            let mut stats =
                StageStats::begin(0, "psu", Stressor::Psu, &snapshot(50.0, 4000, 90.0));
            stats.absorb_tick(&metrics(100.0, 0), &snapshot(70.0, 4000, 95.0), &rules);
            stats.absorb_tick(&fatal_metrics(reason), &snapshot(70.0, 4000, 95.0), &rules);
            stats.finish(&snapshot(70.0, 4000, 95.0));

            assert_eq!(stats.ticks, 2);
            let verdict = evaluate_stage(&stats, &rules);
            assert!(!verdict.pass, "2-tick fatal stage reported pass");
            assert!(verdict
                .violations
                .iter()
                .any(|v| matches!(v, RuleViolation::FatalAbort { .. })));
            assert!(
                verdict.violation_lines().iter().any(|l| l.contains(reason)),
                "reason missing from operator lines: {:?}",
                verdict.violation_lines()
            );
        }
    }

    /// A stage that dies before its first tick carries no tick data and still
    /// cannot pass.
    #[test]
    fn stage_that_dies_before_its_first_tick_fails() {
        let rules = VerdictRules::certification();
        let mut stats = StageStats::begin(0, "gpu", Stressor::Gpu, &snapshot(50.0, 4000, 90.0));
        stats.absorb_final(&fatal_metrics("gpu: inconclusive - no GPU adapters found"));
        stats.finish(&snapshot(50.0, 4000, 90.0));

        assert_eq!(stats.ticks, 0);
        assert!(!evaluate_stage(&stats, &rules).pass, "zero-tick fatal stage reported pass");
    }

    /// The post-loop absorb: a fatal that lands after the last periodic tick
    /// still reaches the verdict.
    #[test]
    fn fatal_after_last_tick_is_captured_by_absorb_final() {
        let rules = VerdictRules::certification();
        let mut stats = stats_for(&rules);
        for _ in 0..60 {
            stats.absorb_tick(&metrics(100.0, 0), &snapshot(70.0, 4000, 95.0), &rules);
        }
        assert!(evaluate_stage(&stats, &rules).pass, "baseline must pass");

        stats.absorb_final(&fatal_metrics("gpu: device is lost"));
        stats.finish(&snapshot(70.0, 4000, 95.0));

        let verdict = evaluate_stage(&stats, &rules);
        assert!(!verdict.pass);
        assert_eq!(stats.ticks, 60, "absorb_final must not count as a tick");
        assert_eq!(stats.fatal_reason.as_deref(), Some("gpu: device is lost"));
    }

    #[test]
    fn absorb_final_folds_errors_and_last_error() {
        let rules = VerdictRules::default();
        let mut stats = stats_for(&rules);
        stats.absorb_tick(&metrics(100.0, 1), &snapshot(70.0, 4000, 95.0), &rules);
        stats.absorb_final(&metrics(100.0, 7));
        assert_eq!(stats.errors, 7);
        assert!(!stats.fatal_abort);

        stats.absorb_final(&fatal_metrics("disk thread 0: write failed"));
        assert_eq!(stats.last_error.as_deref(), Some("disk thread 0: write failed"));
        assert!(stats.fatal_abort);
    }

    /// A stage that measured no throughput never ran its load, so it cannot
    /// pass under any policy — the backstop for stressors that report no fatal.
    #[test]
    fn stage_with_no_throughput_sample_cannot_pass() {
        for rules in [VerdictRules::certification(), VerdictRules::default()] {
            let mut stats = stats_for(&rules);
            for _ in 0..30 {
                stats.absorb_tick(&metrics(0.0, 0), &snapshot(45.0, 4000, 3.0), &rules);
            }
            stats.finish(&snapshot(45.0, 4000, 3.0));
            assert!(!stats.observed_throughput);
            assert!(stats.produced_no_work());

            let verdict = evaluate_stage(&stats, &rules);
            assert!(!verdict.pass, "zero-work stage reported pass");
            assert!(
                verdict
                    .violations
                    .iter()
                    .any(|v| matches!(v, RuleViolation::NoThroughput { ticks: 30 })),
                "violations: {:?}",
                verdict.violations
            );
            let lines = verdict.violation_lines();
            assert!(
                lines.iter().any(|l| l.contains("inconclusive -")),
                "zero-work line must carry the inconclusive marker: {lines:?}"
            );
        }
    }

    /// Honest zero-throughput cases: a stage shorter than the sampling window,
    /// and one whose stressor needed time before its first sample.
    #[test]
    fn short_or_slow_starting_stages_are_not_failed_for_zero_work() {
        let rules = VerdictRules::certification();

        let mut short = stats_for(&rules);
        for _ in 0..(NO_WORK_MIN_TICKS - 1) {
            short.absorb_tick(&metrics(0.0, 0), &snapshot(45.0, 4000, 3.0), &rules);
        }
        assert!(!short.produced_no_work());
        assert!(evaluate_stage(&short, &rules).pass, "short stage failed for zero work");

        // Allocation / warm-up: 20 dead ticks, then the load reports.
        let mut warmup = stats_for(&rules);
        for _ in 0..20 {
            warmup.absorb_tick(&metrics(0.0, 0), &snapshot(45.0, 4000, 3.0), &rules);
        }
        for _ in 0..40 {
            warmup.absorb_tick(&metrics(100.0, 0), &snapshot(70.0, 4000, 95.0), &rules);
        }
        let verdict = evaluate_stage(&warmup, &rules);
        assert!(verdict.pass, "warm-up stage failed: {:?}", verdict.violations);
    }

    /// A sample that lands off the 1 Hz cadence still counts as measured work.
    #[test]
    fn off_cadence_throughput_sample_counts_as_work() {
        let rules = VerdictRules::certification();
        let mut stats = stats_for(&rules);
        for _ in 0..30 {
            stats.absorb_tick(&metrics(0.0, 0), &snapshot(45.0, 4000, 3.0), &rules);
        }
        stats.absorb_final(&metrics(250.0, 0));
        assert!(stats.observed_throughput);
        assert!(!stats.produced_no_work());
        assert!(evaluate_stage(&stats, &rules).pass);
    }

    /// A non-fatal `inconclusive -` message means the load never ran: the stage
    /// must stop reporting pass, and must not read as bad hardware.
    #[test]
    fn non_fatal_inconclusive_message_cannot_pass() {
        let msg = "psu_transient: inconclusive - this GPU is too slow to pulse at this rate";
        for rules in [VerdictRules::certification(), VerdictRules::default()] {
            let mut stats = StageStats::begin(
                0,
                "psu_transient",
                Stressor::PsuTransient,
                &snapshot(50.0, 4000, 90.0),
            );
            for _ in 0..40 {
                stats.absorb_tick(&warn_metrics(120.0, msg), &snapshot(70.0, 4000, 95.0), &rules);
            }
            stats.finish(&snapshot(70.0, 4000, 95.0));
            assert!(!stats.fatal_abort, "the stressor never went fatal");
            assert_eq!(stats.errors, 0, "no counted data errors");
            assert_eq!(stats.inconclusive_reason.as_deref(), Some(msg));

            let verdict = evaluate_stage(&stats, &rules);
            assert!(!verdict.pass, "inconclusive stage reported pass");
            let lines = verdict.violation_lines();
            assert!(
                lines.iter().any(|l| l.contains("inconclusive -")),
                "marker missing: {lines:?}"
            );
            for word in [
                "gpu unavailable",
                "gpu leg stopped",
                "device is lost",
                "device removed",
                "dxgi_error",
            ] {
                assert!(
                    !lines.iter().any(|l| l.to_ascii_lowercase().contains(word)),
                    "device-loss wording '{word}' in {lines:?}"
                );
            }
        }
    }

    /// Device-loss text stays hardware evidence: it is never filed as the
    /// stage's inconclusive reason, even carrying the marker.
    #[test]
    fn device_loss_message_is_not_filed_as_inconclusive() {
        let rules = VerdictRules::certification();
        let msg = "psu: inconclusive - GPU leg stopped (GPU device failed: Device is lost)";
        let mut stats = stats_for(&rules);
        stats.absorb_tick(&warn_metrics(100.0, msg), &snapshot(70.0, 4000, 95.0), &rules);
        assert!(stats.inconclusive_reason.is_none());
        assert!(!evaluate_stage(&stats, &rules)
            .violations
            .iter()
            .any(|v| matches!(v, RuleViolation::Inconclusive { .. })));
    }

    #[test]
    fn gpu_stage_without_gpu_telemetry_cannot_pass() {
        let cert = VerdictRules::certification();
        assert!(cert.max_gpu_temp_c.is_some(), "cert must configure a GPU temp rule");

        let mut stats = StageStats::begin(0, "gpu", Stressor::Gpu, &snapshot(50.0, 4000, 90.0));
        for _ in 0..30 {
            stats.absorb_tick(&metrics(100.0, 0), &snapshot(70.0, 4000, 95.0), &cert);
        }
        stats.finish(&snapshot(70.0, 4000, 95.0));

        let verdict = evaluate_stage(&stats, &cert);
        assert!(!verdict.pass, "a GPU limit that was never evaluable must not pass");
        assert!(verdict
            .violations
            .iter()
            .any(|v| matches!(v, RuleViolation::GpuTelemetryMissing { .. })));
    }

    #[test]
    fn gpu_stage_with_telemetry_passes_normally() {
        let cert = VerdictRules::certification();
        let snap = snapshot_with_gpu(70.0, 4000, 95.0, 60.0);
        let mut stats = StageStats::begin(0, "gpu", Stressor::Gpu, &snap);
        for _ in 0..30 {
            stats.absorb_tick(&metrics(100.0, 0), &snap, &cert);
        }
        stats.finish(&snap);

        let verdict = evaluate_stage(&stats, &cert);
        assert!(verdict.pass, "violations: {:?}", verdict.violations);
        assert_eq!(stats.gpu_temp_samples, 30);
    }

    #[test]
    fn cpu_stage_is_not_graded_on_absent_gpu_telemetry() {
        let cert = VerdictRules::certification();
        let mut stats = StageStats::begin(0, "cpu", Stressor::Cpu, &snapshot(50.0, 4000, 90.0));
        for _ in 0..30 {
            stats.absorb_tick(&metrics(100.0, 0), &snapshot(70.0, 4000, 95.0), &cert);
        }
        stats.finish(&snapshot(70.0, 4000, 95.0));

        let verdict = evaluate_stage(&stats, &cert);
        assert!(verdict.pass, "violations: {:?}", verdict.violations);
    }

    #[test]
    fn rail_droop_only_fails_when_opted_in() {
        let mut snap = snapshot_with_gpu(70.0, 4000, 95.0, 60.0);
        snap.voltages = vec![VoltageReading {
            label: "+12V".into(),
            volts: 10.9,
            calibrated: false,
        }];

        let cert = VerdictRules::certification();
        let mut off = StageStats::begin(0, "psu", Stressor::Psu, &snapshot(50.0, 4000, 90.0));
        for _ in 0..10 {
            off.absorb_tick(&metrics(100.0, 0), &snap, &cert);
        }
        assert!(evaluate_stage(&off, &cert).pass);
        assert_eq!(off.min_v12_v, Some(10.9));

        let opted = VerdictRules {
            min_v12_v: Some(RailDroopRule {
                floor_v: 11.4,
                consecutive_ticks: 5,
            }),
            ..cert
        };
        let mut on = StageStats::begin(0, "psu", Stressor::Psu, &snapshot(50.0, 4000, 90.0));
        for _ in 0..10 {
            on.absorb_tick(&metrics(100.0, 0), &snap, &opted);
        }
        let verdict = evaluate_stage(&on, &opted);
        assert!(!verdict.pass);
        assert!(verdict
            .violations
            .iter()
            .any(|v| matches!(v, RuleViolation::RailDroop { .. })));
    }
}
