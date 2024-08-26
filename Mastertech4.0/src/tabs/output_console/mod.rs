use eframe::egui::{text::LayoutJob, Color32, FontId, Stroke, TextEdit, TextFormat, Ui};

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

        self.ctx.request_repaint();
        let mut layouter = |ui: &Ui, txt: &str, wrap_width: f32| {
            let mut layout_job = LayoutJob::default();
            layout_job.wrap.max_width = wrap_width;

            if txt.contains("Copying ") {
                // Find the position of "Copying" in the text
                if let Some(start_idx) = txt.find("Copying ") {
                    let end_idx = start_idx + "Copying ".len();
        
                    // Append the part before "Copying"
                    if start_idx > 0 {
                        layout_job.append(&txt[..start_idx], 0.0, TextFormat::default());
                    }
        
                    // Append "Copying" with red color
                    layout_job.append("Copying ", 4.0, TextFormat::simple(FontId::default(), Color32::LIGHT_RED));
        
                    // Append the rest of the text after "Copying"
                    if end_idx < txt.len() {
                        layout_job.append(&txt[end_idx..], 0.0, TextFormat::default());
                    }
                }
            } else {
                layout_job.append(txt, 0.0, TextFormat::default());
            }
            ui.fonts(|f| f.layout_job(layout_job))
        };

        // setup_terminal(ui, &self.output_text).unwrap();
        // if let Ok(data) = self.prestashop_api_rx.try_recv() { let res: String = serde_json::from_value(data).unwrap(); self.output_text += res.as_str(); }

        ui.add_sized(ui.available_size(), 
            TextEdit::multiline(&mut self.output_text.to_string())
                .font(FontId::proportional(12.0))
                .hint_text("Output")
                .layouter(&mut layouter)
        );
    }
}