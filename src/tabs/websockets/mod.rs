use std::{env, mem};
use eframe::egui::{CentralPanel, Color32, Key, TopBottomPanel, Ui};
use ewebsock::{WsEvent, WsMessage, WsReceiver, WsSender};
use log::debug;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use surrealdb::{opt::RecordId, sql::{Thing, Uuid}};
use tokio::spawn;
use tracing::info;

use crate::{app_state::MastertechContext, database::schema::{ClientId, ComputerId, ConnectedClient, User, UserId, COMPUTER_TABLE, CONNECTED_CLIENT_TABLE}};
use tui_input::Input;
pub mod websocket;



impl MastertechContext{
    pub fn websockets(&mut self, ui: &mut Ui) {
        // if let Some(frontend) = &mut self.frontend {
        //     self.terminal
        //         .draw(|frame| {
        //             let _area = frame.size();
        //             // render_chart1(frame, area, &app);
        //             frontend.ui(ui);
        //         })
        //     .expect("epic fail");
        // }
        // ui.add( self.terminal.backend_mut());
        // self.terminal.show_cursor().unwrap();

        let _db_tx = self.db_tx.clone();

        if self.current_user.is_none(){
            let _ = self.app_state_tx.send(crate::app_state::AppState::NoAuth("No User".to_string()));
        }
        
        ui.vertical_centered(|ui| {
            if ui.button("Connect").clicked()
            {
                if let Some(db) = self.database.clone(){
                    let client_hash = generate_client_id(self.system_info.hostname.clone(), self.system_info.cpu.trim().to_string());
                    let url_string = format!("{}:{}", self.system_info.hostname.clone(), client_hash.split_at(9).0);
                    info!("url_string: {}", url_string.clone());

                    self.url = Some(format!("ws://127.0.0.1:8081/websocket?room_id={}&role=client",  url_string.clone()));
                    info!("url: {:?}", self.url.clone());
                    let computer_id = &self.system_info.id.clone().unwrap_or( // i need to first check if a computer exists with a customer id or something..
                        ComputerId(
                            Thing::from(
                                (COMPUTER_TABLE,  url_string.clone().as_str())
                            )
                        )
                    );
                    
                    self.client_uuid = Some(
                        ClientId(
                            Thing::from((CONNECTED_CLIENT_TABLE.to_string(), computer_id.0.id.clone()))
                        )
                    );

                    let connected_client = ConnectedClient {
                        id: self.client_uuid.clone(),
                        client_hash,
                        ..Default::default()
                    };


                    info!("Client: {:?}", connected_client);

                    let tx = self.connected_clients_tx.clone();
                    spawn(async move {
                        
                        let res: Result<Vec<ConnectedClient>, surrealdb::Error> = db.database
                            .query("CREATE connected_client CONTENT $content")
                            .bind(("content", connected_client.clone()))
                            .await
                            .unwrap().take(0);
                        match res{
                            Ok(data) => tx.try_send(data.clone()).unwrap(),
                            Err(e) => debug!("db error: {e:?}"),
                        }
                    });
                    
                    if let Some(url) = &self.url{
                        let ctx = ui.ctx().clone();
                        let wakeup = move || ctx.request_repaint(); // wake up UI thread on new message
                        match ewebsock::connect_with_wakeup(url, Default::default(), wakeup) {
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
                }

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

// Function to generate client ID
fn generate_client_id(hostname: String, cpu: String) -> String {
    let cpu_id = env::var("PROCESSOR_IDENTIFIER").unwrap_or_else(|_| "unknown-cpu".to_string());
    let combined = format!("{}-{}-{}", hostname, cpu, cpu_id);
    info!("combined: {}", combined.clone());
    let mut hasher = Sha256::new();
    hasher.update(combined.as_bytes());
    let result = hasher.finalize();
    let hex_string = hex::encode(result);
    info!("hex_string: {}", hex_string.clone());
    hex_string
}


pub struct WebConsoleFrontend {
    pub ws_sender: WsSender,
    pub ws_receiver: WsReceiver,
    pub events: Vec<WsEvent>,
    pub text_to_send: String,
    /// Position of cursor in the editor area.
    pub character_index: usize,
    /// Current value of the input box
    pub input: Input,
    /// Current input mode
    // pub input_mode: InputMode,
    /// History of recorded messages
    pub messages: Vec<String>,
}

impl WebConsoleFrontend {
    pub fn new(ws_sender: WsSender, ws_receiver: WsReceiver) -> Self {
        Self {
            ws_sender,
            ws_receiver,
            events: Default::default(),
            text_to_send: String::new(),
            input: Input::default(),
            // input_mode: InputMode::Normal,
            messages: Vec::new(),
            character_index: 0,
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

    // pub fn ui(&mut self, ui: &mut Ui, f: &mut Frame, area: Rect) {
    //     // if ui.text_edit_singleline(&mut self.text_to_send).lost_focus()
    //     //     && ui.input(|i| i.key_pressed(Key::Enter))
    //     // {
    //     //     self.ws_sender
    //     //         .send(WsMessage::Text(std::mem::take(&mut self.text_to_send)));
    //     // }

    //     while let Some(event) = self.ws_receiver.try_recv() {
    //         self.events.push(event);
    //     }

    //     let mut text: String = String::new();
    //     for event in &self.events {
    //         match event{
    //             WsEvent::Message(msg) => {
    //                 match msg{
    //                     WsMessage::Binary(bin) => {
    //                         text = format!("{bin:?}");
    //                     },
    //                     WsMessage::Text(txt) => {
    //                         text = txt.clone();
    //                     },
    //                     _ => {}
    //                 }
    //             },
    //             // WsEvent::Error(_) => todo!(),
    //             _ => {}
    //         }
            
    //     }

    //     let block = Block::default()
    //         .title(Title::from("MasterTech Web Console").alignment(Alignment::Center))
    //         .title(
    //             Title::from("X")
    //                 .alignment(Alignment::Center)
    //                 .position(Position::Bottom),
    //         )
    //         .borders(Borders::ALL)
    //         .border_set(border::THICK)
    //         .white().bg(Color::Black);

    //     let para = Paragraph::new(text)
    //         .centered()
    //         .block(block)
    //         .cyan()
    //         .on_black();

    //     // for event in &self.events {
    //     //     ui.label(format!("{event:?}"));
    //     // }
    //     // f.render_widget(para, area);
    //     let vertical = Layout::vertical([
    //         Constraint::Length(1),
    //         Constraint::Length(3),
    //         Constraint::Min(1),
    //     ]);
    //     let [help_area, input_area, messages_area] = vertical.areas(f.size());
    
    //     let (msg, style) = match self.input_mode {
    //         InputMode::Normal => (
    //             vec![
    //                 "Press ".into(),
    //                 "q".bold(),
    //                 " to exit, ".into(),
    //                 "e".bold(),
    //                 " to start editing.".bold(),
    //             ],
    //             Style::default().add_modifier(Modifier::RAPID_BLINK),
    //         ),
    //         InputMode::Editing => (
    //             vec![
    //                 "Press ".into(),
    //                 "Esc".bold(),
    //                 " to stop editing, ".into(),
    //                 "Enter".bold(),
    //                 " to record the message".into(),
    //             ],
    //             Style::default(),
    //         ),
    //     };
    //     let text = Text::from(Line::from(msg)).patch_style(style);
    //     let help_message = Paragraph::new(text);
    //     f.render_widget(help_message, help_area);
    
    //     let input = Paragraph::new(self.input.value())
    //         .style(match self.input_mode {
    //             InputMode::Normal => Style::default(),
    //             InputMode::Editing => Style::default().fg(Color::Yellow),
    //         })
    //         .block(Block::bordered().title("Input"));
    //     f.render_widget(input, input_area);
    //     match self.input_mode {
    //         InputMode::Normal =>
    //             // Hide the cursor. `Frame` does this by default, so we don't need to do anything here
    //             {}
    
    //         InputMode::Editing => {
    //             // Make the cursor visible and ask ratatui to put it at the specified coordinates after
    //             // rendering
    //             #[allow(clippy::cast_possible_truncation)]
    //             f.set_cursor(
    //                 // Draw the cursor at the current position in the input field.
    //                 // This position is can be controlled via the left and right arrow key
    //                 input_area.x + self.character_index as u16 + 1,
    //                 // Move one line down, from the border to the input line
    //                 input_area.y + 1,
    //             );
    //         }
    //     }
    
    //     let messages: Vec<ListItem> = self
    //         .messages
    //         .iter()
    //         .enumerate()
    //         .map(|(i, m)| {
    //             let content = Line::from(Span::raw(format!("{i}: {m}")));
    //             ListItem::new(content)
    //         })
    //         .collect();
    //     let messages = List::new(messages).block(Block::bordered().title("Messages"));
    //     f.render_widget(messages, messages_area);
    // }

    // pub fn handle_events(&mut self, ui: &mut Ui) {
    //     if ui.input(|i| i.key_released(Key::Q)) {
    //         panic!("HAVE A NICE WEEK");
    //     }
    //     if ui.input(|i| i.key_released(Key::ArrowRight)) {
    //         self.change_status();
    //     }
    //     if ui.input(|i| i.key_released(Key::ArrowLeft)) {
    //         self.items.unselect();
    //     }
    //     if ui.input(|i| i.key_released(Key::ArrowDown)) {
    //         self.items.next();
    //     }
    //     if ui.input(|i| i.key_released(Key::ArrowUp)) {
    //         self.items.previous();
    //     }
    //     if ui.input(|i| i.key_released(Key::G)) {
    //         self.go_top();
    //     }
    //     if ui.input(|i| i.key_released(Key::F)) {
    //         self.go_bottom();
    //     }
    // }
}