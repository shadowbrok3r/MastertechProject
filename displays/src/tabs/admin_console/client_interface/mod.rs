use crate::{channel_manager::ChannelManager, tabs::{admin_console::client_interface::tabs::command_shell::History, resource_monitor::ResourceMonitor}, virtual_filesystem::FileSystem, Cmd};
use database::schema::{ConnectedClient, SystemInformation};
use ewebsock::WsMessage;
use filesystem_helper::WebSocketHelperDelegate;
use crossbeam::channel::{Receiver, Sender};
use bincode::{config::standard, serde::*};
use remote_explorer::RemoteExplorer;
use event_log_viewer::EventLogViewer;
use services_viewer::ServicesViewer;
use task_scheduler_viewer::TaskSchedulerViewer;
use installed_programs_viewer::InstalledProgramsViewer;
use registry_editor::RegistryEditor;
use startup_apps_viewer::StartupAppsViewer;
use remote_scripts_viewer::RemoteScriptsViewer;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use ui::WsDisplayState;
use web_time::Instant;

#[cfg(not(target_arch="wasm32"))]
use {
    tabs::terminal_viewer::RemoteTerminal,
    tabs::egui_viewer::InlineEguiViewer,
    crate::mcp::{CommandCompletion, DiagnosticResponse, McpService},
    crate::{PlatformSpawner, Spawner},
};


pub mod admin_transport;
pub mod receive;
pub mod tabs;
pub mod ui;
pub mod filesystem_helper;
pub mod remote_explorer;
pub mod event_log_viewer;
pub mod services_viewer;
pub mod task_scheduler_viewer;
pub mod installed_programs_viewer;
pub mod registry_editor;
pub mod startup_apps_viewer;
pub mod remote_scripts_viewer;

pub use admin_transport::{AdminTransport, TransportKind};

pub enum ClientConnection{
    ClientUrl(String),
    Disconnect(String)
}

