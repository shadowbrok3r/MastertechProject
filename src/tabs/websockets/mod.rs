use std::mem;

use eframe::egui::{CentralPanel, Color32, Context, Key, TopBottomPanel, Ui};
use ewebsock::{WsEvent, WsMessage, WsReceiver, WsSender};

use crate::app_state::MastertechContext;


impl MastertechContext{
    pub fn websockets(&mut self, ui: &mut Ui) {
        let db_tx = self.db_tx.clone();

        if self.current_user.is_none(){
            let _ = self.app_state_tx.send(crate::app_state::AppState::NoAuth("No User".to_string()));
        }
        
        ui.vertical_centered(|ui| {
            let ctx = ui.ctx().clone();
            let wakeup = move || ctx.request_repaint(); // wake up UI thread on new message

            if ui.button("Connect").clicked()
            {
                match ewebsock::connect_with_wakeup(&self.url, Default::default(), wakeup) {
                    Ok((ws_sender, ws_receiver)) => {
                        self.frontend = Some(WebConsoleFrontend::new(ws_sender, ws_receiver));
                        self.error.clear();
                    }
                    Err(error) => {
                        log::error!("Failed to connect to {:?}: {}", &self.url, error);
                        self.error = error;
                    }
                };
            }
            if !self.error.is_empty() {
                TopBottomPanel::top("error").show_inside(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Error:");
                        ui.colored_label(Color32::RED, &self.error);
                    });
                });
            }
    
            if let Some(frontend) = &mut self.frontend {
                frontend.ui(ui);
            }


        });
        
    }
}

pub struct WebConsoleFrontend {
    ws_sender: WsSender,
    ws_receiver: WsReceiver,
    events: Vec<WsEvent>,
    text_to_send: String,
}

impl WebConsoleFrontend {
    pub fn new(ws_sender: WsSender, ws_receiver: WsReceiver) -> Self {
        Self {
            ws_sender,
            ws_receiver,
            events: Default::default(),
            text_to_send: Default::default(),
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        while let Some(event) = self.ws_receiver.try_recv() {
            self.events.push(event);
        }

        CentralPanel::default().show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Message to send:");
                if ui.text_edit_singleline(&mut self.text_to_send).lost_focus()
                    && ui.input(|i| i.key_pressed(Key::Enter))
                {
                    self.ws_sender
                        .send(WsMessage::Text(mem::take(&mut self.text_to_send)));
                }
            });

            ui.separator();
            ui.heading("Received events:");
            for event in &self.events {
                match event{
                    WsEvent::Message(msg) => {
                        match msg{
                            WsMessage::Binary(bin) => {
                                ui.label(format!("{bin:?}"));
                            },
                            WsMessage::Text(txt) => {
                                ui.label(txt);
                            },
                            _ => {}
                        }
                    },
                    _ => {}
                }
                
            }
        });
    }
}