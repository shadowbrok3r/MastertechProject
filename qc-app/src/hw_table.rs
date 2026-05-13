//! Per-core CPU table plus a bottom summary strip for the system-wide
//! telemetry that doesn't fit a per-core row (memory, page file, vmmem,
//! disk r/w, network rx/tx, WHEA delta).

use eframe::egui::{self, Widget};
use egui_data_table::{
    viewer::{default_hotkeys, DecodeErrorBehavior, RowCodec, UiActionContext},
    DataTable, Renderer, RowViewer, UiAction,
};
use egui_extras::Column;
use serde::Serialize;
use stress_kit::telemetry::TelemetrySnapshot;

use crate::hw_sampler::CoreRow;

#[derive(Serialize, Default)]
pub struct CoreRowViewer {
    filter: String,
    #[serde(skip)]
    hotkeys: Vec<(egui::KeyboardShortcut, UiAction)>,
}

impl RowViewer<CoreRow> for CoreRowViewer {
    fn try_create_codec(&mut self, _: bool) -> Option<impl RowCodec<CoreRow>> {
        Some(CoreRowCodec)
    }

    fn num_columns(&mut self) -> usize {
        6
    }

    fn column_name(&mut self, column: usize) -> std::borrow::Cow<'static, str> {
        ["#", "Name", "Brand", "Usage %", "Freq (MHz)", "Temp (°C)"][column].into()
    }

    fn is_sortable_column(&mut self, _column: usize) -> bool {
        true
    }

    fn row_filter_hash(&mut self) -> &impl std::hash::Hash {
        &self.filter
    }

    fn filter_row(&mut self, row: &CoreRow) -> bool {
        if self.filter.is_empty() {
            return true;
        }
        let f = self.filter.to_lowercase();
        row.brand.to_lowercase().contains(&f) || row.name.to_lowercase().contains(&f)
    }

    fn hotkeys(
        &mut self,
        context: &UiActionContext,
    ) -> Vec<(egui::KeyboardShortcut, UiAction)> {
        let keys = default_hotkeys(context);
        self.hotkeys.clone_from(&keys);
        keys
    }

    fn show_cell_view(&mut self, ui: &mut egui::Ui, row: &CoreRow, column: usize) {
        let _ = match column {
            0 => ui.horizontal_centered(|ui| {
                ui.colored_label(ui.style().visuals.warn_fg_color, format!(" {}", row.index))
            }),
            1 => ui.horizontal_centered(|ui| ui.label(format!(" {}", row.name))),
            2 => ui.horizontal_centered(|ui| ui.label(format!(" {}", row.brand))),
            3 => {
                let pct = row.usage_pct;
                let color = usage_color(pct);
                ui.horizontal_centered(|ui| ui.colored_label(color, format!(" {pct:.1}%")))
            }
            4 => ui.horizontal_centered(|ui| ui.label(format!(" {} MHz", row.freq_mhz))),
            5 => {
                let text = row
                    .temp_c
                    .map(|t| format!(" {t:.1} °C"))
                    .unwrap_or_else(|| " N/A".into());
                let color = row.temp_c.map(temp_color).unwrap_or(egui::Color32::GRAY);
                ui.horizontal_centered(|ui| ui.colored_label(color, text))
            }
            _ => unreachable!(),
        };
    }

    fn column_render_config(&mut self, column: usize, _last: bool) -> Column {
        let col = Column::auto().resizable(true);
        match column {
            0 => col.at_least(36.0).at_most(48.0),
            1 => col.at_least(60.0).at_most(80.0),
            2 => col.at_least(200.0),
            3 => col.at_least(75.0).at_most(80.0),
            4 => col.at_least(95.0).at_most(110.0),
            5 => col.at_least(75.0).at_most(95.0),
            _ => col,
        }
    }

    fn show_cell_editor(
        &mut self,
        _ui: &mut egui::Ui,
        _row: &mut CoreRow,
        _column: usize,
    ) -> Option<egui::Response> {
        None
    }

    fn on_cell_view_response(
        &mut self,
        _row: &CoreRow,
        _column: usize,
        _resp: &egui::Response,
    ) -> Option<Box<CoreRow>> {
        None
    }

    fn set_cell_value(&mut self, src: &CoreRow, dst: &mut CoreRow, column: usize) {
        match column {
            0 => dst.index = src.index,
            1 => dst.name.clone_from(&src.name),
            2 => dst.brand.clone_from(&src.brand),
            3 => dst.usage_pct = src.usage_pct,
            4 => dst.freq_mhz = src.freq_mhz,
            5 => dst.temp_c = src.temp_c,
            _ => unreachable!(),
        }
    }

    fn compare_cell(&self, l: &CoreRow, r: &CoreRow, column: usize) -> std::cmp::Ordering {
        match column {
            0 => l.index.cmp(&r.index),
            1 => l.name.cmp(&r.name),
            2 => l.brand.cmp(&r.brand),
            3 => l
                .usage_pct
                .partial_cmp(&r.usage_pct)
                .unwrap_or(std::cmp::Ordering::Equal),
            4 => l.freq_mhz.cmp(&r.freq_mhz),
            5 => l
                .temp_c
                .unwrap_or(f32::NEG_INFINITY)
                .partial_cmp(&r.temp_c.unwrap_or(f32::NEG_INFINITY))
                .unwrap_or(std::cmp::Ordering::Equal),
            _ => l.index.cmp(&r.index),
        }
    }

    fn new_empty_row(&mut self) -> CoreRow {
        CoreRow::default()
    }
}

