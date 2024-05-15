use eframe::egui::{TextEdit, Ui};

use crate::app_state::MastertechContext;


impl MastertechContext{
    pub fn websockets(&mut self, ui: &mut Ui) {
        ui.vertical_centered(|ui| {
            ui.heading("Websocket Stuffs");
            TextEdit::singleline(&mut self.github_issue_title)
                .hint_text("Issue Title")
                .show(ui);

            ui.add_space(12.0); 

            ui.heading("Description");
            TextEdit::multiline(&mut self.github_issue_descript)
                .hint_text("Explain your issue")
                .show(ui);

        });
        
    }
}