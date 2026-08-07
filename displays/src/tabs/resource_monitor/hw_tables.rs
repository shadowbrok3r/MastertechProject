use std::collections::HashMap;

use eframe::egui::{self, RichText, Ui, vec2};
use egui_extras::{Column, TableBuilder};
use stress_kit::telemetry::{
    CpuTempCoverage, CpuTempSource, DiskRateSample, GpuSample, MemorySample, NetworkRateSample,
    ProcessSample, TelemetrySnapshot, VoltageReading, WheaStatus, expected_rail_labels,
};

use super::widgets::{
    self, Absent, Gauge, Meter, Ramp, Reading, StatTile, Status, StatusPill, GAUGE_SIZE,
    METER_SIZE, PILL_SIZE, TILE_SIZE,
};
use super::sysinfo_convert::fmt_bytes;
use super::{ABSENT, TelemetrySource};
use crate::ui_tools::{glass_card, icons, theme};

const ROW_HEIGHT: f32 = 18.0;

// Categorical slots in the validated order; 1 (red) and 5 (green) stay with status.
const SERIES_DISK: usize = 0;
const SERIES_NET: usize = 2;
const SERIES_RAIL: usize = 4;

/// Per-core CPU readings keyed by the index in their `CPU Core N` label.
fn per_core_by_index(snapshot: &TelemetrySnapshot) -> HashMap<usize, f32> {
    snapshot
        .cpu_core_readings()
        .into_iter()
        .filter_map(|r| {
            let idx = r
                .label
                .get("CPU Core ".len()..)?
                .trim()
                .parse::<usize>()
                .ok()?;
            Some((idx, r.temp_c))
        })
        .collect()
}

/// Package/Tctl reading for a section header, e.g. `CPU (Tctl) 52.4 °C`.
pub fn cpu_package_summary(snapshot: &TelemetrySnapshot) -> Option<String> {
    snapshot
        .cpu_package_reading()
        .map(|r| format!("{} {:.1} °C", r.label, r.temp_c))
}

/// Mean load across the sampled cores; `None` when no core was sampled.
pub fn cpu_load_pct(snapshot: &TelemetrySnapshot) -> Option<f32> {
    if snapshot.cores.is_empty() {
        return None;
    }
    let sum: f32 = snapshot.cores.iter().map(|c| c.usage_pct).sum();
    Some(sum / snapshot.cores.len() as f32)
}

/// Busiest GPU load; `None` when no adapter reported a utilisation figure.
pub fn gpu_load_pct(gpus: &[GpuSample]) -> Option<f32> {
    gpus.iter()
        .filter_map(|g| g.usage_pct)
        .fold(None::<f32>, |acc, u| Some(acc.map_or(u, |m| m.max(u))))
}

/// RAM used percentage; `None` until a memory total is known.
pub fn ram_used_pct(m: &MemorySample) -> Option<f32> {
    (m.total_mb > 0).then_some(m.used_pct)
}

/// A GPU die temperature; the wire payload's 0.0 stands for no sensor.
pub fn gpu_temp_c(g: &GpuSample) -> Option<f32> {
    g.temp_c.filter(|t| *t > 0.0)
}

/// True when the snapshot carries page-file figures at all.
pub fn page_file_measured(m: &MemorySample) -> bool {
    m.page_file_total_mb > 0
}

// ── Layout helpers ──────────────────────────────────────────────────────────

/// Titled dashboard panel: a themed glass card that frosts its own backdrop.
pub fn panel<R>(ui: &mut Ui, glyph: &str, title: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    glass_card::titled_card(ui, glyph, title, None, |ui| {
        ui.add_space(2.0);
        add(ui)
    })
}

/// Small muted line under a widget group.
pub fn caption(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).small().color(theme::weak_text(ui)));
}

/// Full-width meter size for the current layout column.
fn meter_size(ui: &Ui) -> egui::Vec2 {
    vec2(ui.available_width().max(120.0), METER_SIZE.y)
}

/// Pill sized to its label, never narrower than the widget default.
fn pill_size(ui: &Ui, label: &str) -> egui::Vec2 {
    let width = (label.chars().count() as f32 * 6.6 + 34.0).min(ui.available_width().max(120.0));
    vec2(width.max(PILL_SIZE.x), PILL_SIZE.y)
}

// ── Dashboard sections ──────────────────────────────────────────────────────

/// CPU load gauge over the sampled cores.
pub fn cpu_load_gauge(ui: &mut Ui, snapshot: &TelemetrySnapshot) {
    widgets::circular_gauge(
        ui,
        GAUGE_SIZE,
        &Gauge {
            label: "CPU load",
            value: Reading::new(cpu_load_pct(snapshot), Absent::NotSampled),
            ..Default::default()
        },
    );
}

/// RAM used gauge.
pub fn ram_gauge(ui: &mut Ui, m: &MemorySample) {
    widgets::circular_gauge(
        ui,
        GAUGE_SIZE,
        &Gauge {
            label: "RAM used",
            value: Reading::new(ram_used_pct(m), Absent::NotSampled),
            ..Default::default()
        },
    );
}

