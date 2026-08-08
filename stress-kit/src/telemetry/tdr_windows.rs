//! Windows GPU TDR counter via the System event log (nvlddmkm/amdkmdap event 4101/4109).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use evtx::EvtxParser;
use serde::{Deserialize, Serialize};

const LOG_PATH: &str = r"C:\Windows\System32\Winevt\Logs\System.evtx";
const MIN_POLL_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TdrCounters {
    pub delta_since_program_start: u64,
    pub absolute_since_boot: u64,
}

pub struct TdrMonitor {
    path: PathBuf,
    baseline: u64,
    cached: TdrCounters,
    last_polled: Instant,
}

impl TdrMonitor {
    pub fn open() -> Option<Self> {
        let path = PathBuf::from(LOG_PATH);
        match count_tdr_events(&path) {
            Ok(baseline) => {
                log::debug!(
                    "stress-kit/tdr: opened {} (baseline = {} TDR events)",
                    path.display(),
                    baseline
                );
                Some(Self {
                    path,
                    baseline,
                    cached: TdrCounters {
                        delta_since_program_start: 0,
                        absolute_since_boot: baseline,
                    },
                    last_polled: Instant::now() - MIN_POLL_INTERVAL,
                })
            }
            Err(e) => {
                log::warn!(
                    "stress-kit/tdr: cannot open {}: {} — TDR counter disabled",
                    path.display(),
                    e
                );
                None
            }
        }
    }

    pub fn poll(&mut self) -> TdrCounters {
        if self.last_polled.elapsed() < MIN_POLL_INTERVAL {
            return self.cached.clone();
        }
        self.last_polled = Instant::now();

        match count_tdr_events(&self.path) {
            Ok(abs) => {
                self.cached = TdrCounters {
                    delta_since_program_start: abs.saturating_sub(self.baseline),
                    absolute_since_boot: abs,
                };
            }
            Err(e) => {
                log::debug!("stress-kit/tdr: scan failed: {e}");
            }
        }
        self.cached.clone()
    }
}

fn count_tdr_events(path: &PathBuf) -> Result<u64, String> {
    let mut parser = EvtxParser::from_path(path).map_err(|e| e.to_string())?;
    let mut count: u64 = 0;
    for r in parser.records_json_value() {
        let Ok(rec) = r else { continue };
        if is_tdr(&rec.data) {
            count += 1;
        }
    }
    Ok(count)
}

fn is_tdr(value: &serde_json::Value) -> bool {
    let Some(sys) = value.pointer("/Event/System") else { return false };
    let event_id = sys
        .get("EventID")
        .and_then(|v| match v {
            serde_json::Value::Number(n) => n.as_u64(),
            serde_json::Value::String(s) => s.parse().ok(),
            serde_json::Value::Object(o) => o.get("#text").and_then(|t| t.as_str()).and_then(|s| s.parse().ok()),
            _ => None,
        })
        .unwrap_or(0);
    if !matches!(event_id, 4101 | 4109) {
        return false;
    }
    let provider = sys
        .pointer("/Provider/#attributes/Name")
        .and_then(|v| v.as_str())
        .or_else(|| sys.pointer("/Provider/Name").and_then(|v| v.as_str()))
        .unwrap_or("")
        .to_lowercase();
    provider.contains("nvlddmkm")
        || provider.contains("amdkmdap")
        || provider.contains("amdkmdag")
        || provider.contains("igdkmd")
        || provider.contains("display")
}
