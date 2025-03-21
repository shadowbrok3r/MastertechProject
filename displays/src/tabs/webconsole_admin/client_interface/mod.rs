use crate::{channel_manager::ChannelManager, remote_viewer::term_viewer::RemoteTerminal, tabs::resource_monitor::ResourceMonitor, virtual_filesystem::{FileSysHelper, FileSystem}, Cmd, FileSystemAction, PlatformSpawner, Spawner};
use database::{schema::{ConnectedClient, Record, SystemInformation, CONNECTED_CLIENT_TABLE}, DATABASE};
use ewebsock::{WsReceiver, WsSender};
use crossbeam::channel::{Receiver, Sender};
use serde::{Deserialize, Serialize};
use surrealdb::Response;
use web_time::Instant;
use std::sync::{Arc, Mutex};
use log::info;

pub mod receive;
pub mod tabs;
pub mod ui;
pub mod terminal_viewer;

pub trait ClientHandler { 
    fn connect(&mut self);
    fn export_logs(&mut self, history: Vec<History>);
    fn delete_client(&mut self);
}

pub enum ClientConnection{
    ClientUrl(String),
    Disconnect(String)
}

pub enum WsDisplayState {
    LiveStats,
    Explorer,
    Shell,
    ToolBox,
    Terminal
}

#[derive(Clone)]
struct WebSocketHelperDelegate {
    tx: Sender<Cmd>
}

impl WebSocketHelperDelegate {
    fn new(tx: Sender<Cmd>) -> Self {
        Self { tx }
    }
}

impl FileSysHelper for WebSocketHelperDelegate {
    fn handle_filesystem_action(&mut self, action: &FileSystemAction) {
        log::warn!("FileSysHelper for WebSocketHelperDelegate -> Action -> {action:?}");
        let _ = self.tx.try_send(Cmd::FileSystemAction(action.clone()));
    }
}


pub struct WebSocketClient {
    pub client: ConnectedClient,

    pub ws_sender: Arc<Mutex<WsSender>>,
    pub ws_receiver: Arc<Mutex<WsReceiver>>,
    /// Commands that we are SENDING to Mastertech
    send_cmd_tx: Sender<Cmd>, 
    /// Commands that we are SENDING to Mastertech
    send_cmd_rx: Receiver<Cmd>,
    /// Commands that we are RECEIVING from Mastertech
    receive_cmd_tx: Sender<Cmd>,
    /// Commands that we are RECEIVING from Mastertech
    receive_cmd_rx: Receiver<Cmd>,
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

#[derive(Default, Clone, Serialize, Deserialize, Debug)]
pub struct History {
    from: String,
    message: String,
    timestamp: String
}


impl WebSocketClient {
    pub fn new(ws_sender: WsSender, ws_receiver: WsReceiver, client: ConnectedClient, toolbox: FileSystem) -> Self {
        let display_state_channel = <WsDisplayState>::create_unbounded_channel();
        let (send_cmd_tx, send_cmd_rx) = crossbeam::channel::unbounded();
        let (receive_cmd_tx, receive_cmd_rx) = crossbeam::channel::unbounded();
        let helper_delegate = WebSocketHelperDelegate::new(send_cmd_tx.clone());
        let mut explorer = FileSystem::new();
        explorer.helper_delegate = Some(Box::new(helper_delegate.clone()));
        // let bin_helper_delegate = BinHelperDelegate{};

        let ws_sender = Arc::new(Mutex::new(ws_sender));
        let ws_receiver = Arc::new(Mutex::new(ws_receiver));

        Self {
            remote_terminal: RemoteTerminal::new(ws_sender.clone(), ws_receiver.clone()),
            client,
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
            interactive: false,
            helper_delegate,
            input: Default::default(),
            messages: Default::default(),
            history: Default::default(),
            loading: Default::default(),
            history_idx: Default::default(),
            buffer: Default::default(),
            my_history: Default::default(),
            notifications: Default::default(),
            resource_monitor: ResourceMonitor::default(),
            // bin_msg_delegate,
        }
    }
}

impl ClientHandler for ConnectedClient {
    fn connect(&mut self) { }

    fn export_logs(&mut self, history: Vec<History>) {
        let id = self.id.clone();
        PlatformSpawner::spawn(async move {
            DATABASE.set("id", id).await.unwrap();
            DATABASE.set("history", Some(history.clone())).await.unwrap();
            let query = "UPDATE $id SET command_history += $history";
            let update_history: Result<Response, surrealdb::Error> = DATABASE
                .query(query)
                .await;

            info!("History Response: {update_history:?}");
            info!("History: {:#?}", history.clone());
        });
     }

    fn delete_client(&mut self) {
        let id = self.id.clone();
        PlatformSpawner::spawn(async move {
            let update_history: Result<Option<Record>, surrealdb::Error> = DATABASE
                .delete((CONNECTED_CLIENT_TABLE, id.key().to_string()))
                .await;

            info!("History: {update_history:#?}");
        });
     }
}

pub fn serialize_system_info(system_info: &SystemInformation) -> Option<Vec<u8>> {
    if let Ok(data) = bincode::serialize(system_info){
        Some(data)
    } else { None }
}

pub fn deserialize_system_info(bytes: &[u8]) -> Option<SystemInformation> {
    if let Ok(data) = bincode::deserialize(bytes){
        Some(data)
    } else { None }
}

pub fn deserializer<T: Serialize + for<'a> Deserialize<'a> + 'static >(bytes: &[u8]) -> Option<T> {
    if let Ok(data) = bincode::deserialize(bytes){
        Some(data)
    } else { None }
}

pub fn deserialize_command(bytes: &[u8]) -> Option<Cmd> {
    if let Ok(cmd) = bincode::deserialize(bytes){
        Some(cmd)
    }else{ None }
}

pub fn serialize_command(bytes: &Cmd) -> Vec<u8> {
    bincode::serialize(bytes).expect("Failed to deserialize Cmd")
}