//! Connection manager for WebSocket connections to Mastertech clients.
//!
//! Handles:
//! - WebSocket connection lifecycle
//! - Ping/pong tracking with 10-second timeout
//! - Message routing between UI and client

use crate::{virtual_filesystem::FileSystem, Cmd, PlatformSpawner, Spawner};
use crossbeam::channel::{Receiver, Sender};
use database::{schema::ConnectedClient, WS_MASTER_URL, WS_MASTER_URL_LOCAL};
use eframe::egui::Color32;
use ewebsock::{WsMessage, WsReceiver, WsSender};
use serde::{Deserialize, Serialize};
use web_time::Instant;

/// Connection state for a client
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ConnectionState {
    #[default]
    Disconnected,
    Connecting,
    Connected,
    Stale, // No pong received recently
}

impl ConnectionState {
    /// Get color for connection state indicator
    pub fn color(&self) -> Color32 {
        match self {
            ConnectionState::Connected => Color32::from_rgb(50, 205, 50), // Lime green
            ConnectionState::Connecting => Color32::from_rgb(255, 165, 0), // Orange
            ConnectionState::Stale => Color32::YELLOW,
            ConnectionState::Disconnected => Color32::from_rgb(220, 20, 60), // Crimson
        }
    }

    /// Get status text
    pub fn as_str(&self) -> &str {
        match self {
            ConnectionState::Connected => "Connected",
            ConnectionState::Connecting => "Connecting...",
            ConnectionState::Stale => "Stale",
            ConnectionState::Disconnected => "Disconnected",
        }
    }
}

/// Manages a WebSocket connection to a single Mastertech client
pub struct ConnectionManager {
    /// The client this manager is for
    pub client: ConnectedClient,
    /// Current connection state
    pub state: ConnectionState,
    /// WebSocket sender (if connected)
    ws_sender: Option<WsSender>,
    /// WebSocket receiver (if connected)
    ws_receiver: Option<WsReceiver>,
    /// Time of last received pong
    pub last_pong_time: Option<Instant>,
    /// Time connection was established
    pub connected_at: Option<Instant>,
    /// Ping timeout in seconds
    ping_timeout_secs: u64,
    /// Channel for sending commands to the client
    pub send_cmd_tx: Sender<Cmd>,
    send_cmd_rx: Receiver<Cmd>,
    /// Channel for receiving commands from the client
    pub receive_cmd_tx: Sender<Cmd>,
    pub receive_cmd_rx: Receiver<Cmd>,
    /// Message buffer for text messages
    pub message_buffer: Vec<String>,
    /// Binary message buffer
    pub binary_buffer: Vec<Vec<u8>>,
    /// Shared filesystem for file operations
    pub filesystem: FileSystem,
    /// Error message if connection failed
    pub error: Option<String>,
    /// Last ping sent time
    last_ping_time: Option<Instant>,
}

impl ConnectionManager {
    pub fn new(client: ConnectedClient, filesystem: FileSystem, ping_timeout_secs: u64) -> Self {
        let (send_cmd_tx, send_cmd_rx) = crossbeam::channel::unbounded();
        let (receive_cmd_tx, receive_cmd_rx) = crossbeam::channel::unbounded();

        Self {
            client,
            state: ConnectionState::Disconnected,
            ws_sender: None,
            ws_receiver: None,
            last_pong_time: None,
            connected_at: None,
            ping_timeout_secs,
            send_cmd_tx,
            send_cmd_rx,
            receive_cmd_tx,
            receive_cmd_rx,
            message_buffer: Vec::new(),
            binary_buffer: Vec::new(),
            filesystem,
            error: None,
            last_ping_time: None,
        }
    }

    /// Initiate WebSocket connection
    pub fn connect(&mut self) {
        if matches!(self.state, ConnectionState::Connected | ConnectionState::Connecting) {
            return;
        }

        self.state = ConnectionState::Connecting;
        self.error = None;

        let url = format!(
            "{}&room_id={}",
            if cfg!(debug_assertions) {
                WS_MASTER_URL_LOCAL
            } else {
                WS_MASTER_URL
            },
            self.client.connection_string
        );

        log::info!("ConnectionManager: Connecting to {}", url);

        match ewebsock::connect(&url, Default::default()) {
            Ok((ws_sender, ws_receiver)) => {
                self.ws_sender = Some(ws_sender);
                self.ws_receiver = Some(ws_receiver);
                self.state = ConnectionState::Connected;
                self.connected_at = Some(Instant::now());
                self.last_pong_time = Some(Instant::now());
                log::info!(
                    "ConnectionManager: Connected to {}",
                    self.client.connection_string
                );
            }
            Err(e) => {
                self.state = ConnectionState::Disconnected;
                self.error = Some(format!("Connection failed: {}", e));
                log::error!(
                    "ConnectionManager: Failed to connect to {}: {}",
                    self.client.connection_string,
                    e
                );
            }
        }
    }

    /// Disconnect from the client
    pub fn disconnect(&mut self) {
        if let Some(mut sender) = self.ws_sender.take() {
            sender.close();
        }
        self.ws_receiver = None;
        self.state = ConnectionState::Disconnected;
        self.connected_at = None;
        self.last_pong_time = None;
        self.message_buffer.clear();
        self.binary_buffer.clear();
        log::info!(
            "ConnectionManager: Disconnected from {}",
            self.client.connection_string
        );
    }

    /// Check if connection has timed out (no pong received within timeout)
    pub fn is_timed_out(&self) -> bool {
        if !matches!(self.state, ConnectionState::Connected | ConnectionState::Stale) {
            return false;
        }

        self.last_pong_time
            .map(|t| t.elapsed().as_secs() > self.ping_timeout_secs * 2)
            .unwrap_or(true)
    }

