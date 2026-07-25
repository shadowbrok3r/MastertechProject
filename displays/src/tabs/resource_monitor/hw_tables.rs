use std::collections::HashMap;

use eframe::egui::{self, RichText, Ui};
use egui_extras::{Column, TableBuilder};
use stress_kit::telemetry::{
    DiskRateSample, GpuSample, MemorySample, NetworkRateSample, ProcessSample, TelemetrySnapshot,
    ThermalReading, VoltageReading, WheaCounters,
};

use crate::ui_tools::theme;

const ROW_HEIGHT: f32 = 18.0;

/// Package-level CPU reading: `CPU Package` on Intel, `CPU (Tctl)` on AMD Zen,
/// else any non-per-core CPU sensor such as the `CPUZ_0` ACPI zone. `CPU Core N`
/// is excluded so a package summary never borrows a single core's value.
fn cpu_package_reading(snapshot: &TelemetrySnapshot) -> Option<&ThermalReading> {
    let not_per_core = |r: &&ThermalReading| !r.label.to_lowercase().starts_with("cpu core");
    snapshot
        .thermals
        .iter()
        .find(|r| {
            let l = r.label.to_lowercase();
            not_per_core(r) && (l.contains("package") || l.contains("tctl") || l.contains("tdie"))
        })
        .or_else(|| {
            snapshot
                .thermals
                .iter()
                .find(|r| not_per_core(r) && r.label.to_lowercase().starts_with("cpu"))
        })
}

/// `CPU Core N` readings keyed by core index.
fn per_core_thermals(snapshot: &TelemetrySnapshot) -> HashMap<usize, f32> {
    snapshot
        .thermals
        .iter()
        .filter_map(|r| {
            let rest = r
                .label
                .strip_prefix("CPU Core ")
                .or_else(|| r.label.strip_prefix("cpu core "))?;
            rest.trim().parse::<usize>().ok().map(|i| (i, r.temp_c))
        })
        .collect()
}

/// Package/Tctl reading for a section header, e.g. `CPU (Tctl) 52.4 °C`.
pub fn cpu_package_summary(snapshot: &TelemetrySnapshot) -> Option<String> {
    cpu_package_reading(snapshot).map(|r| format!("{} {:.1} °C", r.label, r.temp_c))
}

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
        match cpu_package_reading(snapshot) {
            Some(r) => {
                ui.colored_label(
                    theme::temp_level(ui, r.temp_c),
                    format!("{:.1} °C", r.temp_c),
                );
                ui.label(RichText::new(&r.label).small().weak());
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

    let per_core = per_core_thermals(snapshot);
    let has_per_core = !per_core.is_empty() || visible.iter().any(|c| c.temp_c.is_some());
    if !has_per_core {
        let brand = visible.first().map(|c| c.brand.to_lowercase()).unwrap_or_default();
        let amd = brand.contains("amd") || brand.contains("ryzen") || brand.contains("threadripper");
        ui.colored_label(
            theme::weak_text(ui),
            if amd {
                "No per-core sensors: AMD Zen exposes only the package Tctl through the SMU."
            } else {
                "No per-core temperature sensors readable on this platform."
            },
        );
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
                            theme::usage_level(ui, c.usage_pct),
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
                                ui.colored_label(theme::temp_level(ui, t), format!("{t:.1} °C"));
                            }
                            None => {
                                ui.colored_label(theme::weak_text(ui), "no sensor");
                            }
                        });
                    }
                });
            }
        });
}

