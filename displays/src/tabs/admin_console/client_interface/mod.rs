use crate::{channel_manager::ChannelManager, tabs::resource_monitor::ResourceMonitor, virtual_filesystem::FileSystem, Cmd, PlatformSpawner, Spawner};
use database::schema::{ConnectedClient, SystemInformation};
use ewebsock::{WsMessage, WsReceiver, WsSender};
use filesystem_helper::WebSocketHelperDelegate;
use crossbeam::channel::{Receiver, Sender};
use bincode::{config::standard, serde::*};
use serde::{Deserialize, Serialize};
use ui::WsDisplayState;
use web_time::Instant;

#[cfg(feature="tokio")]
use tabs::terminal_viewer::RemoteTerminal;

use super::client_interface::tabs::command_shell::{History, CommandSuggestion};

pub mod receive;
pub mod tabs;
pub mod ui;
pub mod filesystem_helper;

pub enum ClientConnection{
    ClientUrl(String),
    Disconnect(String)
}

pub struct WebSocketClient {
    pub client: ConnectedClient,
    pub ws_sender: WsSender,
    pub ws_receiver: WsReceiver,
    /// Commands that we are SENDING to Mastertech
    send_cmd_tx: Sender<Cmd>, 
    /// Commands that we are SENDING to Mastertech
    send_cmd_rx: Receiver<Cmd>,
    /// Commands that we are RECEIVING from Mastertech
    receive_cmd_tx: Sender<Cmd>,
    /// Commands that we are RECEIVING from Mastertech
    receive_cmd_rx: Receiver<Cmd>,
    msg_to_client_rx: Receiver<WsMessage>,
    msg_from_client_tx: Sender<WsMessage>,
    msg_from_client_rx: Receiver<WsMessage>,
    /// Sending / Receiving of UI state
    display_state_channel: (Sender<WsDisplayState>, Receiver<WsDisplayState>),

    pub input: String,
    pub messages: Vec<String>,
    pub history: Vec<History>,
    pub loading: bool,
    pub timeout_counter: Instant,
    pub toolbox: FileSystem,
    pub state: WsDisplayState,
    pub explorer: FileSystem,
    pub interactive: bool,
    pub history_idx: usize,
    helper_delegate: WebSocketHelperDelegate,
    // bin_msg_delegate: BinHelperDelegate,
    /// Accumulates fragments of messages
    buffer: String,     
    my_history: Vec<History>,
    notifications: i32,
    resource_monitor: ResourceMonitor,
    #[cfg(feature="tokio")]
    remote_terminal: RemoteTerminal,
    #[cfg(feature="tokio")]
    stop_tx: Option<crossbeam::channel::Sender<()>>,
    size_rx: Receiver<ratatui::layout::Rect>,
    stop_rx: Receiver<()>,
    /// Track connection status and last pong time
    pub is_connected: bool,
    pub last_pong_time: Option<Instant>,
    pub connection_status: String,
    /// Track if we're using persistent shell mode
    pub persistent_shell_mode: bool,
    /// AI-powered command completion
    pub ai_completion_enabled: bool,
    pub command_suggestions: Vec<CommandSuggestion>,
    pub show_suggestions: bool,
    pub last_partial_command: String,
    pub selected_suggestion: usize,
}

impl Drop for WebSocketClient {
    fn drop(&mut self) {
        #[cfg(feature = "tokio")]
        {
            if let Some(stop_tx) = &self.stop_tx {
                let _ = stop_tx.send(());
            }
        }
    }
}

