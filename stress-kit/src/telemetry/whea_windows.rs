//! Windows Hardware Error Architecture (WHEA) error counter via the local
//! `Microsoft-Windows-WHEA-Logger/Operational` event log. On `start` we snapshot
//! the baseline event count; each `poll` returns `(delta, absolute)`.
//!
//! Scanning the .evtx file is not cheap, so callers should poll on a longer
//! interval than the main telemetry tick (~5 s is reasonable).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use evtx::EvtxParser;

use super::WheaCounters;

/// Default log file installed by Windows when the WHEA driver is enabled.
const LOG_PATH: &str = r"C:\Windows\System32\Winevt\Logs\Microsoft-Windows-WHEA-Logger%4Operational.evtx";
const MIN_POLL_INTERVAL: Duration = Duration::from_secs(5);

pub struct WheaMonitor {
    path: PathBuf,
    baseline: u64,
    cached: WheaCounters,
    last_polled: Instant,
}

impl WheaMonitor {
    /// `None` when the log file isn't readable (driver disabled, missing privileges,
    /// non-default locale path). Logs a one-time warning so the caller knows.
    pub fn open() -> Option<Self> {
        let path = PathBuf::from(LOG_PATH);
        match count_events(&path) {
            Ok(baseline) => {
                log::info!(
                    "stress-kit/whea: opened {} (baseline = {} events)",
                    path.display(),
                    baseline
                );
                Some(Self {
                    path,
                    baseline,
                    cached: WheaCounters {
                        delta_since_program_start: 0,
                        absolute_since_boot: baseline,
                    },
                    last_polled: Instant::now() - MIN_POLL_INTERVAL,
                })
            }
            Err(e) => {
                log::warn!(
                    "stress-kit/whea: cannot open {}: {} — WHEA counter disabled",
                    path.display(),
                    e
                );
                None
            }
        }
    }

    /// Returns the latest counters. Throttles the underlying file scan to
    /// `MIN_POLL_INTERVAL`; intermediate calls return the cached value.
    pub fn poll(&mut self) -> WheaCounters {
        if self.last_polled.elapsed() < MIN_POLL_INTERVAL {
            return self.cached.clone();
        }
        self.last_polled = Instant::now();

        match count_events(&self.path) {
            Ok(abs) => {
                self.cached = WheaCounters {
                    delta_since_program_start: abs.saturating_sub(self.baseline),
                    absolute_since_boot: abs,
                };
            }
            Err(e) => {
                log::debug!("stress-kit/whea: scan failed: {e}");
            }
        }
        self.cached.clone()
    }
}

fn count_events(path: &PathBuf) -> Result<u64, String> {
    let mut parser = EvtxParser::from_path(path).map_err(|e| e.to_string())?;
    let mut count: u64 = 0;
    for r in parser.records() {
        if r.is_ok() {
            count += 1;
        }
    }
    Ok(count)
}