/// Busiest-GPU load gauge; an adapter with no utilisation counter reads absent.
pub fn gpu_load_gauge(ui: &mut Ui, gpus: &[GpuSample]) {
    widgets::circular_gauge(
        ui,
        GAUGE_SIZE,
        &Gauge {
            label: "GPU load",
            value: Reading::new(gpu_load_pct(gpus), Absent::NoSensor),
            ..Default::default()
        },
    );
}

/// The one CPU temperature this snapshot can report, with its sensor class.
pub fn cpu_temp_tile(ui: &mut Ui, snapshot: &TelemetrySnapshot) {
    let reading = snapshot.cpu_temp_reading();
    let value = Reading::new(reading.as_ref().map(|(r, _)| r.temp_c), Absent::NoSensor);
    let sub = match reading.as_ref() {
        Some((r, CpuTempSource::AcpiZone)) => format!("{} · ACPI zone, not the die", r.label),
        Some((r, _)) => format!("{} · die sensor", r.label),
        None => String::new(),
    };
    widgets::stat_tile(
        ui,
        TILE_SIZE,
        &StatTile {
            caption: "CPU temp",
            value,
            unit: "°C",
            decimals: 1,
            sub: Some(sub.as_str()),
            status: Some(Status::from_temp_c(value)),
            ..Default::default()
        },
    );
}

/// Process count tile; the wire payload's own process list is the source.
pub fn process_count_tile(ui: &mut Ui, snapshot: &TelemetrySnapshot) {
    let value = if snapshot.processes.is_empty() {
        Reading::NOT_SAMPLED
    } else {
        Reading::Measured(snapshot.processes.len() as f32)
    };
    widgets::stat_tile(
        ui,
        TILE_SIZE,
        &StatTile {
            caption: "Processes",
            value,
            unit: "",
            decimals: 0,
            sub: Some("tracked this tick"),
            ..Default::default()
        },
    );
}

/// Status and wording for the WHEA state; one rule for the pill and the table.
fn whea_status_parts(snapshot: &TelemetrySnapshot) -> (Status, String) {
    match snapshot.whea_status() {
        WheaStatus::Unavailable => (
            Status::NotMeasured,
            "WHEA not measured — event source would not open".to_string(),
        ),
        WheaStatus::NotSampled => (
            Status::NotMeasured,
            "WHEA not measured — never sampled on this path".to_string(),
        ),
        WheaStatus::Read => match snapshot.whea_counters() {
            None => (Status::NotMeasured, "WHEA not measured".to_string()),
            Some(w) if w.fatal_delta > 0 => (
                Status::Critical,
                format!("{} fatal WHEA error(s) since start", w.fatal_delta),
            ),
            Some(w) if w.delta_since_program_start > 0 || w.corrected_delta > 0 => (
                Status::Warn,
                format!(
                    "{} WHEA error(s) since start",
                    w.delta_since_program_start.max(w.corrected_delta)
                ),
            ),
            Some(_) => (
                Status::Good,
                "no WHEA errors since program start".to_string(),
            ),
        },
    }
}

/// WHEA state as a pill plus the retained total when the log was read.
pub fn show_whea_pill(ui: &mut Ui, snapshot: &TelemetrySnapshot) {
    let (status, label) = whea_status_parts(snapshot);
    let size = pill_size(ui, &label);
    widgets::status_pill(ui, size, &StatusPill { status, label: &label });
    match snapshot.whea_counters() {
        Some(w) => caption(
            ui,
            &format!(
                "{} retained in the log (spans reboots) · {} corrected · {} fatal",
                w.total_retained, w.corrected_delta, w.fatal_delta
            ),
        ),
        None => caption(
            ui,
            "No machine-check count, so errors were neither seen nor ruled out.",
        ),
    }
}

/// RAM and page-file meters; a page file nothing measured reads absent, not 0%.
pub fn show_memory_panel(ui: &mut Ui, m: &MemorySample, source: TelemetrySource) {
    let size = meter_size(ui);
    let ram_text = format!("{} / {} MB", m.used_mb, m.total_mb);
    widgets::linear_meter(
        ui,
        size,
        &Meter {
            label: "RAM",
            value: Reading::new(ram_used_pct(m), Absent::NotSampled),
            unit: "",
            value_text: (m.total_mb > 0).then_some(ram_text.as_str()),
            ..Default::default()
        },
    );

    let pf_text = format!("{} / {} MB", m.page_file_used_mb, m.page_file_total_mb);
    let pf_measured = page_file_measured(m);
    widgets::linear_meter(
        ui,
        size,
        &Meter {
            label: "Page file",
            value: if pf_measured {
                Reading::Measured(m.page_file_used_pct)
            } else {
                Reading::NOT_SAMPLED
            },
            unit: "",
            value_text: pf_measured.then_some(pf_text.as_str()),
            ..Default::default()
        },
    );
    if !pf_measured {
        caption(
            ui,
            "No page-file figures in this snapshot — the wire payload does not carry them.",
        );
    }

    ui.horizontal(|ui| {
        ui.label(RichText::new("vmmem").small());
        ui.add_space(6.0);
        match (m.vmmem_mb, source) {
            (Some(mb), _) => {
                ui.label(
                    RichText::new(format!("{mb} MB resident (WSL / Hyper-V)"))
                        .small()
                        .monospace()
                        .color(theme::strong_text(ui)),
                );
            }
            (None, TelemetrySource::Local) => {
                ui.label(
                    RichText::new("not running")
                        .small()
                        .color(theme::weak_text(ui)),
                );
            }
            (None, TelemetrySource::Wire) => {
                ui.label(
                    RichText::new("not reported by this payload")
                        .small()
                        .color(theme::weak_text(ui)),
                );
            }
        }
    });
}

