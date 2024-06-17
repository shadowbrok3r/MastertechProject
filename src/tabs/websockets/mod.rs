use std::mem;
use eframe::egui::{CentralPanel, Color32, Key, TopBottomPanel, Ui};
use ewebsock::{WsEvent, WsMessage, WsReceiver, WsSender};
use serde::{Deserialize, Serialize};
use surrealdb::{opt::RecordId, sql::{Thing, Uuid}};
use tokio::spawn;
use tracing::info;

use crate::{app_state::MastertechContext, database::schema::{ClientId, UserId, CONNECTED_CLIENT_TABLE}};
use tui_input::Input;
pub mod websocket;

#[derive(Serialize, Debug, Clone, Deserialize)]
pub struct ConnectedClient{
    pub id: Option<ClientId>,
    pub assigned_user: Option<UserId>,
    pub hostname: Option<String>,
    pub client_identifier: Option<String>,
    pub uuid: Option<String>
}

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
            let ctx = ui.ctx().clone();
            let wakeup = move || ctx.request_repaint(); // wake up UI thread on new message

            if ui.button("Connect").clicked()
            {
                if let Some(db) = self.database.clone(){
                    self.client_uuid = *Uuid::new_v4();
                    let uuid = self.client_uuid.clone();
                    let usr = self.current_user.clone().unwrap().id;
                    self.url = format!("ws://127.0.0.1:8081/websocket?room_id={}&role=client", &self.client_uuid);
                    let hostname = &self.system_info.hostname;
                    let client_identifier = format!("{hostname}-{}", uuid.to_string().split_at(36-12).1);

                    let connected_client = ConnectedClient {
                        id: Some(ClientId(Thing::from((CONNECTED_CLIENT_TABLE, client_identifier.as_str())))),
                        assigned_user: Some(usr.clone()),
                        hostname: Some(hostname.clone()),
                        client_identifier: Some(client_identifier.clone()), 
                        uuid: Some(uuid.to_string())
                    };

                    info!("Client: {:#?}", connected_client);

                    spawn(async move {
                        // db.database.set("connected_client", connected_client.id.clone()).await.unwrap();
                        // info!("connected_client: {connected_client:#?}");
                        // db.database.set("user", usr.clone()).await.unwrap();
                        // info!("usr: {usr:?}");
                        
                        let res = db.database
                            .query("CREATE connected_client CONTENT $content")
                            .bind(("content", connected_client.clone()))
                            .query("UPDATE user SET connected_clients += $connected_client WHERE id == $user")
                            .bind(("connected_client", connected_client.id))
                            .bind(("user", usr.clone()))
                            .await;

                        info!("Query done: {res:#?}");
                    });
                }
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