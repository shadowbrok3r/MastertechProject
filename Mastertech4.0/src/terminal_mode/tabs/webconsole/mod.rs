use crate::terminal_mode::{context::TerminalContext, data::LocalTermEvent, events::action_handler::WidgetId, styling::CATPPUCCINTHEME, widgets::button::Button};
use crossbeam::channel::{Receiver, Sender};
use database::{WS_MASTER_URL, schema::ConnectedClient};
use displays::remote_viewer::decode_buffer;
use ewebsock::{WsEvent, WsMessage};
use std::{collections::HashMap, sync::{Arc, Mutex}};
use ratatui::buffer::Buffer;
use reqwest::Client;

pub mod action_handler;
pub mod render;

pub struct WebconsoleTab <'a> {
    pub get_clients_btn: Button<'a>,
    pub ws_clients: HashMap<String, Button<'a>>,
    pub current_client: Option<ConnectedClient>,
    pub _client: Client,
    pub page_state: PageState,
    ctx: Arc<Mutex<TerminalContext>>,
    pub remote_buffer: Option<Buffer>, // Store the latest received buffer
    pub buffer_rx: Option<tokio::sync::mpsc::UnboundedReceiver<(u64, Buffer)>>, // Receive remote buffers
    pub connected_clients_tx: Sender<Vec<ConnectedClient>>,
    pub connected_clients_rx: Receiver<Vec<ConnectedClient>>,
}

// Define a page state to track what’s displayed on the right side
#[derive(Debug, PartialEq)]
pub enum PageState {
    None,                  // No client selected
    RemoteTerminal(String), // Rendering remote terminal for a specific client (connection_string)
}

impl <'a> WebconsoleTab <'a> {
    pub fn new(_client: Client, ctx: Arc<Mutex<TerminalContext>>) -> Self {
        let (connected_clients_tx, connected_clients_rx) = crossbeam::channel::unbounded();
        Self {
            get_clients_btn: Button::new("Get Clients",WidgetId("GetClients".to_owned())).theme(CATPPUCCINTHEME),
            ws_clients: HashMap::new(),
            current_client: None,
            _client,
            ctx,
            page_state: PageState::None,
            remote_buffer: None,
            buffer_rx: None,
            connected_clients_tx, 
            connected_clients_rx
        }
    }

    pub fn receive(&mut self) {
        if let Ok(clients) = self.connected_clients_rx.try_recv() {
            for client in clients.iter() {
                if client.connected {
                    self.ws_clients.insert(
                        client.connection_string.clone(),
                        Button::new(&client.connection_string, WidgetId(client.connection_string.clone())).theme(CATPPUCCINTHEME),
                    );
                }
            }
        }

        // Poll buffer_rx for new frames
        if let Some(ref mut buffer_rx) = self.buffer_rx {
            while let Ok((frame_count, buffer)) = buffer_rx.try_recv() {
                log::info!("Rendering remote buffer, frame_count={}", frame_count);
                log::info!("Buffer size: {}x{}", buffer.area.width, buffer.area.height);
                // Log a sample of the buffer content
                for (i, cell) in buffer.content().iter().take(10).enumerate() {
                    let x = i % buffer.area.width as usize;
                    let y = i / buffer.area.width as usize;
                    log::info!("Cell at ({}, {}): {:?}", x, y, cell);
                }
                self.remote_buffer = Some(buffer);
            }
        }
    }

    // Start WebSocket connection for a specific client
    fn start_remote_websocket(&mut self, connection_string: String) {
        let (buffer_tx, buffer_rx) = tokio::sync::mpsc::unbounded_channel();
        self.buffer_rx = Some(buffer_rx);

        let connection_url = format!("{WS_MASTER_URL}&room_id={}", connection_string);
        let (start_tx, mut start_rx) = tokio::sync::mpsc::unbounded_channel::<bool>();
        let (conn_tx, mut conn_rx) = tokio::sync::mpsc::unbounded_channel::<(bool, String)>();
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<LocalTermEvent>();
        let (shutdown_tx, mut shutdown_rx) = tokio::sync::broadcast::channel(1);

        tokio::spawn(async move {
            let connection = ewebsock::connect(connection_url, ewebsock::Options::default());
            match connection {
                Ok((mut sender, mut receiver)) => {
                    log::info!("Sending READY to {}", connection_string);
                    sender.send(WsMessage::Text("READY".to_string()));
                    let mut ready = false;
                    log::info!("Remote WebSocket started for {}", connection_string);

                    loop {
                        // Poll WebSocket events synchronously
                        while let Some(event) = receiver.try_recv() {
                            log::info!("Received WebSocket event: {:?}", event);
                            match event {
                                WsEvent::Opened => log::info!("WebSocket connection opened"),
                                WsEvent::Error(e) => log::info!("WebSocket error: {:?}", e),
                                WsEvent::Closed => {
                                    log::info!("WebSocket connection closed");
                                    break;
                                },
                                WsEvent::Message(ws_message) => {
                                    log::info!("Received message: {:?}", ws_message);
                                    match ws_message {
                                        WsMessage::Pong(data) => {
                                            log::info!("Received pong: {:?}", data);
                                        },
                                        WsMessage::Text(txt) => {
                                            log::info!("Text message: {}", txt);
                                            if txt == "READY" {
                                                ready = true;
                                                log::info!("Remote WebSocket ready");
                                            }
                                        },
                                        WsMessage::Binary(buffer_array) if ready => {
                                            log::info!("Binary message received, len={}", buffer_array.len());
                                            if let Ok(buffer_msg) = decode_buffer(&buffer_array) {
                                                log::info!("Decoded buffer, frame_count={}", buffer_msg.frame_count);
                                                if buffer_tx.send((buffer_msg.frame_count, buffer_msg.buffer)).is_err() {
                                                    log::warn!("Failed to send remote buffer to buffer_tx");
                                                    break;
                                                }
                                            } else {
                                                log::warn!("Failed to decode buffer: {:?}", buffer_array);
                                            }
                                        },
                                        WsMessage::Binary(bin) => {
                                            log::info!("Binary received but not ready: {:?}", bin);
                                        },
                                        _ => {
                                            log::info!("Unhandled message type");
                                        }
                                    }
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