struct CoreRowCodec;

impl RowCodec<CoreRow> for CoreRowCodec {
    type DeserializeError = DecodeErrorBehavior;

    fn create_empty_decoded_row(&mut self) -> CoreRow {
        CoreRow::default()
    }

    fn encode_column(&mut self, src: &CoreRow, column: usize, dst: &mut String) {
        match column {
            0 => dst.push_str(&src.index.to_string()),
            1 => dst.push_str(&src.name),
            2 => dst.push_str(&src.brand),
            3 => dst.push_str(&format!("{:.1}", src.usage_pct)),
            4 => dst.push_str(&src.freq_mhz.to_string()),
            5 => {
                if let Some(t) = src.temp_c {
                    dst.push_str(&format!("{:.1}", t));
                } else {
                    dst.push_str("N/A");
                }
            }
            _ => unreachable!(),
        }
    }

    fn decode_column(
        &mut self,
        _src: &str,
        _column: usize,
        _dst: &mut CoreRow,
    ) -> Result<(), DecodeErrorBehavior> {
        Ok(())
    }
}

/// `DataTable<CoreRow>` + viewer; call [`HwTable::update`] before [`HwTable::show`].
pub struct HwTable {
    table: DataTable<CoreRow>,
    viewer: CoreRowViewer,
    refresh_label: String,
    snapshot: TelemetrySnapshot,
}

impl HwTable {
    pub fn new() -> Self {
        Self {
            table: DataTable::new(),
            viewer: CoreRowViewer::default(),
            refresh_label: String::new(),
            snapshot: TelemetrySnapshot::default(),
        }
    }

    /// Replace rows + system-wide stats from the latest sampler snapshot.
    pub fn update(&mut self, snapshot: TelemetrySnapshot) {
        let rows = snapshot.cores.clone();
        self.refresh_label = if rows.is_empty() {
            "Waiting for first sample…".into()
        } else {
            format!("{} logical cores", rows.len())
        };
        self.table.replace(rows);
        self.snapshot = snapshot;
    }

