use eframe::egui::{TextEdit, Ui};

use crate::app_state::MastertechContext;


impl MastertechContext{
    pub fn output_console(&mut self, ui: &mut Ui) { 
        self.ctx.request_repaint();
        // setup_terminal(ui, &self.output_text).unwrap();

        ui.add_sized(ui.available_size(), TextEdit::multiline(&mut self.output_text.to_string()).hint_text("Output"));
    }
    
}