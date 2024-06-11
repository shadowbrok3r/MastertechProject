use eframe::egui::{Color32, Stroke, TextEdit, Ui};

use crate::app_state::MastertechContext;


impl MastertechContext{
    pub fn output_console(&mut self, ui: &mut Ui) { 
        ui.style_mut().visuals.selection.stroke.color =  Color32::BLACK;
        ui.style_mut().visuals.selection.bg_fill = Color32::from_rgb(120, 10, 120);
        ui.style_mut().visuals.widgets.inactive.fg_stroke =  Stroke::new(1.0, Color32::WHITE);
        ui.style_mut().visuals.widgets.inactive.weak_bg_fill =  Color32::from_rgb(20, 20, 25);
        ui.style_mut().visuals.widgets.inactive.bg_stroke =  Stroke::new(1.0, Color32::from_rgb(80, 80, 80));
        ui.style_mut().visuals.widgets.open.bg_fill =  Color32::from_black_alpha(50);
        ui.style_mut().visuals.widgets.open.weak_bg_fill =  Color32::from_black_alpha(50);
        ui.style_mut().visuals.widgets.active.weak_bg_fill =  Color32::from_rgb(30,30,30);
        ui.style_mut().visuals.widgets.hovered.weak_bg_fill =  Color32::TRANSPARENT;
        ui.style_mut().visuals.widgets.hovered.bg_fill =  Color32::from_rgb(12, 12, 12);
        ui.style_mut().visuals.widgets.hovered.bg_stroke =  Stroke::new(1.0, Color32::from_rgb(200, 20, 200));

        self.ctx.request_repaint();
        // setup_terminal(ui, &self.output_text).unwrap();
        if let Ok(data) = self.prestashop_api_rx.try_recv(){

            self.output_text += serde_json::to_string(&data).unwrap().as_str();

            // let res: String = serde_json::from_value(data).unwrap();
            // self.output_text += res.as_str();
            
        }
        ui.add_sized(ui.available_size(), 
            TextEdit::multiline(&mut self.output_text.to_string())
                .hint_text("Output")
                .code_editor()
                .text_color(Color32::from_rgb(100, 20, 200))
        );
    }
}