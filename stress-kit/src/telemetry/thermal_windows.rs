//! Windows ACPI thermal-zone reader.
//!
//! `sysinfo` used to surface per-component temperatures through its
//! `Components` API on Windows, but somewhere around 0.30 the upstream
//! crate dropped the underlying providers (the deprecated
//! `Win32_PerfRawData_Counters_ThermalZoneInformation` path), and on
//! modern Windows installs `Components::new_with_refreshed_list()`
//! returns an empty vector. Without per-component temps the admin
//! console's live charts can't render CPU/board thermal panels.
//!
//! WMI's `ROOT\WMI / MSAcpi_ThermalZoneTemperature` class still works
//! on every consumer machine I've tested without requiring elevation
//! or vendor drivers. Each `MSAcpi_ThermalZoneTemperature` instance
//! reports `CurrentTemperature` in tenths of a Kelvin, which we
//! convert to Celsius (and sanity-clamp to a plausible range so
//! garbage values from misbehaving BIOSes don't show as fictional
//! readings).
//!
//! Initialised once on the sampler thread (COM has to be initialised
//! on the same thread that calls into it), polled every tick with a
//! cached cooldown so the round-trip cost is bounded.

use serde::Deserialize;
use std::time::{Duration, Instant};
use wmi::WMIConnection;

use super::ThermalReading;

/// Minimum gap between underlying WMI queries. Telemetry agent ticks
/// at ~1 Hz so even a no-throttle implementation would be fine, but
/// 500 ms gives us headroom in case a future caller polls harder.
const MIN_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Plausible thermal range in °C for a consumer machine. Readings
/// outside this window are dropped — most often a thermal zone that
/// returns 0 K (BIOS placeholder) or `INT_MAX`-tenths-Kelvin (sensor
/// disconnected) shows up as something like -273.0 or 6553.5; both
/// would otherwise dominate the chart axes.
const MIN_PLAUSIBLE_C: f32 = -10.0;
const MAX_PLAUSIBLE_C: f32 = 200.0;

/// HRESULTs meaning the class does not exist on this machine, as they appear in
/// the WMI error text: `WBEM_E_NOT_SUPPORTED`, `WBEM_E_INVALID_CLASS`.
const UNSUPPORTED_HRESULTS: [&str; 2] = ["0X8004100C", "0X80041010"];

/// Gap before an "unsupported" HRESULT is retried, so a provider that answers
/// it transiently (WMI repository rebuild, re-registration) does not disable
/// ACPI zone telemetry for the rest of the run.
const UNSUPPORTED_RETRY_INTERVAL: Duration = Duration::from_secs(300);

#[derive(Deserialize, Debug, Clone)]
#[serde(rename = "MSAcpi_ThermalZoneTemperature")]
#[serde(rename_all = "PascalCase")]
#[allow(dead_code)]
struct MsAcpiThermalZone {
    /// e.g. `ACPI\\ThermalZone\\TZ00_0` or `ACPI\\ThermalZone\\CPUZ_0`.
    instance_name: String,
    /// Temperature in tenths of a Kelvin.
    current_temperature: u32,
    /// True when this zone is currently active and reading. We keep
    /// inactive zones around for hardware that only updates on demand
    /// — they'll show as stale until they're polled again, but at
    /// least the operator sees the label.
    active: bool,
}

/// Lives on the sampler thread; holds the WMI handle and a cache.
/// Not `Send` (WMIConnection isn't), which is fine — the telemetry
/// agent owns exactly one per process.
pub struct ThermalMonitor {
    wmi: WMIConnection,
    cached: Vec<ThermalReading>,
    last_polled: Instant,
    /// Text of the last query error warned about; repeats of it stay at debug.
    reported_error: Option<String>,
    /// Time of the last "class absent" HRESULT; queries pause until
    /// `UNSUPPORTED_RETRY_INTERVAL` has passed.
    unsupported_since: Option<Instant>,
}