/// Per-disk throughput meters, scaled to the busiest device in the tick.
pub fn show_disk_meters(ui: &mut Ui, disks: &[DiskRateSample], source: TelemetrySource) {
    if disks.is_empty() {
        empty_state(ui, "No disks reported in the latest snapshot.");
        return;
    }

    let size = meter_size(ui);
    let rates_measured = source.io_rates_measured();
    let mut any_capacity = false;

    for d in disks {
        // Filesystem and throughput ride the row, so no separate volume table is needed.
        let mut label = d.name.clone();
        if !d.file_system.trim().is_empty() {
            label.push_str(" · ");
            label.push_str(d.file_system.trim());
        }

        let used = d.used_fraction();
        any_capacity |= used.is_some();

        let mut detail = match used {
            Some(_) => format!(
                "{} free of {}",
                fmt_bytes(d.available_bytes),
                fmt_bytes(d.total_bytes)
            ),
            None => "capacity not reported".to_string(),
        };
        if rates_measured {
            detail.push_str(&format!(
                " · {:.2} R / {:.2} W MB/s",
                d.read_mb_per_s, d.write_mb_per_s
            ));
        }

        widgets::linear_meter(
            ui,
            size,
            &Meter {
                label: &label,
                value: match used {
                    Some(frac) => Reading::Measured(frac * 100.0),
                    None => Reading::NOT_SAMPLED,
                },
                unit: "%",
                decimals: 0,
                range: (0.0, 100.0),
                ramp: Ramp::Series(SERIES_DISK),
                value_text: Some(detail.as_str()),
            },
        );
    }

    if !any_capacity {
        caption(ui, "No volume reported a capacity, so no usage could be shown.");
    } else if !rates_measured {
        caption(ui, "Bars show capacity used; the payload carried no I/O counters.");
    } else {
        caption(ui, "Bars show capacity used.");
    }
}

/// Per-adapter meters; wire-payload volume is labelled cumulative, not a rate.
pub fn show_network_meters(ui: &mut Ui, nets: &[NetworkRateSample], source: TelemetrySource) {
    if nets.is_empty() {
        empty_state(ui, "No network interfaces reported.");
        return;
    }

    let size = meter_size(ui);
    let rates = source.io_rates_measured();
    let peak = nets
        .iter()
        .map(|n| n.rx_mbps + n.tx_mbps)
        .fold(0.0_f32, f32::max);
    for n in nets {
        let text = format!("{:.2} rx / {:.2} tx", n.rx_mbps, n.tx_mbps);
        widgets::linear_meter(
            ui,
            size,
            &Meter {
                label: &n.name,
                value: Reading::Measured(n.rx_mbps + n.tx_mbps),
                unit: if rates { "Mbps" } else { "MB total" },
                decimals: 2,
                range: (0.0, peak.max(1.0)),
                ramp: Ramp::Series(SERIES_NET),
                value_text: Some(text.as_str()),
            },
        );
    }
    if rates {
        caption(ui, &format!("Bars scaled to the busiest adapter, {peak:.2} Mbps."));
    } else {
        caption(
            ui,
            "The wire payload carries cumulative volume since the client started, not a rate.",
        );
    }
}

/// Nominal volts for the fixed rails; `Vcore` tracks the workload and has none.
fn rail_nominal(label: &str) -> Option<f32> {
    match label.to_ascii_lowercase().as_str() {
        "+12v" => Some(12.0),
        "+5v" => Some(5.0),
        "3vcc (chip)" => Some(3.3),
        "vbat" => Some(3.0),
        _ => None,
    }
}

