//! Per-stage verdict rules: configurable pass/fail policy evaluated from
//! 1 Hz tick data. Absent telemetry (WHEA/TDR/temps off-Windows) never
//! violates a rule, so dev runs pass vacuously.

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
        }
    }
}

/// One rule breach with enough detail for an operator-readable line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuleViolation {
    Whea { delta: u32 },
    Tdr { delta: u32 },
    StressorErrors { count: u64 },
    CpuTemp { limit_c: f32, peak_c: f32, sustained_ticks: u32 },
    GpuTemp { limit_c: f32, peak_c: f32, sustained_ticks: u32 },
    ClockCollapse { below_pct: f32, ticks: u32 },
    ThroughputUnstable { cv: f64, max_cv: f64 },
}

impl RuleViolation {
    pub fn describe(&self) -> String {
        match self {
            Self::Whea { delta } => format!("{delta} WHEA hardware error(s)"),
            Self::Tdr { delta } => format!("{delta} GPU driver reset(s) (TDR)"),
            Self::StressorErrors { count } => format!("{count} stressor data error(s)"),
            Self::CpuTemp { limit_c, peak_c, sustained_ticks } => format!(
                "CPU over {limit_c:.0}C for {sustained_ticks}s (peak {peak_c:.1}C)"
            ),
            Self::GpuTemp { limit_c, peak_c, sustained_ticks } => format!(
                "GPU over {limit_c:.0}C for {sustained_ticks}s (peak {peak_c:.1}C)"
            ),
            Self::ClockCollapse { below_pct, ticks } => format!(
                "clock under {:.0}% of stage max for {ticks}s",
                below_pct * 100.0
            ),
            Self::ThroughputUnstable { cv, max_cv } => format!(
                "throughput CV {cv:.3} over band {max_cv:.3}"
            ),
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
    pub max_avg_clock_mhz: Option<u32>,
    cpu_temp_over_run: u32,
    pub worst_cpu_temp_over: u32,
    gpu_temp_over_run: u32,
    pub worst_gpu_temp_over: u32,
    collapse_run: u32,
    pub worst_collapse_run: u32,
    pub peak_throughput: Option<f64>,
    tp_sum: f64,
    tp_sum_sq: f64,
    pub tp_samples: u32,
    pub errors: u64,
    whea_baseline: u32,
    pub whea_delta: u32,
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
            max_avg_clock_mhz: None,
            cpu_temp_over_run: 0,
            worst_cpu_temp_over: 0,
            gpu_temp_over_run: 0,
            worst_gpu_temp_over: 0,
            collapse_run: 0,
            worst_collapse_run: 0,
            peak_throughput: None,
            tp_sum: 0.0,
            tp_sum_sq: 0.0,
            tp_samples: 0,
            errors: 0,
            whea_baseline: whea_count(snapshot),
            whea_delta: 0,
            tdr_baseline: tdr_count(snapshot),
            tdr_delta: 0,
            gpu_throttle_ticks: 0,
        }
    }

    /// Fold one ~1 Hz tick of stressor metrics + telemetry.
    pub fn absorb_tick(
        &mut self,
        metrics: &Metrics,
        snapshot: &TelemetrySnapshot,
        rules: &VerdictRules,
    ) {
        self.ticks = self.ticks.saturating_add(1);

        // `Metrics.errors` is cumulative within the stage's stressor.
        self.errors = self.errors.max(metrics.errors);

        self.whea_delta = whea_count(snapshot).saturating_sub(self.whea_baseline);
        self.tdr_delta = tdr_count(snapshot).saturating_sub(self.tdr_baseline);

        let tick_max_cpu_temp = snapshot
            .cores
            .iter()
            .filter_map(|c| c.temp_c)
            .fold(None::<f32>, |acc, t| Some(acc.map_or(t, |m| m.max(t))));
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
        }
        if let Some(rule) = &rules.max_gpu_temp_c {
            track_over_run(
                tick_max_gpu_temp,
                rule.limit_c,
                &mut self.gpu_temp_over_run,
                &mut self.worst_gpu_temp_over,
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

fn whea_count(snapshot: &TelemetrySnapshot) -> u32 {
    snapshot
        .whea
        .as_ref()
        .map(|w| w.delta_since_program_start as u32)
        .unwrap_or(0)
}

fn tdr_count(snapshot: &TelemetrySnapshot) -> u32 {
    snapshot
        .tdr
        .as_ref()
        .map(|t| t.delta_since_program_start as u32)
        .unwrap_or(0)
}

/// Stressors whose tick throughput is bursty or pattern-phased by design;
/// CV never applies.
fn cv_exempt(stressor: Stressor) -> bool {
    matches!(
        stressor,
        Stressor::Psu | Stressor::Disk | Stressor::MemTest | Stressor::GpuVram
    )
}

/// Evaluate one finished stage against the rules.
pub fn evaluate_stage(stats: &StageStats, rules: &VerdictRules) -> StageVerdict {
    let mut violations = Vec::new();

    if rules.whea_fails && stats.whea_delta > 0 {
        violations.push(RuleViolation::Whea { delta: stats.whea_delta });
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
        if stats.worst_gpu_temp_over >= rule.consecutive_ticks {
            violations.push(RuleViolation::GpuTemp {
                limit_c: rule.limit_c,
                peak_c: stats.max_gpu_temp_c.unwrap_or(rule.limit_c),
                sustained_ticks: stats.worst_gpu_temp_over,
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stress_kit::telemetry::{CoreSample, TdrCounters, WheaCounters};

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

    fn metrics(throughput: f64, errors: u64) -> Metrics {
        Metrics {
            elapsed_secs: 1.0,
            throughput,
            errors,
            ..Default::default()
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
            ..Default::default()
        });
        stats.absorb_tick(&metrics(100.0, 0), &snap, &rules);
        stats.finish(&snap);
        let verdict = evaluate_stage(&stats, &rules);
        assert!(!verdict.pass);
        assert!(matches!(verdict.violations[0], RuleViolation::Whea { delta: 2 }));

        // No whea/tdr source at all: vacuous pass.
        let mut bare = stats_for(&rules);
        bare.absorb_tick(&metrics(100.0, 0), &snapshot(70.0, 4000, 95.0), &rules);
        bare.finish(&snapshot(70.0, 4000, 95.0));
        assert!(evaluate_stage(&bare, &rules).pass);
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
    }
}
