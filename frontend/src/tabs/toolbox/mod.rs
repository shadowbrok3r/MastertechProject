use eframe::egui::Ui;
use mtechserver::webworker::Input;
use crate::app_state::MtechServerContext;

impl MtechServerContext {
    pub fn toolbox(&mut self, ui: &mut Ui){
        ui.ctx().request_repaint();
        
        if ui.button("Get storage buckets").clicked() {
            if let Some(bridge) = &self.bridge {
                bridge.send(Input {
                    url: "https://storage-api.master-tech.app".to_string(),
                    access_key: "DMAZwz4511ezKqEiF2vy".to_string(),
                    secret_key: "lUVgT6KPAR7uPZriAC1QPqSTB9aW12oAmgegk6gO".to_string(),
                })
            }
        }
    }
}