/// Every publishable rail, measured or not: volts, minimum seen, calibration.
pub fn show_rail_meters(
    ui: &mut Ui,
    snapshot: &TelemetrySnapshot,
    minimums: &HashMap<String, f32>,
) {
    let size = meter_size(ui);
    for label in expected_rail_labels() {
        let reading = snapshot.rail_reading(label);
        let nominal = rail_nominal(label);
        let uncalibrated = reading.is_some_and(|v| !v.calibrated);
        let name = if uncalibrated {
            format!("{label} {}", icons::STATUS_WARN)
        } else {
            (*label).to_string()
        };
        let text = match (reading, minimums.get(*label)) {
            (Some(v), Some(min)) => format!("{:.3} V · min {min:.3}", v.volts),
            (Some(v), None) => format!("{:.3} V · min {ABSENT}", v.volts),
            (None, _) => String::new(),
        };
        widgets::linear_meter(
            ui,
            size,
            &Meter {
                label: &name,
                value: Reading::new(reading.map(|v| v.volts), Absent::NotSampled),
                unit: "",
                decimals: 3,
                range: (0.0, nominal.unwrap_or(1.6) * 1.25),
                ramp: Ramp::Series(SERIES_RAIL),
                value_text: (!text.is_empty()).then_some(text.as_str()),
            },
        );
    }

    if snapshot.rails().is_empty() {
        caption(
            ui,
            "No board rails in this snapshot. Locally they need a kernel-mode sensor backend to \
             reach the SuperIO chip; remote client telemetry does not carry rails yet.",
        );
        return;
    }
    caption(
        ui,
        "Bars span 1.25× nominal (Vcore 0–2 V, no nominal) · min is the lowest volt seen \
         since telemetry started; droop is the diagnostic signal.",
    );
    if snapshot.any_uncalibrated_rails() {
        let label = format!(
            "{} rail(s) uncalibrated — nominal divider assumed",
            snapshot.rails().iter().filter(|v| !v.calibrated).count()
        );
        let pill = pill_size(ui, &label);
        widgets::status_pill(
            ui,
            pill,
            &StatusPill {
                status: Status::Warn,
                label: &label,
            },
        );
        caption(
            ui,
            "An uncalibrated absolute value can be off; compare a rail against its own droop, \
             not the nominal spec.",
        );
    }
    if let Some(chip) = snapshot.rail_reading("3VCC (chip)") {
        caption(
            ui,
            &format!(
                "3VCC is the SuperIO sensor chip's own supply ({:.3} V), not the board's +3.3V \
                 PSU rail.",
                chip.volts
            ),
        );
    }
}

/// Per-GPU load and VRAM meters with an honest temperature pill.
pub fn show_gpu_panel(ui: &mut Ui, gpus: &[GpuSample]) {
    if gpus.is_empty() {
        empty_state(ui, "No GPU sensors visible to sysinfo.");
        return;
    }

    for g in gpus {
        ui.label(
            RichText::new(format!("{} · {}", g.vendor, g.name))
                .small()
                .strong(),
        );

        let temp = gpu_temp_c(g);
        let temp_label = match temp {
            Some(t) => format!("{t:.1} °C"),
            None => "no thermal sensor".to_string(),
        };
        let pill = pill_size(ui, &temp_label);
        widgets::status_pill(
            ui,
            pill,
            &StatusPill {
                status: Status::from_temp_c(Reading::new(temp, Absent::NoSensor)),
                label: &temp_label,
            },
        );

        let size = meter_size(ui);
        widgets::linear_meter(
            ui,
            size,
            &Meter {
                label: "load",
                value: Reading::new(g.usage_pct, Absent::NoSensor),
                ..Default::default()
            },
        );

        let vram_pct = match (g.memory_used_mb, g.memory_total_mb) {
            (Some(used), Some(total)) if total > 0 => Some((used as f32 / total as f32) * 100.0),
            _ => None,
        };
        let vram_text = match (g.memory_used_mb, g.memory_total_mb) {
            (Some(used), Some(total)) => format!("{used} / {total} MB"),
            (Some(used), None) => format!("{used} MB, no total"),
            _ => String::new(),
        };
        widgets::linear_meter(
            ui,
            size,
            &Meter {
                label: "VRAM",
                value: Reading::new(vram_pct, Absent::NoSensor),
                unit: "",
                value_text: (!vram_text.is_empty()).then_some(vram_text.as_str()),
                ..Default::default()
            },
        );

        if let Some(w) = g.power_w {
            caption(ui, &format!("{w:.0} W draw"));
        }
        if !g.throttle_reasons.is_empty() {
            let label = format!("throttling: {}", g.throttle_reasons.join(", "));
            let pill = pill_size(ui, &label);
            widgets::status_pill(
                ui,
                pill,
                &StatusPill {
                    status: Status::Warn,
                    label: &label,
                },
            );
        }
        ui.add_space(4.0);
    }
}

/// Per-core load meters with each core's own die reading where one exists.
pub fn show_core_meters(ui: &mut Ui, snapshot: &TelemetrySnapshot, columns: usize) {
    if snapshot.cores.is_empty() {
        empty_state(ui, "No cores in the latest snapshot.");
        return;
    }

    if let Some(pkg) = cpu_package_summary(snapshot) {
        caption(ui, &format!("Package {pkg}"));
    }

    let per_core = per_core_by_index(snapshot);
    let columns = columns.max(1);
    let rows = snapshot.cores.len().div_ceil(columns);
    ui.columns(columns, |cols| {
        for (c, col) in cols.iter_mut().enumerate() {
            let size = meter_size(col);
            for core in snapshot.cores.iter().skip(c * rows).take(rows) {
                let temp = core.temp_c.or_else(|| per_core.get(&core.index).copied());
                let text = match temp {
                    Some(t) => format!("{:.0}% · {t:.0} °C", core.usage_pct),
                    None => format!("{:.0}% · {} MHz", core.usage_pct, core.freq_mhz),
                };
                widgets::linear_meter(
                    col,
                    size,
                    &Meter {
                        label: &format!("C{}", core.index),
                        value: Reading::Measured(core.usage_pct),
                        unit: "",
                        value_text: Some(text.as_str()),
                        ..Default::default()
                    },
                );
            }
        }
    });

    if !snapshot.has_per_core_cpu_temps() {
        caption(ui, per_core_absence_reason(snapshot));
    }
}