    /// Check if connection is stale (no pong in timeout period but not timed out)
    pub fn is_stale(&self) -> bool {
        if !matches!(self.state, ConnectionState::Connected | ConnectionState::Stale) {
            return false;
        }

        self.last_pong_time
            .map(|t| {
                let elapsed = t.elapsed().as_secs();
                elapsed > self.ping_timeout_secs && elapsed <= self.ping_timeout_secs * 2
            })
            .unwrap_or(false)
    }

    /// Update connection state, process messages, send pings
    pub fn update(&mut self) {
        if !matches!(self.state, ConnectionState::Connected | ConnectionState::Stale) {
            return;
        }

        // Update stale state
        if self.is_stale() && self.state == ConnectionState::Connected {
            self.state = ConnectionState::Stale;
            log::warn!(
                "ConnectionManager: Connection to {} is stale",
                self.client.connection_string
            );
        } else if !self.is_stale() && self.state == ConnectionState::Stale {
            self.state = ConnectionState::Connected;
        }

        // Process incoming messages
        self.receive_messages();

        // Send pending commands
        self.send_pending_commands();

        // Send ping if needed (every ping_timeout_secs / 2)
        self.maybe_send_ping();
    }

    /// Receive and process WebSocket messages
    fn receive_messages(&mut self) {
        if let Some(receiver) = &self.ws_receiver {
            while let Some(event) = receiver.try_recv() {
                match event {
                    ewebsock::WsEvent::Message(msg) => self.handle_message(msg),
                    ewebsock::WsEvent::Opened => {
                        log::info!(
                            "ConnectionManager: WebSocket opened for {}",
                            self.client.connection_string
                        );
                        self.state = ConnectionState::Connected;
                        self.last_pong_time = Some(Instant::now());
                    }
                    ewebsock::WsEvent::Closed => {
                        log::info!(
                            "ConnectionManager: WebSocket closed for {}",
                            self.client.connection_string
                        );
                        self.state = ConnectionState::Disconnected;
                    }
                    ewebsock::WsEvent::Error(e) => {
                        log::error!(
                            "ConnectionManager: WebSocket error for {}: {}",
                            self.client.connection_string,
                            e
                        );
                        self.error = Some(e);
                        self.state = ConnectionState::Disconnected;
                    }
                }
            }
        }
    }

    /// Handle a single WebSocket message
    fn handle_message(&mut self, msg: WsMessage) {
        match msg {
            WsMessage::Text(text) => {
                self.message_buffer.push(text.to_string());
                // Try to deserialize as command
                if let Ok(cmd) = serde_json::from_str::<Cmd>(&text) {
                    let _ = self.receive_cmd_tx.send(cmd);
                }
            }
            WsMessage::Binary(data) => {
                // Try to deserialize as command using bincode
                if let Some(cmd) = deserialize_command(&data) {
                    let _ = self.receive_cmd_tx.send(cmd);
                } else {
                    self.binary_buffer.push(data.to_vec());
                }
            }
            WsMessage::Ping(_) => {
                // Respond with pong
                if let Some(sender) = &self.ws_sender {
                    sender.send(WsMessage::Pong(vec![].into()));
                }
            }
            WsMessage::Pong(_) => {
                self.last_pong_time = Some(Instant::now());
                if self.state == ConnectionState::Stale {
                    self.state = ConnectionState::Connected;
                }
            }
            WsMessage::Unknown(_) => {}
        }
    }

    /// Send pending commands to the client
    fn send_pending_commands(&mut self) {
        if let Some(sender) = &self.ws_sender {
            while let Ok(cmd) = self.send_cmd_rx.try_recv() {
                let data = serialize_command(&cmd);
                sender.send(WsMessage::Binary(data.into()));
            }
        }
    }

    /// Send ping if enough time has elapsed
    fn maybe_send_ping(&mut self) {
        let should_ping = self
            .last_ping_time
            .map(|t| t.elapsed().as_secs() >= self.ping_timeout_secs / 2)
            .unwrap_or(true);

        if should_ping {
            if let Some(sender) = &self.ws_sender {
                sender.send(WsMessage::Ping(vec![].into()));
                self.last_ping_time = Some(Instant::now());
            }
        }
    }

    /// Send a text message to the client
    pub fn send_text(&self, text: &str) {
        if let Some(sender) = &self.ws_sender {
            sender.send(WsMessage::Text(text.into()));
        }
    }

    /// Send a command to the client
    pub fn send_command(&self, cmd: Cmd) {
        let _ = self.send_cmd_tx.send(cmd);
    }

    /// Get connection uptime
    pub fn uptime(&self) -> Option<std::time::Duration> {
        self.connected_at.map(|t| t.elapsed())
    }

    /// Get time since last pong
    pub fn time_since_pong(&self) -> Option<std::time::Duration> {
        self.last_pong_time.map(|t| t.elapsed())
    }
}

/// Deserialize a command from binary data
fn deserialize_command(bytes: &[u8]) -> Option<Cmd> {
    use bincode::{config::standard, serde::decode_from_slice};
    decode_from_slice(bytes, standard())
        .ok()
        .map(|(cmd, _)| cmd)
}

/// Serialize a command to binary data
fn serialize_command(cmd: &Cmd) -> Vec<u8> {
    use bincode::{config::standard, serde::encode_to_vec};
    encode_to_vec(cmd, standard()).expect("Failed to serialize command")
}

