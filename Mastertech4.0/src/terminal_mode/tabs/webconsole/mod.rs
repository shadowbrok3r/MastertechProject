use crate::terminal_mode::{context::TerminalContext, events::action_handler::WidgetId, styling::CATPPUCCINTHEME, widgets::button::Button};
use crossbeam::channel::{Receiver, Sender};
use database::{WS_MASTER_URL, schema::ConnectedClient};
use displays::remote_viewer::{decode_buffer, ratagui::TerminalEvent};
use ewebsock::{WsEvent, WsMessage};
use std::{collections::HashMap, sync::{Arc, Mutex}};
use ratatui::{buffer::Buffer, layout::Rect};
use reqwest::Client;

pub mod action_handler;
pub mod render;

pub struct WebconsoleTab <'a> {
    pub get_clients_btn: Button<'a>,
    pub ws_clients: HashMap<String, Button<'a>>,
    // pub _current_client: Option<ConnectedClient>,
    pub _client: Client,
    pub page_state: PageState,
    pub _ctx: Arc<Mutex<TerminalContext>>,
    pub remote_buffer: Option<Buffer>, // Store the latest received buffer
    pub buffer_rx: Option<tokio::sync::mpsc::UnboundedReceiver<(u64, Buffer)>>, // Receive remote buffers
    pub connected_clients_tx: Sender<Vec<ConnectedClient>>,
    pub connected_clients_rx: Receiver<Vec<ConnectedClient>>,
    pub event_tx: Sender<TerminalEvent>,
    pub event_rx: Receiver<TerminalEvent>,
    pub client_area: Rect,
    pub show_side_panel: bool,
}

// Define a page state to track what’s displayed on the right side
#[derive(Debug, PartialEq)]
pub enum PageState {
    None,                  // No client selected
    RemoteTerminal(String), // Rendering remote terminal for a specific client (connection_string)
}

impl <'a> WebconsoleTab <'a> {
    pub fn new(_client: Client, _ctx: Arc<Mutex<TerminalContext>>) -> Self {
        let (connected_clients_tx, connected_clients_rx) = crossbeam::channel::unbounded();
        let (event_tx, event_rx) = crossbeam::channel::unbounded();
        Self {
            get_clients_btn: Button::new("Get Clients",WidgetId("GetClients".to_owned())).theme(CATPPUCCINTHEME),
            ws_clients: HashMap::new(),
            // current_client: None,
            _client,
            _ctx,
            page_state: PageState::None,
            remote_buffer: None,
            buffer_rx: None,
            connected_clients_tx, 
            connected_clients_rx,
            // event_tx: None,
            event_tx, event_rx,
            client_area: Rect::default(),
            show_side_panel: true
        }
    }

    pub fn receive(&mut self) {
        if let Ok(clients) = self.connected_clients_rx.try_recv() {
            for client in clients.iter() {
                // if client.connected && client.connection_string != crate::filesystem::get_client_hash()
                    // .connection_string 
                // {
                    self.ws_clients.insert(
                        client.connection_string.clone(),
                        Button::new(&client.connection_string, WidgetId(client.connection_string.clone())).theme(CATPPUCCINTHEME),
                    );
                // }
            }
        }
        // Poll buffer_rx for new frames
        if let Some(ref mut buffer_rx) = self.buffer_rx {
            while let Ok((frame_count, buffer)) = buffer_rx.try_recv() {
                log::info!("Rendering remote buffer, frame_count={}", frame_count);
                self.remote_buffer = Some(buffer);
            }
        }

        
    }

    // Start WebSocket connection for a specific client
    fn start_remote_websocket(&mut self, connection_string: String) {
        let (buffer_tx, buffer_rx) = tokio::sync::mpsc::unbounded_channel();
        
        self.buffer_rx = Some(buffer_rx);
        // self.event_tx = Some(event_tx);

        let connection_url = format!("{WS_MASTER_URL}&room_id={}", connection_string);
        let (_shutdown_tx, mut shutdown_rx) = tokio::sync::broadcast::channel(1);
        let rx = self.event_rx.clone();
        tokio::spawn(async move {
            let connection = ewebsock::connect(connection_url, ewebsock::Options::default());
            match connection {
                Ok((mut sender, receiver)) => {
                    log::info!("Sending READY to {}", connection_string);
                    sender.send(WsMessage::Text("READY".to_string()));
                    log::info!("Remote WebSocket started for {}", connection_string);

                    loop {
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                        
                        if let Ok(evt) = rx.try_recv(){
                            if let Ok(event) = serde_json::to_vec::<TerminalEvent>(&evt) {
                                log::info!("Sending evt to tui: {:?}", event);
                                sender.send(WsMessage::Binary(event));
                            }
                        }

                        // Poll WebSocket events synchronously
                        while let Some(event) = receiver.try_recv() {
                            log::info!("Received WebSocket event");
                            match event {
                                WsEvent::Message(ws_message) => {
                                    log::info!("Received message: {:?}", ws_message);
                                    match ws_message {
                                        WsMessage::Binary(buffer_array) => {
                                            log::info!("Binary message received, len={}", buffer_array.len());
                                            if let Ok(buffer_msg) = decode_buffer(&buffer_array) {
                                                log::info!("Decoded buffer, frame_count={}", buffer_msg.frame_count);
                                                if buffer_tx.send((buffer_msg.frame_count, buffer_msg.buffer.into())).is_err() {
                                                    log::warn!("Failed to send remote buffer to buffer_tx");
                                                    break;
                                                }
                                            } else {
                                                log::warn!("Failed to decode buffer: {:?}", buffer_array);
                                            }
                                        },
                                        WsMessage::Text(txt) => log::info!("Text message: {txt}"),
                                        WsMessage::Pong(_) => log::debug!("Received pong"),
                                        WsMessage::Ping(_) => log::debug!("Received ping"),
                                        WsMessage::Unknown(m) => log::info!("Unhandled message type: {m}"),
                                    }
                                },
                                WsEvent::Error(e) => log::info!("WebSocket error: {:?}", e),
                                WsEvent::Opened => log::info!("WebSocket connection opened"),
                                WsEvent::Closed => {
                                    log::info!("WebSocket connection closed");
                                    break;
                                },
                            }
                        }

                        
                        // Check for shutdown
                        tokio::select! {
                            Ok(()) = shutdown_rx.recv() => {
                                log::info!("Remote WebSocket shutdown");
                                break;
                            },
                            
                            else => {
                                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                            }
                        }
                    }
                },
                Err(e) => log::info!("Failed to connect remote WebSocket: {e:?}"),
            }
        });
    }
}