// ── Tables ──────────────────────────────────────────────────────────────────

pub fn show_cores(ui: &mut egui::Ui, snapshot: &TelemetrySnapshot, filter: &str) {
    let filter_lower = filter.to_lowercase();
    let visible: Vec<_> = snapshot
        .cores
        .iter()
        .filter(|c| {
            filter.is_empty()
                || c.name.to_lowercase().contains(&filter_lower)
                || c.brand.to_lowercase().contains(&filter_lower)
        })
        .collect();

    // Whole-CPU package/Tctl reading, once above the per-core rows.
    ui.horizontal(|ui| {
        ui.label(RichText::new("Package").strong());
        match snapshot.cpu_package_reading() {
            Some(r) => {
                ui.colored_label(
                    temp_color(ui, Some(r.temp_c)),
                    format!("{:.1} °C", r.temp_c),
                );
                ui.label(RichText::new(&r.label).small().weak());
                if snapshot.cpu_temp_source() == CpuTempSource::AcpiZone {
                    ui.label(
                        RichText::new("board ACPI zone, not the CPU die")
                            .small()
                            .weak(),
                    );
                }
            }
            None => {
                ui.colored_label(theme::weak_text(ui), "no package sensor readable");
            }
        }
    });

    if visible.is_empty() {
        empty_state(ui, "No cores match the filter.");
        return;
    }

    let per_core = per_core_by_index(snapshot);
    // Off-Windows sysinfo fills CoreSample::temp_c while the WMI thermal list is empty.
    let has_per_core =
        snapshot.has_per_core_cpu_temps() || visible.iter().any(|c| c.temp_c.is_some());
    if !has_per_core {
        ui.colored_label(theme::weak_text(ui), per_core_absence_reason(snapshot));
    }

    let mut table = TableBuilder::new(ui)
        .striped(true)
        .resizable(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::auto().at_least(36.0).at_most(60.0))
        .column(Column::auto().at_least(60.0).at_most(110.0))
        .column(Column::initial(280.0).at_least(180.0))
        .column(Column::auto().at_least(80.0))
        .column(Column::auto().at_least(100.0));
    if has_per_core {
        table = table.column(Column::auto().at_least(80.0));
    }
    table
        .header(20.0, |mut h| {
            h.col(|ui| header_label(ui, "#"));
            h.col(|ui| header_label(ui, "Name"));
            h.col(|ui| header_label(ui, "Brand"));
            h.col(|ui| header_label(ui, "Usage %"));
            h.col(|ui| header_label(ui, "Freq"));
            if has_per_core {
                h.col(|ui| header_label(ui, "Temp"));
            }
        })
        .body(|mut body| {
            for c in visible {
                body.row(ROW_HEIGHT, |mut r| {
                    r.col(|ui| {
                        ui.colored_label(
                            ui.style().visuals.warn_fg_color,
                            format!("{}", c.index),
                        );
                    });
                    r.col(|ui| {
                        ui.label(&c.name);
                    });
                    r.col(|ui| {
                        ui.label(&c.brand);
                    });
                    r.col(|ui| {
                        ui.colored_label(
                            usage_color(ui, c.usage_pct),
                            format!("{:.1}%", c.usage_pct),
                        );
                    });
                    r.col(|ui| {
                        ui.label(format!("{} MHz", c.freq_mhz));
                    });
                    if has_per_core {
                        let temp = c.temp_c.or_else(|| per_core.get(&c.index).copied());
                        r.col(|ui| match temp {
                            Some(t) => {
                                ui.colored_label(temp_color(ui, Some(t)), format!("{t:.1} °C"));
                            }
                            None => {
                                ui.colored_label(theme::weak_text(ui), "no per-core sensor");
                            }
                        });
                    }
                });
            }
        });
}

/// Why no per-core temperature shows; no coverage outranks brand inference.
fn per_core_absence_reason(snapshot: &TelemetrySnapshot) -> &'static str {
    if snapshot.cpu_temp_coverage() == CpuTempCoverage::None {
        return "No CPU temperature sensors readable on this platform.";
    }
    let brand = snapshot
        .cores
        .first()
        .map(|c| c.brand.to_lowercase())
        .unwrap_or_default();
    let amd = brand.contains("amd") || brand.contains("ryzen") || brand.contains("threadripper");
    if amd {
        "No per-core sensors: AMD Zen exposes only the package Tctl through the SMU."
    } else {
        "No per-core sensors: this platform reports only the package reading above."
    }
}