impl WebSocketClient {
    pub fn new(ws_sender: WsSender, ws_receiver: WsReceiver, client: ConnectedClient, toolbox: FileSystem) -> Self {
        let display_state_channel = <WsDisplayState>::create_unbounded_channel();
        let (send_cmd_tx, send_cmd_rx) = crossbeam::channel::unbounded();
        let (receive_cmd_tx, receive_cmd_rx) = crossbeam::channel::unbounded();
        let (msg_to_client_tx, msg_to_client_rx) = crossbeam::channel::unbounded::<WsMessage>();
        let (msg_from_client_tx, msg_from_client_rx) = crossbeam::channel::unbounded::<WsMessage>();

        let helper_delegate = WebSocketHelperDelegate::new(send_cmd_tx.clone());
        let mut explorer = FileSystem::new();
        explorer.helper_delegate = Some(Box::new(helper_delegate.clone()));

        #[cfg(feature="tokio")]
        let (size_tx, size_rx) = crossbeam::channel::unbounded::<ratatui::layout::Rect>();

        #[cfg(feature="tokio")]
        let remote_terminal = RemoteTerminal::new(msg_to_client_tx, size_tx.clone());

        #[cfg(feature="tokio")]
        let (stop_tx, stop_rx) = crossbeam::channel::unbounded::<()>();

        Self {
            #[cfg(feature="tokio")]
            remote_terminal,
            #[cfg(feature="tokio")]
            stop_tx: if cfg!(feature = "tokio") { Some(stop_tx) } else { None },
            client,
            msg_to_client_rx,
            msg_from_client_tx,
            msg_from_client_rx,
            ws_sender,
            ws_receiver,
            size_rx,
            stop_rx,

            send_cmd_tx, 
            send_cmd_rx,
            receive_cmd_tx, 
            receive_cmd_rx,

            display_state_channel,
            timeout_counter: Instant::now(),
            toolbox,
            state: WsDisplayState::Shell,
            explorer,
            helper_delegate,
            interactive: false,
            input: Default::default(),
            messages: Default::default(),
            history: Default::default(),
            loading: Default::default(),
            history_idx: Default::default(),
            buffer: Default::default(),
            my_history: Default::default(),
            notifications: Default::default(),
            resource_monitor: ResourceMonitor::default(),
            is_connected: false,
            last_pong_time: None,
            connection_status: "Disconnected".to_string(),
            persistent_shell_mode: false,
            ai_completion_enabled: true,
            command_suggestions: Vec::new(),
            show_suggestions: false,
            last_partial_command: String::new(),
            selected_suggestion: 0,
        }
    }


    #[cfg(feature="tokio")]
    pub fn start_receiving_buffers(&mut self) {
        let rx = self.msg_from_client_rx.clone();
        let tx = self.remote_terminal.buffer_tx.clone();
        let current_area = self.remote_terminal.current_area;
        let size_rx = self.size_rx.clone();
        let stop_rx = self.stop_rx.clone();
        PlatformSpawner::spawn(async move {
            loop {
                // Check for stop signal
                if stop_rx.try_recv().is_ok() {
                    log::info!("Stopping RemoteTerminal receive_buffer task");
                    break;
                }
                while let Ok(msg) = rx.try_recv() {
                    if let WsMessage::Binary(buffer_array) = msg {
                        RemoteTerminal::receive_buffer(
                            tx.clone(), 
                            &size_rx, 
                            buffer_array, 
                            current_area
                        );
                    }
                }
                // Add a small sleep to avoid busy-waiting
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        });
    }
}

pub fn serialize_system_info(system_info: &SystemInformation) -> Option<Vec<u8>> {
    if let Ok(data) = encode_to_vec(system_info, standard()) {
        Some(data)
    } else { None }
}

pub fn deserialize_system_info(bytes: &[u8]) -> Option<SystemInformation> {
    if let Ok((data, _)) = decode_from_slice(bytes, standard()){
        Some(data)
    } else { None }
}

pub fn deserializer<T: Serialize + for<'a> Deserialize<'a> + 'static >(bytes: &[u8]) -> Option<T> {
    if let Ok((data, _)) = decode_from_slice(bytes, standard()){
        Some(data)
    } else { None }
}

pub fn deserialize_command(bytes: &[u8]) -> Option<Cmd> {
    if let Ok((cmd, _)) = decode_from_slice(bytes, standard()){
        Some(cmd)
    }else{ None }
}

pub fn serialize_command(bytes: &Cmd) -> Vec<u8> {
    encode_to_vec(bytes, standard()).expect("Failed to deserialize Cmd")
}