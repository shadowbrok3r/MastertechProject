//! Thin wrapper over `stress_kit::telemetry::TelemetryAgent`. Keeps the
//! `HwSampler::start / get / stop` shape the rest of qc-app already calls into,
//! and re-exports `CoreSample` under the legacy `CoreRow` name so the egui
//! table impls don't churn.

use std::sync::Arc;

use stress_kit::telemetry::{TelemetryAgent, TelemetrySnapshot};

pub use stress_kit::telemetry::CoreSample as CoreRow;

/// Background telemetry collector. Owns the agent thread for its lifetime.
pub struct HwSampler {
    agent: Arc<TelemetryAgent>,
}

impl HwSampler {
    /// Poll interval in ms; clamped to ≥100 ms inside the agent.
    pub fn start(refresh_ms: u64) -> Self {
        Self {
            agent: Arc::new(TelemetryAgent::start(refresh_ms)),
        }
    }

    /// Clone of the latest per-core list. Empty until the first tick completes.
    pub fn get(&self) -> Vec<CoreRow> {
        self.agent.snapshot().cores
    }

    /// Full latest snapshot (cores + memory + disks + networks + WHEA).
    pub fn snapshot(&self) -> TelemetrySnapshot {
        self.agent.snapshot()
    }

    /// Shared handle so other subsystems (e.g. MCP) can read the same snapshot.
    pub fn agent(&self) -> Arc<TelemetryAgent> {
        self.agent.clone()
    }

    pub fn stop(&self) {
        self.agent.stop();
    }
}