/// Board rails; `3VCC (chip)` is the sensor chip's supply, not the +3.3V PSU rail.
pub fn show_voltages(ui: &mut egui::Ui, voltages: &[VoltageReading]) {
    if voltages.is_empty() {
        empty_state(
            ui,
            "No board rails in this snapshot. Locally they need a kernel-mode sensor backend to \
             reach the SuperIO chip; remote client telemetry does not carry rails yet.",
        );
        return;
    }

    let any_uncalibrated = voltages.iter().any(|v| !v.calibrated);

    TableBuilder::new(ui)
        .striped(true)
        .resizable(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::auto().at_least(90.0).at_most(140.0))
        .column(Column::auto().at_least(80.0))
        .column(Column::initial(260.0).at_least(160.0))
        .header(20.0, |mut h| {
            h.col(|ui| header_label(ui, "Rail"));
            h.col(|ui| header_label(ui, "Volts"));
            h.col(|ui| header_label(ui, "Scaling"));
        })
        .body(|mut body| {
            for v in voltages {
                let chip_supply = v.label.eq_ignore_ascii_case("3VCC (chip)");
                body.row(ROW_HEIGHT, |mut r| {
                    r.col(|ui| {
                        let resp = ui.label(&v.label);
                        if chip_supply {
                            resp.on_hover_text(
                                "Supply voltage of the SuperIO sensor chip itself. NOT the board's \
                                 +3.3V PSU rail — do not read it as one.",
                            );
                        }
                    });
                    r.col(|ui| {
                        ui.label(format!("{:.3} V", v.volts));
                    });
                    r.col(|ui| {
                        if v.calibrated {
                            ui.colored_label(theme::success(ui), "calibrated divider");
                        } else {
                            ui.colored_label(theme::warn(ui), "nominal divider — uncalibrated");
                        }
                        if chip_supply {
                            ui.colored_label(theme::info(ui), "sensor chip supply, not +3.3V PSU");
                        }
                    });
                });
            }
        });

    if any_uncalibrated {
        ui.add_space(4.0);
        ui.colored_label(
            theme::weak_text(ui),
            "Uncalibrated rails assume a nominal divider, so the absolute value can be off; \
             compare against the rail's own trend, not the nominal spec.",
        );
    }
}

pub fn show_disks(ui: &mut egui::Ui, disks: &[DiskRateSample], filter: &str) {
    let filter_lower = filter.to_lowercase();
    let visible: Vec<_> = disks
        .iter()
        .filter(|d| filter.is_empty() || d.name.to_lowercase().contains(&filter_lower))
        .collect();

    if visible.is_empty() {
        empty_state(ui, "No disks reported in the latest snapshot.");
        return;
    }

    TableBuilder::new(ui)
        .striped(true)
        .resizable(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::initial(220.0).at_least(120.0))
        .column(Column::auto().at_least(110.0))
        .column(Column::auto().at_least(110.0))
        .header(20.0, |mut h| {
            h.col(|ui| header_label(ui, "Name"));
            h.col(|ui| header_label(ui, "Read MB/s"));
            h.col(|ui| header_label(ui, "Write MB/s"));
        })
        .body(|mut body| {
            for d in visible {
                body.row(ROW_HEIGHT, |mut r| {
                    r.col(|ui| {
                        ui.label(&d.name);
                    });
                    r.col(|ui| {
                        ui.label(format!("{:.2}", d.read_mb_per_s));
                    });
                    r.col(|ui| {
                        ui.label(format!("{:.2}", d.write_mb_per_s));
                    });
                });
            }
        });
}

pub fn show_networks(ui: &mut egui::Ui, nets: &[NetworkRateSample], filter: &str) {
    let filter_lower = filter.to_lowercase();
    let visible: Vec<_> = nets
        .iter()
        .filter(|n| filter.is_empty() || n.name.to_lowercase().contains(&filter_lower))
        .collect();

    if visible.is_empty() {
        empty_state(ui, "No network interfaces reported.");
        return;
    }

    TableBuilder::new(ui)
        .striped(true)
        .resizable(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::initial(240.0).at_least(140.0))
        .column(Column::auto().at_least(110.0))
        .column(Column::auto().at_least(110.0))
        .header(20.0, |mut h| {
            h.col(|ui| header_label(ui, "Interface"));
            h.col(|ui| header_label(ui, "Rx"));
            h.col(|ui| header_label(ui, "Tx"));
        })
        .body(|mut body| {
            for n in visible {
                body.row(ROW_HEIGHT, |mut r| {
                    r.col(|ui| {
                        ui.label(&n.name);
                    });
                    r.col(|ui| {
                        ui.label(format!("{:.2}", n.rx_mbps));
                    });
                    r.col(|ui| {
                        ui.label(format!("{:.2}", n.tx_mbps));
                    });
                });
            }
        });
}

