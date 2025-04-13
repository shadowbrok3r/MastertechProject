use crate::{channel_manager::ChannelManager, tabs::resource_monitor::ResourceMonitor, virtual_filesystem::FileSystem, Cmd, PlatformSpawner, Spawner};
use database::schema::{ConnectedClient, SystemInformation};
use ewebsock::{WsMessage, WsReceiver, WsSender};
use filesystem_helper::WebSocketHelperDelegate;
use crossbeam::channel::{Receiver, Sender};
use tabs::terminal_viewer::RemoteTerminal;
use serde::{Deserialize, Serialize};
use ui::WsDisplayState;
use web_time::Instant;

use bincode::{config::standard, serde::*};

use super::client_interface::tabs::command_shell::History;

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
    remote_terminal: RemoteTerminal
}

impl WebSocketClient {
    pub fn new(ws_sender: WsSender, ws_receiver: WsReceiver, client: ConnectedClient, toolbox: FileSystem) -> Self {
        let display_state_channel = <WsDisplayState>::create_unbounded_channel();
        let (send_cmd_tx, send_cmd_rx) = crossbeam::channel::unbounded();
        let (receive_cmd_tx, receive_cmd_rx) = crossbeam::channel::unbounded();
        let (msg_to_client_tx, msg_to_client_rx) = crossbeam::channel::unbounded::<WsMessage>();
        let (msg_from_client_tx, msg_from_client_rx) = crossbeam::channel::unbounded::<WsMessage>();
        let (size_tx, size_rx) = crossbeam::channel::unbounded();

        let helper_delegate = WebSocketHelperDelegate::new(send_cmd_tx.clone());
        let mut explorer = FileSystem::new();
        explorer.helper_delegate = Some(Box::new(helper_delegate.clone()));

        let remote_terminal = RemoteTerminal::new(msg_to_client_tx, size_tx.clone());
        let current_area = remote_terminal.current_area;
        let tx = remote_terminal.buffer_tx.clone();

        PlatformSpawner::spawn(async move {
            log::info!("Checking for messages from client");
            loop {
                while let Ok(msg) = msg_from_client_rx.try_recv() {
                    log::info!("GOT A BUFFER");
                    if let WsMessage::Binary(buffer_array) = msg {
                        RemoteTerminal::receive_buffer(
                            tx.clone(), 
                            &size_rx, 
                            buffer_array, 
                            current_area
                        );
                    }
                }
            }
        });


        Self {
            remote_terminal,
            client,
            msg_to_client_rx,
            msg_from_client_tx,
            ws_sender,
            ws_receiver,

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
        }
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