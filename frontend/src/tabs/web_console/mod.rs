use egui::Ui;
use log::info;
// use ratatui::layout::{Constraint, Direction, Layout, Rect};
use wasm_bindgen_futures::spawn_local;
use websockets::TerminalFrontend;
use crate::{app_state::MtechServerContext, utilities::get_other::get_connected_clients};

pub mod websockets;
pub mod charts;

impl MtechServerContext {
    pub fn web_console(&mut self, ui: &mut Ui){
        let ctx = ui.ctx().clone();
        ctx.request_repaint();
        
        // self.terminal.draw(|frame| {
        //         let area = frame.size();
        //         if let Some(frontend) = &mut self.terminal_frontend {
        //           frontend.ui(ui, area, frame);
        //         }
        //     })
        // .expect("epic fail");

        if let Some(frontend) = &mut self.terminal_frontend {
            frontend.ui(ui);
        }
        ui.vertical_centered(|ui | {
            if ui.button("Connect").clicked()
            {
                if let Some(db) = self.database.clone(){
                    let usr = self.current_user.clone();
                    if let Some(user) = usr{
                        let tx = self.connected_clients_tx.clone();
                        spawn_local(async move {
                            get_connected_clients(db, tx, user).await.unwrap();
                        });
                    }
                }
            }
        });

        // ui.add( self.terminal.backend_mut());

        let wakeup = move || ctx.request_repaint();
        for (client_id, _client) in self.clients.iter(){
            if ui.button(client_id).clicked(){
                let url = format!("ws://127.0.0.1:8081/websocket?role=master&room_id={}", client_id);
                info!("url: {:?}", url.clone());
                match ewebsock::connect_with_wakeup(&url, Default::default(), wakeup.clone()) {
                    Ok((ws_sender, ws_receiver)) => {
                        self.terminal_frontend = Some(TerminalFrontend::new(ws_sender, ws_receiver));
                        self.error.clear();
                    }
                    Err(error) => {
                        log::error!("Failed to connect to {:?}: {}", &self.url, error);
                        self.error = error;
                    }
                };
            }
        }

        if !self.error.is_empty() {
            egui::TopBottomPanel::top("error").show(ui.ctx(), |ui| {
                ui.horizontal(|ui| {
                    ui.label("Error:");
                    ui.colored_label(egui::Color32::RED, &self.error);
                });
            });
        }
    }
}