    /// Draw the full table UI.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        #[allow(deprecated)]
        egui::TopBottomPanel::top("hw_table_top")
            .exact_size(28.0)
            .show_inside(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    egui::TextEdit::singleline(&mut self.viewer.filter)
                        .hint_text(" Search brand / name…")
                        .desired_width(200.0)
                        .ui(ui);
                    ui.add_space(12.0);
                    ui.label(egui::RichText::new(&self.refresh_label).small().weak());
                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            #[cfg(target_os = "windows")]
                            ui.colored_label(
                                egui::Color32::from_rgb(200, 160, 60),
                                "Temperature: N/A on Windows (no sensor in `sysinfo`)",
                            );
                        },
                    );
                });
            });

        #[allow(deprecated)]
        egui::TopBottomPanel::bottom("hw_table_summary")
            .resizable(false)
            .min_size(56.0)
            .show_inside(ui, |ui| {
                self.show_summary(ui);
            });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            egui::ScrollArea::horizontal()
                .auto_shrink(false)
                .show(ui, |ui| {
                    Renderer::new(&mut self.table, &mut self.viewer)
                        .with_style_modify(|s| {
                            s.single_click_edit_mode = true;
                            s.auto_shrink = [false, false].into();
                        })
                        .ui(ui);
                });
        });
    }

    fn show_summary(&self, ui: &mut egui::Ui) {
        let s = &self.snapshot;
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            // RAM
            chip(
                ui,
                "RAM",
                &format!(
                    "{:.1}% — {} / {} MB",
                    s.memory.used_pct, s.memory.used_mb, s.memory.total_mb
                ),
                usage_color(s.memory.used_pct),
            );

            // Page file
            chip(
                ui,
                "Page file",
                &format!(
                    "{:.1}% — {} / {} MB",
                    s.memory.page_file_used_pct,
                    s.memory.page_file_used_mb,
                    s.memory.page_file_total_mb
                ),
                usage_color(s.memory.page_file_used_pct),
            );

            // vmmem (WSL / Hyper-V host) — only when present
            if let Some(mb) = s.memory.vmmem_mb {
                chip(
                    ui,
                    "vmmem",
                    &format!("{mb} MB"),
                    egui::Color32::from_rgb(140, 180, 230),
                );
            }

            // Top disk by combined throughput
            if let Some(d) = top_disk(&s.disks) {
                chip(
                    ui,
                    &format!("Disk {}", d.name),
                    &format!("R {:.1} / W {:.1} MB/s", d.read_mb_per_s, d.write_mb_per_s),
                    egui::Color32::from_rgb(160, 200, 140),
                );
            }

            // Top network by combined throughput
            if let Some(n) = top_network(&s.networks) {
                chip(
                    ui,
                    &format!("Net {}", n.name),
                    &format!("Rx {:.1} / Tx {:.1} Mbps", n.rx_mbps, n.tx_mbps),
                    egui::Color32::from_rgb(160, 200, 220),
                );
            }

            // WHEA
            if let Some(w) = &s.whea {
                let color = if w.delta_since_program_start > 0 {
                    egui::Color32::from_rgb(220, 80, 60)
                } else {
                    egui::Color32::from_rgb(140, 200, 140)
                };
                chip(
                    ui,
                    "WHEA",
                    &format!(
                        "+{} new (since boot: {})",
                        w.delta_since_program_start, w.absolute_since_boot
                    ),
                    color,
                );
            }
        });
        ui.add_space(4.0);
    }
}

fn chip(ui: &mut egui::Ui, label: &str, value: &str, color: egui::Color32) {
    egui::Frame::default()
        .fill(ui.visuals().extreme_bg_color)
        .corner_radius(egui::CornerRadius::same(4))
        .inner_margin(egui::Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(label).small().weak());
                ui.colored_label(color, egui::RichText::new(value).strong());
            });
        });
}

fn top_disk(
    disks: &[stress_kit::telemetry::DiskRateSample],
) -> Option<&stress_kit::telemetry::DiskRateSample> {
    disks
        .iter()
        .max_by(|a, b| {
            (a.read_mb_per_s + a.write_mb_per_s)
                .partial_cmp(&(b.read_mb_per_s + b.write_mb_per_s))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

fn top_network(
    nets: &[stress_kit::telemetry::NetworkRateSample],
) -> Option<&stress_kit::telemetry::NetworkRateSample> {
    nets.iter().max_by(|a, b| {
        (a.rx_mbps + a.tx_mbps)
            .partial_cmp(&(b.rx_mbps + b.tx_mbps))
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}

impl Default for HwTable {
    fn default() -> Self {
        Self::new()
    }
}

fn usage_color(pct: f32) -> egui::Color32 {
    if pct >= 90.0 {
        egui::Color32::from_rgb(220, 80, 60)
    } else if pct >= 70.0 {
        egui::Color32::from_rgb(230, 180, 60)
    } else {
        egui::Color32::from_rgb(100, 200, 100)
    }
}

fn temp_color(temp: f32) -> egui::Color32 {
    if temp >= 90.0 {
        egui::Color32::from_rgb(220, 80, 60)
    } else if temp >= 75.0 {
        egui::Color32::from_rgb(230, 180, 60)
    } else {
        egui::Color32::from_rgb(100, 200, 100)
    }
}