pub struct WebSocketClient {
    #[cfg(not(target_arch="wasm32"))]
    pub mcp_service: McpService,
    #[cfg(not(target_arch="wasm32"))]
    pub diagnostic_tx: Sender<DiagnosticResponse>,
    #[cfg(not(target_arch="wasm32"))]
    pub diagnostic_rx: Receiver<DiagnosticResponse>,
    pub client: ConnectedClient,
    /// Admin↔client message pipe. Wraps either an `ewebsock` WebSocket
    /// (relay path) or a direct TCP socket. Exposes the same minimal
    /// `send(WsMessage)` / `try_recv() -> Option<WsEvent>` / `close()`
    /// surface so the existing receive/handler code is transport-agnostic.
    pub transport: AdminTransport,
    /// Commands that we are SENDING to Mastertech
    pub send_cmd_tx: Sender<Cmd>, 
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
    hovered: HashSet<String>,
    remove_hovered: Option<String>,
    // bin_msg_delegate: BinHelperDelegate,
    /// Accumulates fragments of messages
    buffer: String,     
    my_command_history: Vec<History>,
    notifications: i32,
    pub resource_monitor: ResourceMonitor,
    #[cfg(not(target_arch="wasm32"))]
    remote_terminal: RemoteTerminal,
    #[cfg(not(target_arch="wasm32"))]
    pub egui_viewer: InlineEguiViewer,
    #[cfg(not(target_arch="wasm32"))]
    stop_tx: Option<crossbeam::channel::Sender<()>>,
    #[cfg(not(target_arch="wasm32"))]
    size_rx: Receiver<ratatui::layout::Rect>,
    #[cfg(not(target_arch="wasm32"))]
    stop_rx: Receiver<()>,
    /// Track connection status and last pong time
    pub is_connected: bool,
    pub last_pong_time: Option<Instant>,
    pub connection_status: String,
    /// Track if we're using persistent shell mode
    pub persistent_shell_mode: bool,
    /// AI-powered command completion
    pub ai_completion_enabled: bool,
    #[cfg(not(target_arch="wasm32"))]
    pub command_suggestions: Vec<CommandCompletion>,
    pub show_suggestions: bool,
    pub last_partial_command: String,
    pub selected_suggestion: usize,
    #[cfg(not(target_arch="wasm32"))]
    pub completion_cancel_tx: Option<tokio::sync::oneshot::Sender<()>>,
    #[cfg(not(target_arch="wasm32"))]
    pub last_input_change_time: Option<Instant>,
    #[cfg(not(target_arch="wasm32"))]
    pub pending_completion: Option<String>,
    /// Track if live stats (resource monitor) is actively streaming data
    pub live_stats_active: bool,
    /// Show remote egui capture in a separate OS window (clearer than embedding in admin chrome).
    #[cfg(not(target_arch = "wasm32"))]
    pub egui_remote_popout: bool,
    /// Last inner size we applied to the remote-UI pop-out (avoid resizing every frame).
    #[cfg(not(target_arch = "wasm32"))]
    pub egui_remote_popout_inner_sent: Option<(u32, u32)>,
    /// MCP-injected remote egui input; drained in `WebSocketClient::receive`.
    #[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
    pub remote_egui_mcp_rx: Option<crossbeam::channel::Receiver<Vec<u8>>>,
    /// Remote filesystem explorer (websocket-based)
    pub remote_explorer: RemoteExplorer,
    /// Pending file download - stores the filename being downloaded
    pub pending_download_filename: Option<String>,
    /// Buffer for accumulating file chunks during download
    pub download_buffer: Vec<u8>,
    pub event_log_viewer: EventLogViewer,
    pub services_viewer: ServicesViewer,
    pub task_scheduler_viewer: TaskSchedulerViewer,
    /// Slice 3: Installed Programs view + uninstall round-trip.
    pub installed_programs_viewer: InstalledProgramsViewer,
    pub registry_editor: RegistryEditor,
    pub startup_apps_viewer: StartupAppsViewer,
    pub remote_scripts_viewer: RemoteScriptsViewer,
    /// Whether the remote egui frame capture is actively streaming.
    pub egui_viewer_active: bool,
    /// File transfer progress: (filename, chunks_sent, total_chunks)
    pub file_transfer_progress: Option<(String, u32, u32)>,
    /// Channel receiving chunked Cmds from the background file-read thread
    #[cfg(not(target_arch = "wasm32"))]
    pub file_transfer_rx: Option<Receiver<Cmd>>,
    /// Channel receiving `MastertechSelfUpdateChunk` Cmds for a remote self-update
    #[cfg(not(target_arch = "wasm32"))]
    pub self_update_rx: Option<Receiver<Cmd>>,
}

impl Drop for WebSocketClient {
    fn drop(&mut self) {
        #[cfg(feature = "tokio")]
        {
            if let Some(stop_tx) = &self.stop_tx {
                let _ = stop_tx.send(());
            }
        }
        #[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
        if self.remote_egui_mcp_rx.is_some() {
            crate::plugins::remote_egui_control::hub().unregister(&self.client.connection_string);
        }
    }
}