/// WHEA counters behind the read status that separates clean from unmeasured.
pub fn show_whea(ui: &mut egui::Ui, snapshot: &TelemetrySnapshot) {
    ui.add_space(8.0);
    ui.vertical(|ui| {
        ui.label(
            RichText::new("Windows Hardware Error Architecture")
                .strong()
                .size(15.0),
        );
        ui.add_space(8.0);
        show_whea_pill(ui, snapshot);
        ui.add_space(8.0);
        let counters = snapshot.whea_counters();
        for (label, value) in [
            (
                "New since program start:",
                counters.map(|w| w.delta_since_program_start),
            ),
            ("Corrected:", counters.map(|w| w.corrected_delta)),
            ("Fatal / uncorrected:", counters.map(|w| w.fatal_delta)),
            (
                "Total retained (spans reboots):",
                counters.map(|w| w.total_retained),
            ),
        ] {
            ui.horizontal(|ui| {
                ui.label(RichText::new(label).strong());
                match value {
                    Some(v) => {
                        ui.label(RichText::new(v.to_string()).monospace());
                    }
                    None => {
                        ui.colored_label(theme::weak_text(ui), ABSENT);
                    }
                }
            });
        }
    });
}

pub fn show_processes(ui: &mut egui::Ui, procs: &[ProcessSample], filter: &str) {
    let filter_lower = filter.to_lowercase();
    let visible: Vec<_> = procs
        .iter()
        .filter(|p| filter.is_empty() || p.name.to_lowercase().contains(&filter_lower))
        .collect();

    if visible.is_empty() {
        empty_state(ui, "No processes in the latest snapshot.");
        return;
    }

    TableBuilder::new(ui)
        .striped(true)
        .resizable(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::auto().at_least(60.0).at_most(80.0))
        .column(Column::initial(240.0).at_least(140.0))
        .column(Column::auto().at_least(90.0))
        .column(Column::auto().at_least(90.0))
        .column(Column::auto().at_least(80.0))
        .header(20.0, |mut h| {
            h.col(|ui| header_label(ui, "PID"));
            h.col(|ui| header_label(ui, "Name"));
            h.col(|ui| header_label(ui, "CPU %"));
            h.col(|ui| header_label(ui, "RAM MB"));
            h.col(|ui| header_label(ui, "PPID"));
        })
        .body(|mut body| {
            for p in visible {
                body.row(ROW_HEIGHT, |mut r| {
                    r.col(|ui| {
                        ui.label(format!("{}", p.pid));
                    });
                    r.col(|ui| {
                        ui.label(&p.name);
                    });
                    r.col(|ui| {
                        ui.colored_label(
                            usage_color(ui, p.cpu_pct.min(100.0)),
                            format!("{:.1}", p.cpu_pct),
                        );
                    });
                    r.col(|ui| {
                        ui.label(format!("{}", p.mem_mb));
                    });
                    r.col(|ui| match p.parent_pid {
                        Some(pp) => {
                            ui.label(format!("{pp}"));
                        }
                        None => {
                            ui.colored_label(theme::weak_text(ui), ABSENT);
                        }
                    });
                });
            }
        });
}

pub fn show_gpus(ui: &mut egui::Ui, gpus: &[GpuSample]) {
    if gpus.is_empty() {
        empty_state(ui, "No GPU sensors visible to sysinfo.");
        return;
    }

    TableBuilder::new(ui)
        .striped(true)
        .resizable(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::auto().at_least(36.0).at_most(60.0))
        .column(Column::auto().at_least(80.0))
        .column(Column::initial(260.0).at_least(160.0))
        .column(Column::auto().at_least(150.0))
        .column(Column::auto().at_least(80.0))
        .column(Column::auto().at_least(120.0))
        .header(20.0, |mut h| {
            h.col(|ui| header_label(ui, "#"));
            h.col(|ui| header_label(ui, "Vendor"));
            h.col(|ui| header_label(ui, "Name / label"));
            h.col(|ui| header_label(ui, "Temp"));
            h.col(|ui| header_label(ui, "Usage %"));
            h.col(|ui| header_label(ui, "VRAM"));
        })
        .body(|mut body| {
            for g in gpus {
                body.row(ROW_HEIGHT, |mut r| {
                    r.col(|ui| {
                        ui.label(format!("{}", g.index));
                    });
                    r.col(|ui| {
                        ui.label(&g.vendor);
                    });
                    r.col(|ui| {
                        ui.label(&g.name);
                    });
                    // A 0.0 is the wire payload's no-sensor fill, not a measured 0 °C.
                    r.col(|ui| match gpu_temp_c(g) {
                        Some(t) => {
                            ui.colored_label(temp_color(ui, Some(t)), format!("{t:.1} °C"));
                        }
                        None => {
                            ui.colored_label(
                                theme::weak_text(ui),
                                format!("{ABSENT} no thermal sensor"),
                            );
                        }
                    });
                    r.col(|ui| match g.usage_pct {
                        Some(u) => {
                            ui.colored_label(usage_color(ui, u), format!("{u:.1}"));
                        }
                        None => {
                            ui.colored_label(theme::weak_text(ui), ABSENT);
                        }
                    });
                    r.col(|ui| match (g.memory_used_mb, g.memory_total_mb) {
                        (Some(u), Some(t)) => {
                            ui.label(format!("{u} / {t} MB"));
                        }
                        (Some(u), None) => {
                            ui.label(format!("{u} MB"));
                        }
                        _ => {
                            ui.colored_label(theme::weak_text(ui), ABSENT);
                        }
                    });
                });
            }
        });
}

/// Status colour for a usage percentage, graded by the widget tiers.
fn usage_color(ui: &Ui, pct: f32) -> egui::Color32 {
    Status::from_usage_pct(Reading::Measured(pct)).color(ui)
}

