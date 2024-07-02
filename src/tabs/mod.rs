use eframe::egui::Ui;
use log::info;
use crate::app_state::MastertechContext;
use std::{sync::atomic::Ordering, thread::spawn}; 
use github::self_updater::run;

pub mod scripts;
pub mod output_console;
pub mod quality_check;
pub mod mastertech_website;
pub mod system_information;
pub mod file_browser;
pub mod minidump;
pub mod puffin_profiler;
pub mod tur_sheet;
pub mod github;
pub mod websockets;

impl MastertechContext {
    pub fn simple_demo_menu(&mut self, ui: &mut Ui) {
        ui.label("Secret menu... -.-");
        ui.menu_button("Sub menu", |ui| {
            ui.label("(.)(.)");
        });
        if ui.button("update").clicked(){
            let (tx, rx) = crossbeam::channel::bounded(1);

            spawn(move ||{
                match run(){
                    Ok(response) => {
                        match tx.send((response.0, response.1)){
                            Ok(_) => drop(tx),
                            Err(e) => info!("{e}"),
                        }
                    },
                    Err(e) => info!("err: {e}"),
                }
            });
            
            if let Ok(res) = rx.recv(){
                self.output_text = format!("Status: \n     {}\nReleases:\n     {}", &res.1.to_string(), &res.0.to_string());
            }
        }
    }

    pub fn file_browser_popup(&mut self, ui: &mut Ui) {
        let current_state = self.show_deferred_viewport.load(Ordering::Relaxed);
        let new_state = !current_state; // Toggle the state: if it's true, make it false, and vice versa

        if current_state{
            if ui.button("Attach File Browser").clicked(){self.show_deferred_viewport.store(new_state, Ordering::Relaxed);}
        }else {
            if ui.button("Detach File Browser").clicked(){self.show_deferred_viewport.store(new_state, Ordering::Relaxed);}
        }
    }
}
