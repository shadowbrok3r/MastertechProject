use eframe::egui::{TextEdit, Ui};
use log::info;

use crate::app_state::MastertechContext;


impl MastertechContext{
    pub fn output_console(&mut self, ui: &mut Ui) { 
        self.ctx.request_repaint();
        // setup_terminal(ui, &self.output_text).unwrap();
        if let Ok(data) = self.prestashop_api_rx.try_recv(){
            if data.is_object(){
                info!("data: {:?}", data);
                // self.output_text.clear();
                self.output_text += serde_json::to_string(&data.as_object()).unwrap().as_str();
            }else{
                let res: String = serde_json::from_value(data).unwrap();
                self.output_text += res.as_str();
            }
        }
        ui.add_sized(ui.available_size(), TextEdit::multiline(&mut self.output_text.to_string()).hint_text("Output"));
    }
}