/// Status colour for a temperature; an absent reading is never graded good.
fn temp_color(ui: &Ui, temp_c: Option<f32>) -> egui::Color32 {
    Status::from_temp_c(Reading::new(temp_c, Absent::NoSensor)).color(ui)
}

fn header_label(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).strong());
}

fn empty_state(ui: &mut egui::Ui, msg: &str) {
    ui.add_space(12.0);
    ui.colored_label(theme::weak_text(ui), msg);
}

pub fn fmt_captured_at(ms: u64) -> String {
    if ms == 0 {
        return ABSENT.into();
    }
    let secs = ms / 1000;
    let frac = ms % 1000;
    let h = (secs / 3600) % 24;
    let mi = (secs / 60) % 60;
    let s = secs % 60;
    format!("{h:02}:{mi:02}:{s:02}.{frac:03}Z")
}

#[cfg(test)]
mod tests {
    use super::*;
    use stress_kit::telemetry::{CoreSample, ThermalReading, WheaCounters};

    fn snap_with(thermals: &[(&str, f32)], cores: &[(&str, f32)]) -> TelemetrySnapshot {
        TelemetrySnapshot {
            cores: cores
                .iter()
                .enumerate()
                .map(|(index, (brand, usage_pct))| CoreSample {
                    index,
                    name: format!("cpu{index}"),
                    brand: (*brand).to_string(),
                    usage_pct: *usage_pct,
                    freq_mhz: 3600,
                    temp_c: None,
                })
                .collect(),
            thermals: thermals
                .iter()
                .map(|(label, temp_c)| ThermalReading {
                    label: (*label).to_string(),
                    temp_c: *temp_c,
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn no_coverage_outranks_amd_brand_inference() {
        let none = snap_with(&[], &[("AMD Ryzen 9 7950X", 12.0)]);
        assert_eq!(
            per_core_absence_reason(&none),
            "No CPU temperature sensors readable on this platform."
        );

        let package_only = snap_with(&[("CPU (Tctl)", 61.0)], &[("AMD Ryzen 9 7950X", 12.0)]);
        assert!(per_core_absence_reason(&package_only).contains("AMD Zen"));

        let intel_package = snap_with(&[("CPU Package", 55.0)], &[("Intel Core i7", 8.0)]);
        assert!(per_core_absence_reason(&intel_package).contains("only the package reading"));
    }

    #[test]
    fn a_zero_gpu_temperature_is_no_sensor() {
        let gpu = |temp_c| GpuSample {
            temp_c,
            ..Default::default()
        };
        assert_eq!(gpu_temp_c(&gpu(Some(0.0))), None);
        assert_eq!(gpu_temp_c(&gpu(Some(41.5))), Some(41.5));
        assert_eq!(gpu_temp_c(&gpu(None)), None);
    }

    #[test]
    fn an_unmeasured_page_file_is_not_zero_percent() {
        assert!(!page_file_measured(&MemorySample::default()));
        assert!(page_file_measured(&MemorySample {
            page_file_total_mb: 8192,
            page_file_used_mb: 512,
            page_file_used_pct: 6.25,
            ..Default::default()
        }));
    }

    #[test]
    fn load_accessors_report_absence_rather_than_zero() {
        let empty = TelemetrySnapshot::default();
        assert_eq!(cpu_load_pct(&empty), None);
        assert_eq!(ram_used_pct(&empty.memory), None);
        assert_eq!(gpu_load_pct(&empty.gpus), None);

        let loaded = snap_with(&[], &[("Intel", 10.0), ("Intel", 30.0)]);
        assert_eq!(cpu_load_pct(&loaded), Some(20.0));
    }

    #[test]
    fn rail_nominals_cover_the_published_labels() {
        assert_eq!(rail_nominal("+12V"), Some(12.0));
        assert_eq!(rail_nominal("3VCC (chip)"), Some(3.3));
        assert_eq!(rail_nominal("vbat"), Some(3.0));
        assert_eq!(rail_nominal("Vcore"), None);
    }

    #[test]
    fn an_unread_whea_log_is_never_graded_good() {
        let clean = TelemetrySnapshot {
            whea: Some(WheaCounters::default()),
            ..Default::default()
        };
        assert_eq!(whea_status_parts(&clean).0, Status::Good);

        let unavailable = TelemetrySnapshot {
            whea_unavailable: true,
            ..Default::default()
        };
        let (status, label) = whea_status_parts(&unavailable);
        assert_eq!(status, Status::NotMeasured);
        assert!(label.contains("not measured"));

        let (status, label) = whea_status_parts(&TelemetrySnapshot::default());
        assert_eq!(status, Status::NotMeasured);
        assert!(label.contains("never sampled"));

        let fatal = TelemetrySnapshot {
            whea: Some(WheaCounters {
                delta_since_program_start: 2,
                total_retained: 9,
                corrected_delta: 1,
                fatal_delta: 1,
            }),
            ..Default::default()
        };
        assert_eq!(whea_status_parts(&fatal).0, Status::Critical);
    }
}