impl ThermalMonitor {
    /// `None` when COM init or namespace connect fails — happens on
    /// Wine, in some sandboxed environments, or if Windows refuses
    /// the `ROOT\WMI` namespace. Logs a warning so the absence is
    /// visible. Also `None` when the very first query reports the
    /// class as absent, so callers see thermal as unavailable instead
    /// of permanently empty.
    ///
    /// `wmi` 0.18 handles `COMLibrary` lifecycle inside the connection
    /// constructor — callers no longer thread a `COMLibrary` through
    /// explicitly.
    pub fn open() -> Option<Self> {
        let wmi = match WMIConnection::with_namespace_path("ROOT\\WMI") {
            Ok(w) => w,
            Err(e) => {
                log::warn!(
                    "stress-kit/thermal: cannot connect to ROOT\\WMI: {e} — \
                     thermal monitor disabled"
                );
                return None;
            }
        };

        let mut me = Self {
            wmi,
            cached: Vec::new(),
            last_polled: Instant::now() - MIN_POLL_INTERVAL,
            reported_error: None,
            unsupported_since: None,
        };
        me.cached = me.read_zones();
        if me.unsupported_since.is_some() {
            return None;
        }
        log::debug!(
            "stress-kit/thermal: opened ROOT\\WMI ({} thermal zone(s))",
            me.cached.len()
        );
        Some(me)
    }

    /// Returns the latest readings. Throttled by `MIN_POLL_INTERVAL`;
    /// intermediate calls return the cached value. Failing a query
    /// keeps the previous cache rather than zeroing it out.
    pub fn poll(&mut self) -> Vec<ThermalReading> {
        if self.in_unsupported_backoff() || self.last_polled.elapsed() < MIN_POLL_INTERVAL {
            return self.cached.clone();
        }
        self.last_polled = Instant::now();
        let readings = self.read_zones();
        if !readings.is_empty() {
            self.cached = readings;
        }
        self.cached.clone()
    }

    /// True while an "unsupported" HRESULT is still inside its retry window.
    fn in_unsupported_backoff(&self) -> bool {
        self.unsupported_since
            .is_some_and(|t| t.elapsed() < UNSUPPORTED_RETRY_INTERVAL)
    }

    fn read_zones(&mut self) -> Vec<ThermalReading> {
        let zones: Vec<MsAcpiThermalZone> = match self.wmi.query() {
            Ok(z) => {
                self.unsupported_since = None;
                z
            }
            Err(e) => {
                self.report_query_failure(&e.to_string());
                return Vec::new();
            }
        };
        zones
            .into_iter()
            .map(|z| {
                let kelvin_tenths = z.current_temperature as f32;
                let temp_c = kelvin_tenths / 10.0 - 273.15;
                ThermalReading {
                    label: friendly_label(&z.instance_name),
                    temp_c,
                }
            })
            .filter(|r| r.temp_c >= MIN_PLAUSIBLE_C && r.temp_c <= MAX_PLAUSIBLE_C)
            .collect()
    }

    /// Warns once per distinct error text, and starts the retry backoff when the
    /// HRESULT says the class is absent.
    fn report_query_failure(&mut self, error: &str) {
        let unsupported = is_unsupported(error);
        if unsupported {
            self.unsupported_since = Some(Instant::now());
        }
        if self.reported_error.as_deref() == Some(error) {
            log::debug!("stress-kit/thermal: MSAcpi_ThermalZoneTemperature query failed: {error}");
            return;
        }
        self.reported_error = Some(error.to_string());
        if unsupported {
            log::warn!(
                "stress-kit/thermal: no ACPI thermal zones on this machine ({error}); thermal \
                 polling paused for {}s",
                UNSUPPORTED_RETRY_INTERVAL.as_secs()
            );
        } else {
            log::warn!("stress-kit/thermal: MSAcpi_ThermalZoneTemperature query failed: {error}");
        }
    }
}

/// True when the error text carries an HRESULT meaning the class is absent.
fn is_unsupported(error: &str) -> bool {
    let upper = error.to_ascii_uppercase();
    UNSUPPORTED_HRESULTS.iter().any(|h| upper.contains(h))
}

/// Trim the ACPI prefix off the instance name. `ACPI\\ThermalZone\\TZ00_0`
/// becomes `TZ00_0`, which is what shows up in BIOS / vendor docs.
fn friendly_label(instance: &str) -> String {
    instance
        .rsplit('\\')
        .next()
        .unwrap_or(instance)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permanent_hresults_latch_and_transient_ones_do_not() {
        assert!(is_unsupported("HRESULT Call failed with: 0x8004100C"));
        assert!(is_unsupported("hresult call failed with: 0x80041010"));
        assert!(!is_unsupported("HRESULT Call failed with: 0x80041033"));
        assert!(!is_unsupported("connection lost"));
    }
}
