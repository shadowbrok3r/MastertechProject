//! Per-view table renderers for [`crate::hw_monitor::HwMonitor`].
//!
//! Each `show_*` function takes the slice of samples for its view plus a
//! filter string (where it applies) and draws a TableBuilder. Read-only,
//! sortable by clicking column headers (TableBuilder default), and styled
//! consistently across views.

use eframe::egui::{self, Color32, RichText};
use egui_extras::{Column, TableBuilder};
use stress_kit::telemetry::{
    DiskRateSample, GpuSample, MemorySample, NetworkRateSample, ProcessSample, ThermalReading,
    WheaCounters,
};

use super::{temp_color, usage_color};

const ROW_HEIGHT: f32 = 18.0;

pub fn show_cores(
    ui: &mut egui::Ui,
    snapshot: &stress_kit::telemetry::TelemetrySnapshot,
    filter: &str,
) {
    let filter_lower = filter.to_lowercase();
    let visible: Vec<&stress_kit::telemetry::CoreSample> = snapshot
        .cores
        .iter()
        .filter(|c| {
            filter.is_empty()
                || c.name.to_lowercase().contains(&filter_lower)
                || c.brand.to_lowercase().contains(&filter_lower)
        })
        .collect();

    if visible.is_empty() {
        empty_state(ui, "No cores match the filter.");
        return;
    }

    TableBuilder::new(ui)
        .striped(true)
        .resizable(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::auto().at_least(36.0).at_most(60.0)) // index
        .column(Column::auto().at_least(60.0).at_most(110.0)) // name
        .column(Column::initial(280.0).at_least(180.0)) // brand
        .column(Column::auto().at_least(80.0)) // usage
        .column(Column::auto().at_least(100.0)) // freq
        .column(Column::auto().at_least(80.0)) // temp
        .header(20.0, |mut h| {
            h.col(|ui| header_label(ui, "#"));
            h.col(|ui| header_label(ui, "Name"));
            h.col(|ui| header_label(ui, "Brand"));
            h.col(|ui| header_label(ui, "Usage %"));
            h.col(|ui| header_label(ui, "Freq"));
            h.col(|ui| header_label(ui, "Temp"));
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
                            usage_color(c.usage_pct),
                            format!("{:.1}%", c.usage_pct),
                        );
                    });
                    r.col(|ui| {
                        ui.label(format!("{} MHz", c.freq_mhz));
                    });
                    r.col(|ui| {
                        // sysinfo has no per-core temp on Windows; fall back to the
                        // die reader's `CPU Core N` entry in the thermals list.
                        let temp = c.temp_c.or_else(|| {
                            let want = format!("CPU Core {}", c.index);
                            snapshot
                                .thermals
                                .iter()
                                .find(|t| t.label == want)
                                .map(|t| t.temp_c)
                        });
                        match temp {
                            Some(t) => {
                                ui.colored_label(temp_color(t), format!("{t:.1} °C"));
                            }
                            None => {
                                ui.colored_label(Color32::GRAY, "N/A");
                            }
                        }
                    });
                });
            }
        });
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
                        Color32::from_rgb(140, 180, 230),
                        format!("{mb} MB resident (WSL / Hyper-V)"),
                    );
                }
                None => {
                    ui.colored_label(Color32::GRAY, "not running");
                }
            }
        });

        ui.add_space(12.0);
        ui.label(
            RichText::new(
                "Page-file numbers on Windows are derived from \
                 GetPerformanceInfo; non-Windows targets report zeroes.",
            )
            .small()
            .weak(),
        );
    });
}

pub fn show_disks(ui: &mut egui::Ui, disks: &[DiskRateSample], filter: &str) {
    let filter_lower = filter.to_lowercase();
    let visible: Vec<&DiskRateSample> = disks
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
    let visible: Vec<&NetworkRateSample> = nets
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
                Color32::GRAY,
                "WHEA counters unavailable on this platform \
                 (Windows only, requires readable WHEA log).",
            );
        }
        Some(w) => {
            let delta_color = if w.delta_since_program_start > 0 {
                Color32::from_rgb(220, 80, 60)
            } else {
                Color32::from_rgb(140, 200, 140)
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
                        Color32::from_rgb(180, 180, 200),
                        format!("{}", w.total_retained),
                    );
                });
                ui.add_space(8.0);
                ui.label(
                    RichText::new(
                        "Any non-zero delta during a stress run is a hardware-error \
                         signal worth investigating. Empty out the WHEA log between runs \
                         if you want a clean delta.",
                    )
                    .small()
                    .weak(),
                );
            });
        }
    }
}

