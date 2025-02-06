use crate::app_state::MtechServerContext;
// use anyhow::{Error, Result};
use core::f32;
use eframe::egui::{
    CentralPanel, Color32, ComboBox, Frame, Margin, ScrollArea, SidePanel, TextStyle,
    TopBottomPanel, Ui,
};

#[derive(Default, PartialEq)]
pub enum Tables {
    #[default]
    Task,
    Customer,
    ConnectedClient,
    Ticket,
    Computer,
    TaskNote,
}

impl MtechServerContext {
    pub fn query_builder(&mut self, ui: &mut Ui) {
        let s_frame = Frame::default();
        let _ = s_frame.inner_margin(Margin::same(20));
        let _ = s_frame.outer_margin(Margin::same(10));
        SidePanel::left("left-panel-query-builder")
            .frame(s_frame)
            .max_width(130.)
            .resizable(false)
            .show_inside(ui, |ui| {
                ui.vertical_centered_justified(|_ui| {});
            });

        TopBottomPanel::top("top-panel-query-builder").show_inside(ui, |ui| {
            ui.vertical_centered(|ui| ui.heading("Query Builder"));
        });

        let c_frame = Frame::default();
        let _ = c_frame.inner_margin(Margin::same(40));
        let _ = c_frame.outer_margin(Margin::same(15));

        CentralPanel::default()
            .frame(c_frame)
            .show_inside(ui, |ui| {
                let available_height = ui.available_height();
                let font_id = TextStyle::Body.resolve(ui.style());
                let row_height = ui.fonts(|f| f.row_height(&font_id)) + ui.spacing().item_spacing.y;
                let total_rows = (available_height / row_height).floor() as usize;
                ScrollArea::new([false, true])
                    .max_width(f32::INFINITY)
                    .auto_shrink(false)
                    .show_rows(ui, row_height, total_rows, |ui, _row_range| {
                        ui.add_space(ui.available_width() / 3.0);
                        ui.horizontal_top(|ui| {
                            // Run 'INFO FOR TABLE x' Query to get field names instead of using *
                            ui.colored_label(Color32::LIGHT_RED, "SELECT * FROM ");
                            let mut selected = Tables::default();
                            ComboBox::new("table selection", "")
                                .selected_text("Task Table")
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut selected, Tables::Task, "Task Table");
                                    ui.selectable_value(
                                        &mut selected,
                                        Tables::Customer,
                                        "Customer Table",
                                    );
                                    ui.selectable_value(
                                        &mut selected,
                                        Tables::Ticket,
                                        "Service Table",
                                    );
                                    ui.selectable_value(
                                        &mut selected,
                                        Tables::Computer,
                                        "Computer Table",
                                    );
                                    ui.selectable_value(
                                        &mut selected,
                                        Tables::TaskNote,
                                        "TaskNote Table",
                                    );
                                    ui.selectable_value(
                                        &mut selected,
                                        Tables::ConnectedClient,
                                        "Connected Client Table",
                                    );
                                });
                        });
                    });
            });
    }
}
