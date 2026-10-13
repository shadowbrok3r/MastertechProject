use eframe::egui::{Grid, ScrollArea, Ui, Vec2, Vec2b};

use crate::ui_tools::{
    controls::{self, BarKind, Metrics, Tokens},
    glass_card,
    icons::svg,
};

pub fn display_job_builder_page(ui: &mut Ui) {
    let m = Metrics::get(ui.ctx());
    ui.add_space(2.0 * m.gap.y);
    ScrollArea::vertical()
        .max_height(f32::INFINITY)
        .max_width(680.0)
        .auto_shrink(Vec2b::new(false, false))
        .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                controls::bar(ui, BarKind::Header, |ui| {
                    controls::icon_label(ui, svg::TOOLBOX, "Job Builder");
                });
                let t = Tokens::get(ui.ctx());
                glass_card::group(ui, |ui| {
                    Grid::new("job builder grid")
                        .spacing(Vec2::new(m.gap.x, m.gap.y))
                        .min_col_width(150.)
                        .max_col_width(150.)
                        .striped(true)
                        .num_columns(2)
                        .show(ui, |ui| {
                            for job in ["Libre Office", "SEB", "CPS", "Data Transfer", "Data Transfer"] {
                                ui.colored_label(t.text_dim, job);
                                controls::toggle(ui, &mut false, "");
                                ui.end_row();
                            }
                        });
                });
            });
        });
}
