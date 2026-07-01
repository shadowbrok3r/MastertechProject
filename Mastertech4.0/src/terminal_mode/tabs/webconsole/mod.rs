use crate::terminal_mode::{context::TerminalContext, events::action_handler::{get_update_sender, ActionHandler, WidgetId}, styling::ThemeRole, widgets::button::Button};
use crossbeam::channel::{Receiver, Sender};
use database::{schema::ConnectedClient, websocket_url_with_room, WS_MASTER_URL, WS_MASTER_URL_LOCAL};
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
    /// Full client metadata keyed by connection_string — used to get
    /// `local_ip` / `tcp_port` when opening a direct-TCP admin session.
    pub client_map: HashMap<String, ConnectedClient>,
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
            get_clients_btn: Button::new("Get Clients",WidgetId("GetClients".to_owned())).theme(ThemeRole::Tertiary),
            ws_clients: HashMap::new(),
            client_map: HashMap::new(),
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
                // if client.connected && client.connection_string != crate::filesystem::get_client_hash().connection_string 
                // {
                    self.ws_clients.insert(
                        client.connection_string.clone(),
                        Button::new(&client.connection_string, WidgetId(client.connection_string.clone())).theme(ThemeRole::Neutral),
                    );
                    // Store full client info so we can look up local_ip/tcp_port
                    // when opening a connection.
                    self.client_map.insert(client.connection_string.clone(), client.clone());
                // }
            }
            let _ = get_update_sender().try_send(self.widget_id());
        }
        // Poll buffer_rx for new frames
        if let Some(ref mut buffer_rx) = self.buffer_rx {
            while let Ok((frame_count, buffer)) = buffer_rx.try_recv() {
                log::info!("Rendering remote buffer, frame_count={}", frame_count);
                self.remote_buffer = Some(buffer);
            }
        }
    }

    /// Open a live remote terminal session for the given client.
    /// Prefers a direct TCP connection when `local_ip` and `tcp_port` are
    /// published; falls back to the WebSocket relay otherwise.
    pub fn start_remote_connection(&mut self, connection_string: String) {
        if let Some(client) = self.client_map.get(&connection_string).cloned() {
            if let (Some(ip), Some(port)) = (client.local_ip.clone(), client.tcp_port) {
                log::info!(
                    "WebconsoleTab -> using direct TCP {}:{} for {}",
                    ip, port, connection_string
                );
                self.start_remote_tcp(connection_string, ip, port);
                return;
            }
        }
        log::info!(
            "WebconsoleTab -> no TCP address for {}; using WS relay",
            connection_string
        );
        self.start_remote_websocket(connection_string);
    }

    /// Open a direct TCP session to a remote client for terminal viewing.
    fn start_remote_tcp(&mut self, connection_string: String, local_ip: String, tcp_port: u16) {
        use crate::transport::{FRAME_TAG_BINARY, HANDSHAKE_MAGIC, HANDSHAKE_VERSION};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let (buffer_tx, buffer_rx) = tokio::sync::mpsc::unbounded_channel();
        self.buffer_rx = Some(buffer_rx);

        let rx = self.event_rx.clone();
        tokio::spawn(async move {
            let addr = format!("{local_ip}:{tcp_port}");
            log::info!("WebconsoleTab -> dialing TCP {addr} for {connection_string}");

            let stream = match tokio::net::TcpStream::connect(&addr).await {
                Ok(s) => s,
                Err(e) => {
                    log::error!("WebconsoleTab -> TCP connect {addr} failed: {e}");
                    return;
                }
            };
            stream.set_nodelay(true).ok();

            let (mut read_half, mut write_half) = stream.into_split();

            // Send handshake: MTRX magic + version + u32 LE len + connection_string
            let id_bytes = connection_string.as_bytes();
            let mut handshake = Vec::with_capacity(4 + 1 + 4 + id_bytes.len());
            handshake.extend_from_slice(HANDSHAKE_MAGIC);
            handshake.push(HANDSHAKE_VERSION);
            handshake.extend_from_slice(&(id_bytes.len() as u32).to_le_bytes());
            handshake.extend_from_slice(id_bytes);
            if let Err(e) = write_half.write_all(&handshake).await {
                log::error!("WebconsoleTab -> handshake write failed: {e}");
                return;
            }
            log::info!("WebconsoleTab -> TCP handshake sent for {connection_string}");

            // Spawn a task that forwards terminal events (keyboard/mouse) to the client.
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    if let Ok(evt) = rx.try_recv() {
                        if let Ok(event_bytes) = serde_json::to_vec(&evt) {
                            let total_len = (event_bytes.len() as u32).saturating_add(1);
                            if write_half.write_all(&total_len.to_le_bytes()).await.is_err() { break; }
                            if write_half.write_all(&[FRAME_TAG_BINARY]).await.is_err() { break; }
                            if write_half.write_all(&event_bytes).await.is_err() { break; }
                        }
                    }
                }
            });

            // Read incoming frames: ratatui buffer (or egui) frames from the client.
            loop {
                let total_len = match read_half.read_u32_le().await {
                    Ok(n) if n > 0 => n,
                    _ => break,
                };
                let _tag = match read_half.read_u8().await {
                    Ok(t) => t,
                    Err(_) => break,
                };
                let payload_len = (total_len - 1) as usize;
                let mut payload = vec![0u8; payload_len];
                if read_half.read_exact(&mut payload).await.is_err() { break; }

                if let Ok(buf_msg) = decode_buffer(&payload) {
                    log::debug!(
                        "WebconsoleTab TCP -> frame {} ({} bytes)",
                        buf_msg.frame_count, payload_len
                    );
                    if buffer_tx.send((buf_msg.frame_count, buf_msg.buffer.into())).is_err() {
                        break;
                    }
                }
            }
            log::info!("WebconsoleTab -> TCP session closed for {connection_string}");
        });
    }

    // Start WebSocket connection for a specific client
    fn start_remote_websocket(&mut self, connection_string: String) {        let (buffer_tx, buffer_rx) = tokio::sync::mpsc::unbounded_channel();
        
        self.buffer_rx = Some(buffer_rx);
        // self.event_tx = Some(event_tx);

        let connection_url = websocket_url_with_room(
            if cfg!(debug_assertions) {
                WS_MASTER_URL_LOCAL
            } else {
                WS_MASTER_URL
            },
            &connection_string,
            "master",
        );
        log::warn!("Connection URL: {connection_url}");
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
                Err(e) => log::error!("Failed to connect remote WebSocket: {e:?}"),
            }
        });
    }
}
