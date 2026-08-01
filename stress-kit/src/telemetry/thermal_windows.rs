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
}

impl ThermalMonitor {
    /// `None` when COM init or namespace connect fails — happens on
    /// Wine, in some sandboxed environments, or if Windows refuses
    /// the `ROOT\WMI` namespace. Logs a warning so the absence is
    /// visible.
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

        let initial = read_zones(&wmi);
        log::info!(
            "stress-kit/thermal: opened ROOT\\WMI ({} thermal zone(s))",
            initial.len()
        );
        Some(Self {
            wmi,
            cached: initial,
            last_polled: Instant::now() - MIN_POLL_INTERVAL,
        })
    }

    /// Returns the latest readings. Throttled by `MIN_POLL_INTERVAL`;
    /// intermediate calls return the cached value. Failing a query
    /// keeps the previous cache rather than zeroing it out.
    pub fn poll(&mut self) -> Vec<ThermalReading> {
        if self.last_polled.elapsed() < MIN_POLL_INTERVAL {
            return self.cached.clone();
        }
        self.last_polled = Instant::now();
        let readings = read_zones(&self.wmi);
        if !readings.is_empty() {
            self.cached = readings;
        }
        self.cached.clone()
    }
}

fn read_zones(wmi: &WMIConnection) -> Vec<ThermalReading> {
    let zones: Vec<MsAcpiThermalZone> = match wmi.query() {
        Ok(z) => z,
        Err(e) => {
            log::warn!("stress-kit/thermal: MSAcpi_ThermalZoneTemperature query failed: {e}");
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

/// Trim the ACPI prefix off the instance name. `ACPI\\ThermalZone\\TZ00_0`
/// becomes `TZ00_0`, which is what shows up in BIOS / vendor docs.
fn friendly_label(instance: &str) -> String {
    instance
        .rsplit('\\')
        .next()
        .unwrap_or(instance)
        .to_string()
}
