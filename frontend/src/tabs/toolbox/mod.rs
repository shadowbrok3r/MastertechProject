use eframe::egui::Ui;
use log::info;
use mtechserver::webworker::Input;
use crate::app_state::MtechServerContext;

pub mod storage_api;

impl MtechServerContext {
    pub fn toolbox(&mut self, ui: &mut Ui){
        ui.ctx().request_repaint();
        
        if ui.button("Get storage buckets").clicked() {
            if let Some(bridge) = &self.bridge {
                bridge.send(Input {
                    url: "https://storage-api.master-tech.app".to_string(),
                    access_key: "DMAZwz4511ezKqEiF2vy".to_string(),
                    secret_key: "lUVgT6KPAR7uPZriAC1QPqSTB9aW12oAmgegk6gO".to_string(),
                });
            }
        }

        let data_update = self.data_update.as_mut().unwrap();
        if let Some(items) = data_update.take() { 
            info!("items: {items:?}");
            self.file_system.build_file_system(items);
        }
        self.file_system.display(ui);
    }
}