impl WebSocketClient {
    pub fn new(transport: AdminTransport, client: ConnectedClient, toolbox: FileSystem) -> Self {
        let display_state_channel = <WsDisplayState>::create_unbounded_channel();
        let (send_cmd_tx, send_cmd_rx) = crossbeam::channel::unbounded();
        let (receive_cmd_tx, receive_cmd_rx) = crossbeam::channel::unbounded();
        let (msg_to_client_tx, msg_to_client_rx) = crossbeam::channel::unbounded::<WsMessage>();
        let (msg_from_client_tx, msg_from_client_rx) = crossbeam::channel::unbounded::<WsMessage>();
        #[cfg(not(target_arch="wasm32"))]
        let (diagnostic_tx, diagnostic_rx) = crossbeam::channel::unbounded();
        let helper_delegate = WebSocketHelperDelegate::new(send_cmd_tx.clone());
        let mut explorer = FileSystem::new();
        explorer.helper_delegate = Some(Box::new(helper_delegate.clone()));

        #[cfg(not(target_arch="wasm32"))]
        let (size_tx, size_rx) = crossbeam::channel::unbounded::<ratatui::layout::Rect>();

        #[cfg(not(target_arch="wasm32"))]
        let remote_terminal = RemoteTerminal::new(msg_to_client_tx, size_tx.clone());

        #[cfg(not(target_arch="wasm32"))]
        let (stop_tx, stop_rx) = crossbeam::channel::unbounded::<()>();
        #[cfg(not(target_arch="wasm32"))]
        let mcp_service = McpService::default();
        // Attempt to connect OpenAI bridge to local MCP TCP server in the background
        #[cfg(not(target_arch = "wasm32"))]
        {
            // model can be overridden later; default to lightweight model

            use crate::mcp::run_mcp_server_tcp;
            mcp_service.spawn_openai_connect("127.0.0.1:9002", crate::ai::gpts::MODEL, Some(
                format!("You are a command-line completion assistant. Provide a list of up to 5 command completions for a Powershell shell.
Each completion should be on a new line. Do not add any extra text, explanations, or formatting.
The user wants to append the completion to their existing input, so provide the remaining part of the command.
For example, if the user types 'get' you should return suggestions like: 
Get-CimClass
Get-WmiObject")
            ));
            let run_mcp_server_tcp = run_mcp_server_tcp();
            log::warn!("run_mcp_server_tcp: {run_mcp_server_tcp:?}");
        }

        Self {
            #[cfg(not(target_arch="wasm32"))]
            mcp_service,
            #[cfg(not(target_arch="wasm32"))]
            diagnostic_tx, 
            #[cfg(not(target_arch="wasm32"))]
            diagnostic_rx,
            #[cfg(not(target_arch="wasm32"))]
            remote_terminal,
            #[cfg(not(target_arch="wasm32"))]
            egui_viewer: InlineEguiViewer::new(),
            #[cfg(not(target_arch="wasm32"))]
            stop_tx: if cfg!(not(target_arch="wasm32")) { Some(stop_tx) } else { None },
            client,
            msg_to_client_rx,
            msg_from_client_tx,
            msg_from_client_rx,
            transport,
            #[cfg(not(target_arch="wasm32"))]
            size_rx,
            #[cfg(not(target_arch="wasm32"))]
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
            my_command_history: Default::default(),
            notifications: Default::default(),
            resource_monitor: ResourceMonitor::default(),
            is_connected: false,
            last_pong_time: None,
            connection_status: "Disconnected".to_string(),
            persistent_shell_mode: false,
            ai_completion_enabled: true,
            #[cfg(not(target_arch="wasm32"))]
            command_suggestions: Vec::new(),
            show_suggestions: false,
            last_partial_command: String::new(),
            selected_suggestion: 0,
            hovered: HashSet::new(),
            remove_hovered: None,
            #[cfg(not(target_arch="wasm32"))]
            completion_cancel_tx: None,
            #[cfg(not(target_arch="wasm32"))]
            last_input_change_time: None,
            #[cfg(not(target_arch="wasm32"))]
            pending_completion: None,
            live_stats_active: false,
            #[cfg(not(target_arch = "wasm32"))]
            egui_remote_popout: false,
            #[cfg(not(target_arch = "wasm32"))]
            egui_remote_popout_inner_sent: None,
            #[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
            remote_egui_mcp_rx: None,
            remote_explorer: {
                let mut explorer = RemoteExplorer::new();
                // Set the bucket name from the current user for My Tools
                if let Some(user) = crate::get_current_user_from_auth() {
                    explorer.set_bucket_name(&user.get_user_bucket_name());
                }
                explorer
            },
            pending_download_filename: None,
            download_buffer: Vec::new(),
            event_log_viewer: EventLogViewer::new(),
            services_viewer: ServicesViewer::new(),
            task_scheduler_viewer: TaskSchedulerViewer::new(),
            installed_programs_viewer: InstalledProgramsViewer::new(),
            registry_editor: RegistryEditor::new(),
            startup_apps_viewer: StartupAppsViewer::new(),
            remote_scripts_viewer: RemoteScriptsViewer::new(),
            egui_viewer_active: false,
            file_transfer_progress: None,
            #[cfg(not(target_arch = "wasm32"))]
            file_transfer_rx: None,
            #[cfg(not(target_arch = "wasm32"))]
            self_update_rx: None,
        }
    }


    #[cfg(not(target_arch="wasm32"))]
    pub fn start_receiving_buffers(&mut self) {
        #[cfg(feature = "tokio")]
        if self.remote_egui_mcp_rx.is_none() {
            let rx = crate::plugins::remote_egui_control::hub()
                .register(self.client.connection_string.clone());
            self.remote_egui_mcp_rx = Some(rx);
        }
        let rx = self.msg_from_client_rx.clone();
        let terminal_tx = self.remote_terminal.buffer_tx.clone();
        let egui_frame_tx = self.egui_viewer.frame_tx.clone();
        let conn_for_egui_meta = self.client.connection_string.clone();
        let current_area = self.remote_terminal.current_area;
        let size_rx = self.size_rx.clone();
        let stop_rx = self.stop_rx.clone();
        PlatformSpawner::spawn(async move {
            loop {
                if stop_rx.try_recv().is_ok() {
                    log::info!("Stopping RemoteTerminal receive_buffer task");
                    break;
                }
                while let Ok(msg) = rx.try_recv() {
                    if let WsMessage::Binary(buffer_array) = msg {
                        if buffer_array.first() == Some(&crate::EGUI_FRAME_TAG) {
                            if let Ok((frame, _)) = bincode::serde::decode_from_slice::<
                                crate::plugins::EguiFrameMessage, _,
                            >(
                                &buffer_array[1..],
                                bincode::config::standard(),
                            ) {
                                #[cfg(feature = "tokio")]
                                crate::plugins::remote_egui_control::hub()
                                    .record_last_frame(&conn_for_egui_meta, &frame);
                                let _ = egui_frame_tx.try_send(frame);
                            }
                        } else {
                            RemoteTerminal::receive_buffer(
                                terminal_tx.clone(),
                                &size_rx,
                                buffer_array,
                                current_area,
                            );
                        }
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        });
    }

    /// Pop-out window body: receive WS, paint remote egui, forward pointer/scroll as `EGUI_INPUT_TAG` binary.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn show_egui_remote_viewport_panel(
        &mut self,
        ui: &mut eframe::egui::Ui,
        ctx: &eframe::egui::Context,
    ) {
        use crate::plugins::remote::EguiInputEvent;
        use crate::EGUI_INPUT_TAG;
        self.receive(ctx);
        self.egui_viewer.poll_frames();
        let tag = EGUI_INPUT_TAG;
        let Self {
            egui_viewer,
            transport,
            client,
            ..
        } = self;
        let mcp_sess = client.connection_string.as_str();
        egui_viewer.ui(
            ui,
            |input_ev: EguiInputEvent| {
            let loud = matches!(
                &input_ev,
                EguiInputEvent::PointerButton { .. }
                    | EguiInputEvent::PointerLeave
                    | EguiInputEvent::Scroll { .. }
                    | EguiInputEvent::Key { .. }
                    | EguiInputEvent::Text(_)
            );
            match bincode::serde::encode_to_vec(&input_ev, bincode::config::standard()) {
                Ok(ser) => {
                    let mut v = vec![tag];
                    v.extend(ser);
                    if loud {
                        log::error!(
                            target: "egui_remote",
                            "[admin_ws_popout] send {:?} ({} bytes)",
                            input_ev,
                            v.len()
                        );
                    } else {
                        log::debug!(
                            target: "egui_remote",
                            "[admin_ws_popout] send PointerMoved ({} bytes)",
                            v.len()
                        );
                    }
                    transport.send(WsMessage::Binary(v));
                }
                Err(e) => {
                    log::error!(
                        target: "egui_remote",
                        "[admin_ws_popout] bincode encode failed for {input_ev:?}: {e}"
                    );
                }
            }
            },
            Some(mcp_sess),
        );
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