use eframe::egui::Ui;
use eframe::egui::{CentralPanel, Color32, Frame, Margin, Rounding, Stroke, TopBottomPanel, Vec2};
use crate::app_state::MtechServerContext;

pub mod storage_api;

impl MtechServerContext {
    pub fn toolbox(&mut self, ui: &mut Ui){
        ui.ctx().request_repaint();

        let mut inner_margin_top = Margin::default();
        inner_margin_top.top = 5.0;

        let top_panel_frame = Frame::default().inner_margin(inner_margin_top)
            .rounding(Rounding::same(10.0));


        let mut inner_margin = Margin::default();
        inner_margin.top = 3.0;
        inner_margin.left = 3.0;
        inner_margin.right = 3.0;
        inner_margin.bottom = 5.0;

        let panel_frame = Frame::default()
            .fill(Color32::from_rgb(12, 12, 14))
            .inner_margin(inner_margin)
            .rounding(Rounding::same(10.0))
            .stroke(Stroke::new(1.0, Color32::from_additive_luminance(50)));
        
        ui.style_mut().spacing.button_padding = Vec2::new(10.0, 3.0);

        TopBottomPanel::bottom("FileBrowserBottom").frame(top_panel_frame)
            .show_separator_line(false)
            .show_inside(ui, |ui| {
                ui.vertical_centered(|ui |
                {
                    self.file_system.show_progress(ui);
                })
            });

        ui.add_space(10.0);
        CentralPanel::default().frame(panel_frame)
            .show_inside(ui, |ui| 
        {
            self.file_system.display(ui);
        });
    }
}