pub fn show_processes(ui: &mut egui::Ui, procs: &[ProcessSample], filter: &str) {
    let filter_lower = filter.to_lowercase();
    let visible: Vec<&ProcessSample> = procs
        .iter()
        .filter(|p| filter.is_empty() || p.name.to_lowercase().contains(&filter_lower))
        .collect();

    if visible.is_empty() {
        empty_state(
            ui,
            "No processes in the latest snapshot. (The first tick may take a few seconds.)",
        );
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
                            usage_color(p.cpu_pct.min(100.0)),
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
                            ui.colored_label(Color32::GRAY, "—");
                        }
                    });
                });
            }
        });
}

pub fn show_gpus(ui: &mut egui::Ui, gpus: &[GpuSample]) {
    if gpus.is_empty() {
        empty_state(
            ui,
            "No GPU sensors visible to sysinfo. \
             Vendor-specific live metrics (NVML / ADL) are not wired up yet.",
        );
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
                            ui.colored_label(temp_color(t), format!("{t:.1} °C"));
                        }
                        None => {
                            ui.colored_label(Color32::GRAY, "N/A");
                        }
                    });
                    r.col(|ui| match g.usage_pct {
                        Some(u) => {
                            ui.colored_label(usage_color(u), format!("{u:.1}"));
                        }
                        None => {
                            ui.colored_label(Color32::GRAY, "—");
                        }
                    });
                    r.col(|ui| {
                        match (g.memory_used_mb, g.memory_total_mb) {
                            (Some(u), Some(t)) => {
                                ui.label(format!("{u} / {t} MB"));
                            }
                            (Some(u), None) => {
                                ui.label(format!("{u} MB"));
                            }
                            _ => {
                                ui.colored_label(Color32::GRAY, "—");
                            }
                        }
                    });
                });
            }
        });
}

pub fn show_thermals(ui: &mut egui::Ui, thermals: &[ThermalReading], filter: &str) {
    let filter_lower = filter.to_lowercase();
    let visible: Vec<&ThermalReading> = thermals
        .iter()
        .filter(|t| filter.is_empty() || t.label.to_lowercase().contains(&filter_lower))
        .collect();

    if visible.is_empty() {
        empty_state(
            ui,
            "No temperature sensors in the latest snapshot. CPU/board temps need a kernel-mode \
             sensor backend and an elevated run; NVMe/SATA disk temps need no driver. See the \
             Logs tab for which backend was tried and why it declined.",
        );
        return;
    }

    TableBuilder::new(ui)
        .striped(true)
        .resizable(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::initial(220.0).at_least(140.0)) // sensor
        .column(Column::auto().at_least(90.0)) // temp
        .header(20.0, |mut h| {
            h.col(|ui| header_label(ui, "Sensor"));
            h.col(|ui| header_label(ui, "Temp"));
        })
        .body(|mut body| {
            for t in visible {
                body.row(ROW_HEIGHT, |mut r| {
                    r.col(|ui| {
                        ui.label(&t.label);
                    });
                    r.col(|ui| {
                        ui.colored_label(temp_color(t.temp_c), format!("{:.1} °C", t.temp_c));
                    });
                });
            }
        });
}

// ── helpers ──────────────────────────────────────────────────────────────

fn header_label(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).strong());
}

fn empty_state(ui: &mut egui::Ui, msg: &str) {
    ui.add_space(12.0);
    ui.colored_label(Color32::GRAY, msg);
}

fn bar_row(ui: &mut egui::Ui, label: &str, value: &str, pct: f32) {
    let pct = pct.clamp(0.0, 100.0);
    let color = usage_color(pct);
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
