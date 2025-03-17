use crate::app_state::MastertechContext;
use eframe::egui::{Ui, CentralPanel};
use ratatui::widgets::Paragraph;

impl MastertechContext{
    pub fn egui_terminal(&mut self, ui: &mut Ui) {
        
        self.shared_ctx.terminal.draw(|f| {
            // TerminalApp::default()
            let size = f.area();
            f.render_widget(Paragraph::new("Hello from Ratatui!"), size);
        }).unwrap();

        CentralPanel::default().show_inside(ui, |ui| {
            ui.add(self.shared_ctx.terminal.backend_mut());
        });
    }
}