/// Board rails from the SuperIO chip. `3VCC (chip)` is the sensor chip's own
/// supply, never the board's +3.3V PSU rail; rails scaled with a nominal
/// divider are marked uncalibrated.
pub fn show_voltages(ui: &mut egui::Ui, voltages: &[VoltageReading]) {
    if voltages.is_empty() {
        empty_state(
            ui,
            "No board rails in this snapshot. Locally they need the WinRing0 driver for the SuperIO \
             chip (Memory Integrity blocks it); remote client telemetry does not carry rails yet.",
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

pub fn show_memory(ui: &mut egui::Ui, m: &MemorySample) {
    ui.add_space(8.0);
    ui.vertical(|ui| {
        bar_row(
            ui,
            "RAM",
            &format!("{} / {} MB", m.used_mb, m.total_mb),
            m.used_pct,
        );
        ui.add_space(8.0);
        bar_row(
            ui,
            "Page file",
            &format!("{} / {} MB", m.page_file_used_mb, m.page_file_total_mb),
            m.page_file_used_pct,
        );
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            ui.label(RichText::new("vmmem").strong());
            ui.add_space(8.0);
            match m.vmmem_mb {
                Some(mb) => {
                    ui.colored_label(
                        theme::info(ui),
                        format!("{mb} MB resident (WSL / Hyper-V)"),
                    );
                }
                None => {
                    ui.colored_label(theme::weak_text(ui), "not running");
                }
            }
        });
    });
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
            h.col(|ui| header_label(ui, "Rx Mbps"));
            h.col(|ui| header_label(ui, "Tx Mbps"));
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

pub fn show_whea(ui: &mut egui::Ui, whea: &Option<WheaCounters>) {
    ui.add_space(8.0);
    match whea {
        None => {
            ui.colored_label(
                theme::weak_text(ui),
                "WHEA counters unavailable on this platform.",
            );
        }
        Some(w) => {
            let delta_color = if w.delta_since_program_start > 0 {
                theme::error(ui)
            } else {
                theme::success(ui)
            };
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("Windows Hardware Error Architecture")
                        .strong()
                        .size(15.0),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("New since program start:").strong());
                    ui.colored_label(
                        delta_color,
                        format!("{}", w.delta_since_program_start),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Total retained (spans reboots):").strong());
                    ui.colored_label(
                        theme::weak_text(ui),
                        format!("{}", w.total_retained),
                    );
                });
            });
        }
    }
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
                            theme::usage_level(ui, p.cpu_pct.min(100.0)),
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
                            ui.colored_label(theme::weak_text(ui), "—");
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
        .column(Column::auto().at_least(80.0))
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
                    r.col(|ui| match g.temp_c {
                        Some(t) => {
                            ui.colored_label(theme::temp_level(ui, t), format!("{t:.1} °C"));
                        }
                        None => {
                            ui.colored_label(theme::weak_text(ui), "N/A");
                        }
                    });
                    r.col(|ui| match g.usage_pct {
                        Some(u) => {
                            ui.colored_label(theme::usage_level(ui, u), format!("{u:.1}"));
                        }
                        None => {
                            ui.colored_label(theme::weak_text(ui), "—");
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
                            ui.colored_label(theme::weak_text(ui), "—");
                        }
                    });
                });
            }
        });
}

fn header_label(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).strong());
}

fn empty_state(ui: &mut egui::Ui, msg: &str) {
    ui.add_space(12.0);
    ui.colored_label(theme::weak_text(ui), msg);
}

fn bar_row(ui: &mut Ui, label: &str, value: &str, pct: f32) {
    let pct = pct.clamp(0.0, 100.0);
    let color = theme::usage_level(ui, pct);
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).strong());
        ui.add_space(8.0);
        ui.colored_label(color, format!("{pct:.1}%"));
        ui.add_space(8.0);
        ui.label(RichText::new(value).weak());
    });
    let bar_h = 10.0;
    let bar_w = ui.available_width().min(360.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(bar_w, bar_h), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 4.0, ui.visuals().extreme_bg_color);
    let filled_w = rect.width() * (pct / 100.0);
    let filled = egui::Rect::from_min_size(rect.min, egui::vec2(filled_w, rect.height()));
    painter.rect_filled(filled, 4.0, color);
}

pub fn fmt_captured_at(ms: u64) -> String {
    if ms == 0 {
        return "—".into();
    }
    let secs = ms / 1000;
    let frac = ms % 1000;
    let h = (secs / 3600) % 24;
    let mi = (secs / 60) % 60;
    let s = secs % 60;
    format!("{h:02}:{mi:02}:{s:02}.{frac:03}Z")
}
