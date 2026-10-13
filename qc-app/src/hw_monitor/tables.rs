//! Per-view table renderers for [`crate::hw_monitor::HwMonitor`].
//!
//! Each `show_*` function takes the slice of samples for its view plus a
//! filter string (where it applies) and draws a TableBuilder. Read-only,
//! sortable by clicking column headers (TableBuilder default), and styled
//! consistently across views.

use eframe::egui::{self, RichText};
use egui_extras::{Column, TableBuilder};
use mtech_ui::controls::{self, Icon, Metrics, Tokens};
use mtech_ui::icons::svg;
use mtech_ui::theme;
use stress_kit::telemetry::{
    DiskRateSample, GpuSample, MemorySample, NetworkRateSample, ProcessSample, ThermalReading,
    WheaCounters,
};

const ROW_HEIGHT: f32 = 18.0;
/// Height of a usage bar.
const BAR_H: f32 = 10.0;
/// Widest a usage bar runs.
const BAR_MAX_W: f32 = 360.0;

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
        controls::empty_state(ui, svg::CPU, "No cores", "No cores match the filter.", &[]);
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
                        let tone = theme::warn(ui);
                        ui.colored_label(tone, format!("{}", c.index));
                    });
                    r.col(|ui| {
                        ui.label(&c.name);
                    });
                    r.col(|ui| {
                        ui.label(&c.brand);
                    });
                    r.col(|ui| {
                        let tone = theme::usage_level(ui, c.usage_pct);
                        ui.colored_label(tone, format!("{:.1}%", c.usage_pct));
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
                                let tone = theme::temp_level(ui, t);
                                ui.colored_label(tone, format!("{t:.1} °C"));
                            }
                            None => missing(ui, "N/A"),
                        }
                    });
                });
            }
        });
}

pub fn show_memory(ui: &mut egui::Ui, m: &MemorySample) {
    let gap = Metrics::get(ui.ctx()).gap;
    ui.add_space(gap.x);
    ui.vertical(|ui| {
        bar_row(
            ui,
            svg::MEMORY,
            "RAM",
            &format!("{} / {} MB", m.used_mb, m.total_mb),
            m.used_pct,
        );
        ui.add_space(gap.x);
        bar_row(
            ui,
            svg::SWAP,
            "Page file",
            &format!("{} / {} MB", m.page_file_used_mb, m.page_file_total_mb),
            m.page_file_used_pct,
        );
        ui.add_space(gap.x);

        ui.horizontal(|ui| {
            ui.label(RichText::new("vmmem").strong());
            match m.vmmem_mb {
                Some(mb) => {
                    let tone = theme::info(ui);
                    ui.colored_label(tone, format!("{mb} MB resident (WSL / Hyper-V)"));
                }
                None => missing(ui, "not running"),
            }
        });

        ui.add_space(gap.y * 2.0);
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
        controls::empty_state(
            ui,
            svg::HARD_DRIVES,
            "No disks",
            "No disks reported in the latest snapshot.",
            &[],
        );
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
        controls::empty_state(
            ui,
            svg::NETWORK_SLASH,
            "No network interfaces reported",
            "",
            &[],
        );
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
    let gap = Metrics::get(ui.ctx()).gap;
    ui.add_space(gap.x);
    match whea {
        None => {
            controls::empty_state(
                ui,
                svg::WARNING_OCTAGON,
                "WHEA counters unavailable on this platform",
                "Windows only, requires readable WHEA log.",
                &[],
            );
        }
        Some(w) => {
            let delta_color = if w.delta_since_program_start > 0 {
                theme::error(ui)
            } else {
                theme::success(ui)
            };
            ui.vertical(|ui| {
                controls::subheading(ui, svg::CIRCUITRY, "Windows Hardware Error Architecture");
                ui.add_space(gap.x);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("New since program start:").strong());
                    ui.colored_label(delta_color, format!("{}", w.delta_since_program_start));
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Total retained (spans reboots):").strong());
                    ui.label(format!("{}", w.total_retained));
                });
                ui.add_space(gap.x);
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
        controls::empty_state(
            ui,
            svg::LIST_BULLETS,
            "No processes yet",
            "No processes in the latest snapshot. (The first tick may take a few seconds.)",
            &[],
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
                        let tone = theme::usage_level(ui, p.cpu_pct.min(100.0));
                        ui.colored_label(tone, format!("{:.1}", p.cpu_pct));
                    });
                    r.col(|ui| {
                        ui.label(format!("{}", p.mem_mb));
                    });
                    r.col(|ui| match p.parent_pid {
                        Some(pp) => {
                            ui.label(format!("{pp}"));
                        }
                        None => missing(ui, "—"),
                    });
                });
            }
        });
}

pub fn show_gpus(ui: &mut egui::Ui, gpus: &[GpuSample]) {
    if gpus.is_empty() {
        controls::empty_state(
            ui,
            svg::GRAPHICS_CARD,
            "No GPU",
            "No GPU visible to NVML, WDDM, or sysinfo.",
            &[],
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
                            let tone = theme::temp_level(ui, t);
                            ui.colored_label(tone, format!("{t:.1} °C"));
                        }
                        None => missing(ui, "N/A"),
                    });
                    r.col(|ui| match g.usage_pct {
                        Some(u) => {
                            let tone = theme::usage_level(ui, u);
                            ui.colored_label(tone, format!("{u:.1}"));
                        }
                        None => missing(ui, "—"),
                    });
                    r.col(|ui| match (g.memory_used_mb, g.memory_total_mb) {
                        (Some(u), Some(t)) => {
                            ui.label(format!("{u} / {t} MB"));
                        }
                        (Some(u), None) => {
                            ui.label(format!("{u} MB"));
                        }
                        _ => missing(ui, "—"),
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
        controls::empty_state(
            ui,
            svg::THERMOMETER,
            "No temperature sensors",
            "No temperature sensors in the latest snapshot. CPU/board temps need a kernel-mode \
             sensor backend and an elevated run; NVMe/SATA disk temps need no driver. See the \
             Logs tab for which backend was tried and why it declined.",
            &[],
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
                        let tone = theme::temp_level(ui, t.temp_c);
                        ui.colored_label(tone, format!("{:.1} °C", t.temp_c));
                    });
                });
            }
        });
}

// ── helpers ──────────────────────────────────────────────────────────────

fn header_label(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).strong());
}

/// Faint placeholder for a missing reading.
fn missing(ui: &mut egui::Ui, text: &str) {
    let tone = theme::faint_text(ui);
    ui.colored_label(tone, text);
}

fn bar_row(ui: &mut egui::Ui, icon: Icon, label: &str, value: &str, pct: f32) {
    let pct = pct.clamp(0.0, 100.0);
    let color = theme::usage_level(ui, pct);
    let t = Tokens::get(ui.ctx());
    let m = Metrics::get(ui.ctx());
    ui.horizontal(|ui| {
        let (slot, _) = ui.allocate_exact_size(egui::Vec2::splat(m.icon_md), egui::Sense::hover());
        icon.paint(ui, slot, m.icon_md, t.icon);
        ui.label(RichText::new(label).strong());
        ui.colored_label(color, format!("{pct:.1}%"));
        ui.label(RichText::new(value).weak());
    });
    let bar_w = ui.available_width().min(BAR_MAX_W);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(bar_w, BAR_H), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, t.radius_sm, t.sunken);
    let filled_w = rect.width() * (pct / 100.0);
    let filled = egui::Rect::from_min_size(rect.min, egui::vec2(filled_w, rect.height()));
    painter.rect_filled(filled, t.radius_sm, color);
}
