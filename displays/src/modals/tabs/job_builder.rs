use eframe::egui::{Color32, Grid, ScrollArea, Ui, Vec2, Vec2b};
use super::return_colors;

pub fn display_job_builder_page(ui: &mut Ui) {
    ui.add_space(15.0);
    ScrollArea::vertical()
        .max_height(f32::INFINITY)
        .max_width(680.0)
        .auto_shrink(Vec2b::new(false, false))
        .show(ui, |ui|
    {
        ui.vertical_centered(|ui| {
            ui.label("Job Builder");
            ui.group(|ui| {
                Grid::new("job builder grid")
                    .spacing(Vec2::new(4., 6.))
                    .min_col_width(150.)
                    .max_col_width(150.)
                    .with_row_color(|num, style| return_colors(num, style))
                    .num_columns(2)
                    .show(ui, |ui| {
                        ui.colored_label(Color32::LIGHT_RED, "Libre Office");
                        ui.checkbox(&mut false, "");
                        ui.end_row();

                        ui.colored_label(Color32::LIGHT_RED, "SEB");
                        ui.checkbox(&mut false, "");
                        ui.end_row();

                        ui.colored_label(Color32::LIGHT_RED, "CPS");
                        ui.checkbox(&mut false, "");
                        ui.end_row();
                        
                        ui.colored_label(Color32::LIGHT_RED, "Data Transfer");
                        ui.checkbox(&mut false, "");
                        ui.end_row();
                        
                        ui.colored_label(Color32::LIGHT_RED, "Data Transfer");
                        ui.checkbox(&mut false, "");
                        ui.end_row();
                    });
            });
        });
    });
}
