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

fn event_id(value: &serde_json::Value) -> Option<u64> {
    match value {
        serde_json::Value::Number(n) => n.as_u64(),
        serde_json::Value::String(s) => s.parse().ok(),
        serde_json::Value::Object(o) => o
            .get("#text")
            .and_then(|t| t.as_u64().or_else(|| t.as_str()?.parse().ok())),
        _ => None,
    }
}

fn is_tdr(value: &serde_json::Value) -> bool {
    let Some(sys) = value.pointer("/Event/System") else {
        return false;
    };
    let event_id = sys.get("EventID").and_then(event_id).unwrap_or(0);
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn record(event_id: Value, provider: &str) -> Value {
        json!({
            "Event": {
                "System": {
                    "Provider": { "#attributes": { "Name": provider } },
                    "EventID": event_id,
                }
            }
        })
    }

    #[test]
    fn event_id_number() {
        assert_eq!(event_id(&json!(4101)), Some(4101));
    }

    #[test]
    fn event_id_string() {
        assert_eq!(event_id(&json!("4101")), Some(4101));
    }

    #[test]
    fn event_id_object_with_number() {
        let v = json!({ "#attributes": { "Qualifiers": 0 }, "#text": 4101 });
        assert_eq!(event_id(&v), Some(4101));
    }

    #[test]
    fn event_id_object_with_string() {
        let v = json!({ "#attributes": { "Qualifiers": "0" }, "#text": "4101" });
        assert_eq!(event_id(&v), Some(4101));
    }

    #[test]
    fn event_id_rejects_unparseable() {
        assert_eq!(event_id(&json!({ "#text": "abc" })), None);
        assert_eq!(
            event_id(&json!({ "#attributes": { "Qualifiers": 0 } })),
            None
        );
        assert_eq!(event_id(&json!(null)), None);
        assert_eq!(event_id(&json!(-1)), None);
    }

    #[test]
    fn is_tdr_accepts_every_event_id_shape() {
        let shapes = [
            json!(4101),
            json!("4101"),
            json!({ "#attributes": { "Qualifiers": 0 }, "#text": 4101 }),
            json!({ "#attributes": { "Qualifiers": "0" }, "#text": "4101" }),
        ];
        for shape in shapes {
            assert!(is_tdr(&record(shape.clone(), "Display")), "{shape}");
        }
    }

    #[test]
    fn is_tdr_accepts_4109_from_gpu_drivers() {
        for provider in ["nvlddmkm", "amdkmdag", "amdkmdap", "igdkmd64"] {
            assert!(is_tdr(&record(json!(4109), provider)), "{provider}");
        }
    }

    #[test]
    fn is_tdr_rejects_other_display_events() {
        let v = json!({ "#attributes": { "Qualifiers": 0 }, "#text": 4107 });
        assert!(!is_tdr(&record(v, "Display")));
    }

    #[test]
    fn is_tdr_rejects_non_gpu_provider() {
        let v = json!({ "#attributes": { "Qualifiers": 0 }, "#text": 4101 });
        assert!(!is_tdr(&record(v, "Microsoft-Windows-Kernel-Power")));
    }

    #[test]
    fn is_tdr_rejects_record_without_system() {
        assert!(!is_tdr(&json!({ "Event": {} })));
    }
}
