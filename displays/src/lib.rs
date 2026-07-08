use database::{schema::{LiveTaskPayload, Node, Priority, Status, SystemInformation, TaskNotePayload, User}, CURRENT_USER_INFO, STORE_USERS};
use eframe::egui::{Modifiers, Response, Ui};
use crossbeam::channel::{Receiver, Sender};
use bincode::{config::standard, serde::*};
use modals::task_modal::ModalAction;
use serde::{Deserialize, Serialize};
use async_trait::async_trait;
use once_cell::sync::Lazy;
use database::schema::RecordId;
use egui_extras::Strip;
use std::fmt::Debug;

pub mod virtual_filesystem;
pub mod channel_manager;
pub mod markdown_editor;
pub mod file_viewer;
pub mod viewports;
pub mod app_state;
pub mod open_service_suggestions;
pub mod mcp_tool_log;
pub mod ui_tools;
pub mod ui_data;
pub mod scripts;
pub mod modals;
pub mod pages;
pub mod chats;
pub mod tabs;
pub mod ai;

pub use platform::PlatformSpawner;

#[cfg(not(target_arch = "wasm32"))]
pub mod mcp;

pub mod plugins;

pub mod remote_desktop;

// Uses ratatui (a non-wasm dependency); the zstd buffer codecs inside are
// further gated to the tokio live-viewer that calls them.
#[cfg(not(target_arch = "wasm32"))]
pub mod remote_viewer;


use crate::modals::create_task_modal::Tur;

pub use {
    // rayon::prelude::{self as rayon},
    async_openai::{self as openai}
};


pub const STYLE: &str = r#"{"warn_if_rect_changes_id": false, "override_text_style":null,"override_font_id":null,"override_text_valign":"Center","text_styles":{"Small":{"size":10.0,"family":"Monospace"},"Body":{"size":14.0,"family":"Monospace"},"Monospace":{"size":12.0,"family":"Monospace"},"Button":{"size":14.0,"family":"Monospace"},"Heading":{"size":18.0,"family":"Monospace"}},"drag_value_text_style":"Button","wrap":null,"wrap_mode":null,"spacing":{"item_spacing":{"x":3.0,"y":3.0},"window_margin":{"left":12,"right":12,"top":12,"bottom":12},"button_padding":{"x":5.0,"y":3.0},"menu_margin":{"left":12,"right":12,"top":12,"bottom":12},"indent":18.0,"interact_size":{"x":40.0,"y":20.0},"slider_width":100.0,"slider_rail_height":8.0,"combo_width":100.0,"text_edit_width":280.0,"icon_width":14.0,"icon_width_inner":8.0,"icon_spacing":6.0,"default_area_size":{"x":600.0,"y":400.0},"tooltip_width":600.0,"menu_width":400.0,"menu_spacing":2.0,"indent_ends_with_horizontal_line":false,"combo_height":200.0,"scroll":{"floating":true,"bar_width":6.0,"handle_min_length":12.0,"bar_inner_margin":4.0,"bar_outer_margin":0.0,"floating_width":2.0,"floating_allocated_width":0.0,"foreground_color":true,"dormant_background_opacity":0.0,"active_background_opacity":0.4,"interact_background_opacity":0.7,"dormant_handle_opacity":0.0,"active_handle_opacity":0.6,"interact_handle_opacity":1.0}},"interaction":{"interact_radius":5.0,"resize_grab_radius_side":5.0,"resize_grab_radius_corner":10.0,"show_tooltips_only_when_still":true,"tooltip_delay":0.5,"tooltip_grace_time":0.2,"selectable_labels":true,"multi_widget_text_select":true},"visuals":{"dark_mode":true,"text_alpha_from_coverage":"TwoCoverageMinusCoverageSq","override_text_color":[207,216,220,255],"weak_text_alpha":0.6,"weak_text_color":null,"widgets":{"noninteractive":{"bg_fill":[0,0,0,0],"weak_bg_fill":[61,61,61,232],"bg_stroke":{"width":1.0,"color":[71,71,71,247]},"corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"fg_stroke":{"width":1.0,"color":[207,216,220,255]},"expansion":0.0},"inactive":{"bg_fill":[58,51,106,0],"weak_bg_fill":[8,8,8,231],"bg_stroke":{"width":1.5,"color":[48,51,73,255]},"corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"fg_stroke":{"width":1.0,"color":[207,216,220,255]},"expansion":0.0},"hovered":{"bg_fill":[37,29,61,97],"weak_bg_fill":[95,62,97,69],"bg_stroke":{"width":1.7,"color":[106,101,155,255]},"corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"fg_stroke":{"width":1.5,"color":[83,87,88,35]},"expansion":2.0},"active":{"bg_fill":[12,12,15,255],"weak_bg_fill":[39,37,54,214],"bg_stroke":{"width":1.0,"color":[12,12,16,255]},"corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"fg_stroke":{"width":2.0,"color":[207,216,220,255]},"expansion":1.0},"open":{"bg_fill":[20,22,28,255],"weak_bg_fill":[17,18,22,255],"bg_stroke":{"width":1.8,"color":[42,44,93,165]},"corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"fg_stroke":{"width":1.0,"color":[109,109,109,255]},"expansion":0.0}},"selection":{"bg_fill":[23,64,53,27],"stroke":{"width":1.0,"color":[12,12,15,255]}},"hyperlink_color":[135,85,129,255],"faint_bg_color":[17,18,22,255],"extreme_bg_color":[9,12,15,83],"text_edit_bg_color":null,"code_bg_color":[30,31,35,255],"warn_fg_color":[61,185,157,255],"error_fg_color":[255,55,102,255],"window_corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"window_shadow":{"offset":[0,0],"blur":7,"spread":5,"color":[17,17,41,118]},"window_fill":[11,11,15,255],"window_stroke":{"width":1.0,"color":[77,94,120,138]},"window_highlight_topmost":true,"menu_corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"panel_fill":[12,12,15,255],"popup_shadow":{"offset":[0,0],"blur":8,"spread":3,"color":[19,18,18,96]},"resize_corner_size":18.0,"text_cursor":{"stroke":{"width":2.0,"color":[197,192,255,255]},"preview":true,"blink":true,"on_duration":0.5,"off_duration":0.5},"clip_rect_margin":3.0,"button_frame":true,"collapsing_header_frame":true,"indent_has_left_vline":true,"striped":true,"slider_trailing_fill":true,"handle_shape":{"Rect":{"aspect_ratio":0.5}},"interact_cursor":"Crosshair","image_loading_spinners":true,"numeric_color_space":"GammaByte","disabled_alpha":0.5},"animation_time":0.083333336,"debug":{"debug_on_hover":false,"warn_if_rect_changes_id":false, "show_focused_widget": false, "debug_on_hover_with_all_modifiers":false,"hover_shows_next":false,"show_expand_width":false,"show_expand_height":false,"show_resize":false,"show_interactive_widgets":false,"show_widget_hits":false,"show_unaligned":true},"explanation_tooltips":false,"url_in_tooltip":false,"always_scroll_the_only_direction":true,"scroll_animation":{"points_per_second":1000.0,"duration":{"min":0.1,"max":0.3}},"compact_menu_style":true}"#;

/// Toast message type for displaying notifications from async contexts
#[derive(Debug, Clone)]
pub enum ToastMessage {
    Success(String),
    Error(String),
    Warning(String),
    Info(String),
}

// Define a global event sender (wrapped in `Arc<Mutex<T>>` for safe access)
static GLOBAL_USERS_CHANNEL: Lazy<(Sender<Vec<User>>, Receiver<Vec<User>>)> = Lazy::new(|| crossbeam::channel::unbounded());

/// Global channel for toast messages from async contexts
static GLOBAL_TOAST_CHANNEL: Lazy<(Sender<ToastMessage>, Receiver<ToastMessage>)> = Lazy::new(|| crossbeam::channel::unbounded());

/// Global channel for the slice-2 "security inventory arrived from a
/// connected client" event. The producer is the per-session
/// `WebSocketClient::receive` handler matching
/// `Cmd::SecurityInventoryResponse`; the consumer is
/// `AdminConsole::receive` which (a) caches the inventory by
/// `connection_string` for the expanded client-row body to render
/// without a DB round-trip, and (b) upserts it onto the linked
/// `computer` row's `current_antivirus` field so it survives session
/// teardown.
///
/// Going through a global Lazy channel keeps this slice's surface
/// area tight: no `WebSocketClient::new` signature change, no extra
/// field plumbed through `client_action::handle_action`, just the
/// same shape used by `GLOBAL_TOAST_CHANNEL`.
static GLOBAL_SECURITY_INVENTORY_CHANNEL: Lazy<(
    Sender<SecurityInventoryEvent>,
    Receiver<SecurityInventoryEvent>,
)> = Lazy::new(|| crossbeam::channel::unbounded());

/// Payload pushed on [`GLOBAL_SECURITY_INVENTORY_CHANNEL`] — pairs
/// the source `connection_string` (so the admin knows which row to
/// update) with the freshly-gathered inventory.
#[derive(Debug, Clone)]
pub struct SecurityInventoryEvent {
    pub connection_string: String,
    pub products: Vec<database::schema::InstalledSecurityProduct>,
}

pub fn get_security_inventory_sender() -> Sender<SecurityInventoryEvent> {
    GLOBAL_SECURITY_INVENTORY_CHANNEL.0.clone()
}

pub fn get_security_inventory_receiver() -> Receiver<SecurityInventoryEvent> {
    GLOBAL_SECURITY_INVENTORY_CHANNEL.1.clone()
}

pub fn get_users_channel_sender() -> Sender<Vec<User>> {
    GLOBAL_USERS_CHANNEL.0.clone()
}

pub fn get_users_channel_receiver() -> Receiver<Vec<User>> {
    GLOBAL_USERS_CHANNEL.1.clone()
}

/// Get the sender for toast messages (use from async contexts)
pub fn get_toast_sender() -> Sender<ToastMessage> {
    GLOBAL_TOAST_CHANNEL.0.clone()
}

/// Get the receiver for toast messages (use in UI update loop)
pub fn get_toast_receiver() -> Receiver<ToastMessage> {
    GLOBAL_TOAST_CHANNEL.1.clone()
}

// ── Global graceful-shutdown signal ──────────────────────────────────────────
//
// All long-running tokio loops in the desktop client (the direct-TCP admin
// `accept_loop`, the MCP servers on :9001/:9003/:9004, etc.) should
// `tokio::select!` on [`wait_for_shutdown`] alongside their normal work.
// `MasterTechApp::on_exit` fires [`signal_shutdown`] so those loops can break
// cleanly before the process actually exits — without this, dropping the
// implicit `#[tokio::main]` runtime can hang on Windows IOCP waits and keep
// the launching terminal window alive after the egui window closes.
#[cfg(feature = "tokio")]
mod shutdown_signal {
    use once_cell::sync::Lazy;
    use tokio::sync::watch;

    static SHUTDOWN_TX: Lazy<watch::Sender<bool>> = Lazy::new(|| {
        let (tx, _rx) = watch::channel(false);
        tx
    });

    /// Fire the global shutdown signal. Idempotent; safe to call from any thread.
    pub fn signal_shutdown() {
        let _ = SHUTDOWN_TX.send_replace(true);
    }

    /// Non-async check: has the process started shutting down?
    pub fn is_shutting_down() -> bool {
        *SHUTDOWN_TX.borrow()
    }

    /// Resolves once shutdown has been signaled. If already signaled when
    /// awaited, returns immediately. Pair with `tokio::select!` in any
    /// long-running accept loop so it can break cleanly on app exit.
    pub async fn wait_for_shutdown() {
        let mut rx = SHUTDOWN_TX.subscribe();
        if *rx.borrow() {
            return;
        }
        while !*rx.borrow() {
            if rx.changed().await.is_err() {
                return;
            }
        }
    }
}

#[cfg(feature = "tokio")]
pub use shutdown_signal::{is_shutting_down, signal_shutdown, wait_for_shutdown};


pub trait Spawner {
    #[cfg(not(target_arch = "wasm32"))]
    fn spawn<F>(future: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static;

    #[cfg(target_arch = "wasm32")]
    fn spawn<F>(future: F)
    where
        F: std::future::Future<Output = ()> + 'static;
}

#[cfg(target_arch = "wasm32")]
mod platform {
    use super::Spawner;
    use wasm_bindgen_futures::spawn_local;

    pub struct PlatformSpawner;

    impl Spawner for PlatformSpawner {
        fn spawn<F>(future: F)
        where
            F: std::future::Future<Output = ()> + 'static,
        {
            spawn_local(future);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod platform {
    use super::Spawner;
    use tokio::task;

    pub struct PlatformSpawner;

    impl Spawner for PlatformSpawner {
        fn spawn<F>(future: F)
        where
            F: std::future::Future<Output = ()> 
                + 'static 
                + std::marker::Send,
                
        {
            task::spawn(future);
        }
    }
}


pub fn get_current_user_from_auth() -> Option<User> {
    if let Ok(current_user) = CURRENT_USER_INFO.try_lock() {
        log::trace!("get_current_user_from_auth: user retrieved from global state");
        current_user.clone()
    } else {
        log::warn!("get_current_user_from_auth: failed to acquire lock");
        None
    }
}

pub fn get_database_users() -> Vec<User> {
    if let Ok(users) = STORE_USERS.try_lock() {
        log::trace!("get_database_users: users retrieved from global state");
        users.clone()
    } else {
        log::warn!("get_database_users: failed to acquire lock");
        vec![]
    }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskUiActions {
    OpenTaskModal(LiveTaskPayload),
    CreateTaskModal(Option<Tur>),
    OpenChatModal((RecordId, Vec<TaskNotePayload>, Option<String>)),
    OpenViewport(LiveTaskPayload),
    OpenCreateTaskModalFromOrder(database::schema::prestashop_schema::PrestashopPayload),
    OpenCreateTaskModalFromSystem(crate::tabs::stock::SystemInStoreData),
    /// Open the Admin Console tab and focus on a specific connected client by
    /// `connection_string`. Emitted by the connected-client cards on My Tasks.
    OpenAdminConsole(String),
    /// Open the diagnostics popup for a connected client by
    /// `connection_string`. Shows past `DiagnosticSession`s for this client.
    OpenClientDiagnostics(String),
    /// Force-refresh the client's open-service-order suggestion bundle.
    /// Sends `Cmd::RequestOpenServiceCandidates { refresh: true }` over
    /// the active admin transport, which re-runs the PrestaShop lookup
    /// on the client and replies with a fresh
    /// `OpenServiceCandidatesResponse` that updates
    /// `open_service_suggestions` for the matching `connection_string`.
    /// Requires an active session; the action handler should toast if
    /// none is open.
    RefreshOpenServiceSuggestions(String),
    /// Open the Stage-4 confirmation modal for binding a suggested
    /// service order (by index) to the client's computer.  Carries the
    /// `connection_string` so the modal can read the bundle from
    /// `open_service_suggestions`.
    OpenServiceCandidateModal {
        connection_string: String,
        candidate_index: usize,
    },
    None,
}


pub trait Displayable {
    fn display_cards(
        &mut self, 
        ui: &mut Ui, 
        user: &User, 
        store_users: &Vec<User>, 
        notes: Vec<TaskNotePayload>,
        tx: Sender<TaskUiActions>,
        last_read: Option<chrono::DateTime<chrono::Utc>>,
    );
}


pub trait ColumnLayout {
    fn layout_cols(&mut self, ui: &mut Ui);
    fn columns(&mut self, s: &mut Strip);
    fn headers(&mut self, s: Strip);
    // fn card_layout(&mut self, uir &mut Ui) -> Option<TaskUiActions>;
}

#[async_trait]
pub trait Updatable {
    async fn update_service_number(&self, service_number: String) -> anyhow::Result<(), anyhow::Error>;
    async fn update_completed(&self, completed: bool) -> anyhow::Result<(), anyhow::Error>;
    async fn update_due_date(&self) -> anyhow::Result<(), anyhow::Error>;
    async fn update_assignee(&self, assignee: RecordId) -> anyhow::Result<(), anyhow::Error>;
    async fn update_task_name(&self, name: String) -> anyhow::Result<(), anyhow::Error>;
    async fn update_status(&self, status: Status) -> anyhow::Result<(), anyhow::Error>;
    async fn update_priority(&self, priority: Option<Priority>) -> anyhow::Result<(), anyhow::Error>;
    async fn update_task_description(&self) -> anyhow::Result<(), anyhow::Error>;
}

pub trait Interaction {
    fn interact_service_number(&mut self, ui: &mut Ui) -> Response;
    fn interact_task_name(&mut self, ui: &mut Ui) -> Response; 
    fn interact_task_description(&mut self, ui: &mut Ui) -> Response; 
    fn interact_due_date(&mut self, ui: &mut Ui) -> Response; 
    fn interact_completed(&mut self, ui: &mut Ui) -> Response; 
    fn interact_status(&mut self, user: &User, ui: &mut Ui) -> Response; 
    fn interact_priority(&mut self, ui: &mut Ui) -> Response; 
    fn interact_assignee(&mut self, ui: &mut Ui, store_users: &Vec<User>, current_user: &User) -> Response; 
}


#[async_trait]
pub trait Task {
    async fn get_computer_data<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(
        &mut self,
    ) -> anyhow::Result<Option<T>, anyhow::Error>;
    async fn get_customer_data<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(
        &mut self,
    ) -> anyhow::Result<Option<T>, anyhow::Error>;
    async fn get_task_notes<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(
        &mut self,
    ) -> anyhow::Result<Option<T>, anyhow::Error>;
    async fn get_ticket_payload<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(
        &mut self,
    ) -> anyhow::Result<Option<T>, anyhow::Error>;
}

pub trait DisplayModal {
    fn display(&mut self, ui: &mut Ui, action_handler: &mut dyn FnMut(ModalAction)) -> Option<ModalAction>;
}


#[derive(Serialize, Deserialize, Debug)]
pub enum Cmd {
    LiveData,
    TaskManager,
    FileSystemAction(FileSystemAction),
    // (The legacy `UninstallProgram(String)` tuple variant lived
    // here but was never handled — only a commented-out
    // `todo!()` in `terminal_mode`. The new struct variant
    // `UninstallProgram { id, prefer_silent }` is defined below
    // alongside `ListInstalledPrograms` / `InstalledProgramsResponse` /
    // `UninstallProgramResult` so all of the slice-3 wire shapes
    // are grouped together.)
    PullKeys(String),
    PullTicket(String),
    InteractiveInput(String),
    /// Execute a shell command with specified shell type
    ShellCommand { command: String, shell: ShellCommandType },
    /// Start an interactive shell session with specified shell type
    StartInteractiveShell(ShellCommandType),
    QuitInteractive,
    ReadEvents,
    Quit,
    /// Kill a process by PID
    KillProcess(u32),
    /// Open a process location in file explorer
    OpenProcessInExplorer(String),
    /// Request directory listing from remote machine
    ListDirectory(String),
    /// Response with directory listing (entries, resolved_path)
    DirectoryListing(Vec<RemoteDirEntry>, Option<String>),
    /// Request available drives
    GetDrives,
    /// Response with available drives
    DriveList(Vec<String>),
    /// Request to download a file from remote machine
    DownloadRemoteFile(String),
    /// Zip and download a directory from the remote machine
    DownloadRemoteDirectory(String),
    /// Walk a directory tree and return total size
    ScanDirectorySize(String),
    /// Directory size scan result
    DirectorySizeResult {
        path: String,
        total_bytes: u64,
        file_count: u64,
        dir_count: u64,
        error: Option<String>,
    },
    /// File data chunk response (data, is_last_chunk)
    FileChunk(Vec<u8>, bool),
    /// Execute/open a file on the remote machine
    ExecuteRemoteFile(String),
    /// Request file content for text preview
    PreviewRemoteFile(String),
    /// Response with file content for preview (path, content)
    FilePreviewContent(String, String),
    /// Upload a file to the remote client (destination_path, data)
    UploadToClient(String, Vec<u8>),
    /// Request a thumbnail for an image file
    RequestThumbnail(String),
    /// Response with thumbnail data (path, png_bytes)
    ThumbnailResponse(String, Vec<u8>),
    /// Save edited file content to remote (path, content)
    SaveRemoteFile(String, String),
    /// Response indicating save result
    SaveResult(bool, String),
    /// Reboot the remote system (with optional persistence flag for auto-restart)
    RebootSystem { persist_mastertech: bool, terminal_mode: bool },
    /// Spawn Mastertech in terminal mode on the remote client without rebooting.
    /// Equivalent to the local "Terminal Mode" menu bar action.
    LaunchTerminalMode,
    /// Shutdown the remote system
    ShutdownSystem,
    /// Lock the remote workstation
    LockWorkstation,
    /// Log off the current user
    LogOffUser,

    // --- Event Log ---
    ReadEventLog { log_name: String, max_entries: u32, level_filter: Option<String> },
    EventLogResponse(Vec<EventLogEntry>),

    // --- Windows Services ---
    ListServices,
    ServiceListResponse(Vec<WindowsService>),
    ControlService { name: String, action: ServiceActionType },
    ServiceActionResponse { name: String, success: bool, message: String },

    // --- Task Scheduler ---
    ListScheduledTasks { folder: Option<String> },
    ScheduledTaskListResponse(Vec<ScheduledTask>),
    ToggleScheduledTask { path: String, enable: bool },
    RunScheduledTask(String),
    ScheduledTaskActionResponse { success: bool, message: String },

    // --- Registry ---
    ListRegistryKeys(String),
    RegistryKeyResponse { path: String, subkeys: Vec<RegistryKeyInfo>, values: Vec<RegistryValueEntry> },
    BackupRegistryKey(String),
    RegistryBackupResponse { success: bool, backup_path: String, message: String },
    CommitRegistryEdits(Vec<RegistryEdit>),
    RegistryEditResponse { success: bool, message: String },

    // --- Security inventory (slice 2 of the AV-data refactor) ---
    /// Ask the remote client to enumerate its installed security
    /// products (antivirus / antispyware / EDR) via WMI
    /// `SecurityCenter2`, backstopped by a registry uninstall walk
    /// for missing version/publisher fields. Sent from the admin
    /// console on `ConnectClient`; the response gets upserted onto
    /// the client's linked `computer` row's `current_antivirus`
    /// field.
    GatherSecurityInventory,
    /// Response payload from the remote client carrying the
    /// gathered list. Uses the `InstalledSecurityProduct` struct
    /// (re-exported from `database::schema::computer`) directly so
    /// the admin can `DATABASE.update(...)` it onto the `computer`
    /// row without any field-by-field copying.
    SecurityInventoryResponse(Vec<database::schema::InstalledSecurityProduct>),

    // --- Installed Programs (slice 3 of the connected-client refactor) ---
    /// Enumerate every installed program from the registry's
    /// `Uninstall` subtrees (HKLM, HKLM\WOW6432Node, HKCU). Used
    /// by the admin's Installed Programs viewer.
    ListInstalledPrograms,
    InstalledProgramsResponse(Vec<InstalledProgram>),
    /// Uninstall the program identified by its registry subkey
    /// name. When `prefer_silent` is true the client tries (in
    /// order) the publisher's `QuietUninstallString`, an
    /// MSI-rewrite to `MsiExec.exe /X{guid} /qn`, or a heuristic
    /// silent-switch append (`/S` for NSIS, `/SILENT /NORESTART`
    /// for InnoSetup). If none of those work it falls through to
    /// the raw `UninstallString` — which usually pops the GUI
    /// uninstaller on the remote. The result includes whichever
    /// strategy was tried so the operator can see what
    /// happened.
    UninstallProgram { id: String, prefer_silent: bool },
    UninstallProgramResult {
        id: String,
        success: bool,
        message: String,
    },

    // --- Windows Update (slice 4 of the connected-client refactor) ---
    /// Trigger a Windows Update scan + install on the remote
    /// client. Uses the existing
    /// `utilities::windows::windows_update::install_windows_updates`
    /// COM flow under the hood; this Cmd is just the
    /// admin-trigger entry. When `reboot_when_done` is true the
    /// client follows up with a reboot if any update requested
    /// one (the admin still gets the result message first).
    ///
    /// Used by the slice-4 "Batch ▾" menu to fan out an update
    /// across every connected client in a single click.
    RunWindowsUpdate { reboot_when_done: bool },
    /// Result of a `RunWindowsUpdate`. `summary` captures the
    /// number of updates found / installed; the message is a
    /// human-readable one-liner suitable for a toast.
    WindowsUpdateResult {
        success: bool,
        summary: String,
    },

    // --- Startup Apps ---
    ListStartupApps,
    StartupAppsResponse(Vec<StartupApp>),
    ToggleStartupApp { name: String, registry_path: String, enable: bool },
    StartupAppActionResponse { success: bool, message: String },

    // --- Remote Scripts ---
    GetRemoteScriptList,
    RemoteScriptListResponse { categories: Vec<(String, Vec<RemoteScriptItem>)> },
    RunRemoteScripts {
        scripts: Vec<RemoteScriptItem>,
        service_number: String,
        customer_email: String,
        #[serde(default)]
        diagnostic_session_id: String,
    },
    RemoteScriptLog(String),
    RemoteScriptResult { name: String, status: RemoteScriptStatus },
    RemoteScriptsComplete,

    /// Run a script on the remote client by sending its text content directly.
    /// The filename extension determines the shell: .ps1 → PowerShell, .bat/.cmd → cmd.
    RunScriptContent { filename: String, content: String },

    /// Load a WASM plugin on the receiving Mastertech instance.
    /// Sent from admin console → remote client to hot-deploy a plugin without recompiling.
    LoadWasmPlugin { plugin_id: String, wasm_bytes: Vec<u8> },

    /// Response after a WASM plugin load attempt.
    LoadWasmPluginResult { plugin_id: String, success: bool, message: String },

    /// Enable or disable the EguiFrameCapture plugin on the remote client.
    /// Sent from admin console to start/stop frame streaming.
    SetFrameCapture { enabled: bool },

    /// Push a file chunk from admin console → remote client.
    /// Files are split into ≤512 KB chunks and reassembled on the client.
    DirectFileTransfer {
        filename: String,
        chunk_index: u32,
        total_chunks: u32,
        data: Vec<u8>,
    },

    /// Acknowledgement after all chunks of a direct file transfer have been received and written.
    DirectFileTransferResult {
        filename: String,
        success: bool,
        message: String,
    },

    // ── Remote Mastertech self-update ────────────────────────────────────────
    // Admin pushes the new MasterTech.exe in 512 KiB chunks; when the remote
    // client has all chunks it calls self_replace, relaunches with the same CLI
    // args, and responds with MastertechSelfUpdateResult before exiting.

    /// One chunk of a binary self-update payload.  Admin → remote client only.
    /// `chunk_index` is 0-based; `total_chunks` is the total number expected.
    MastertechSelfUpdateChunk {
        chunk_index: u32,
        total_chunks: u32,
        data: Vec<u8>,
    },

    /// Sent by the remote client before apply/relaunch so the admin can pause
    /// ping deadlines and expect a brief disconnect.
    MastertechSelfUpdateRelaunching {
        reconnect_hint_secs: u32,
    },

    /// Sent by the remote client back to the admin after the update either
    /// completed (binary replaced + relaunch spawned) or failed.
    MastertechSelfUpdateResult {
        success: bool,
        message: String,
    },

    /// Call an MCP tool on a remote client's plugin, routed via the admin WebSocket.
    /// The admin console sends this → remote client handles it → replies with the result.
    CallRemotePluginTool {
        request_id: String,
        plugin_id: String,
        tool_name: String,
        args_json: String,
    },

    /// Response from a remote client after handling a CallRemotePluginTool.
    RemotePluginToolResult {
        request_id: String,
        plugin_id: String,
        tool_name: String,
        success: bool,
        result_json: String,
    },

    /// Sent by a `plugin_builder` worker right after the WebSocket
    /// handshake completes. Lets the admin/MCP side enumerate which
    /// build targets the worker can produce without round-tripping a
    /// no-op compile job. `capabilities` is an open-ended set of
    /// feature tags (e.g. `"sccache"`, `"cargo-1.85"`); `target_triples`
    /// lists the rustc targets the worker has installed (must include
    /// `wasm32-wasip1` to be useful).
    BuildWorkerHello {
        hostname: String,
        target_triples: Vec<String>,
        capabilities: Vec<String>,
    },

    /// Admin → builder: request a `cargo build` of a single-file plugin.
    /// `cargo_toml` and `lib_rs` are full file contents written to a
    /// scratch directory on the worker; `target` is a rustc triple
    /// (typically `wasm32-wasip1`); `profile` is the cargo profile
    /// (typically `release`).
    CompilePluginRequest {
        job_id: String,
        plugin_id: String,
        cargo_toml: String,
        lib_rs: String,
        target: String,
        profile: String,
    },

    /// Builder → admin: optional progress signal while a long compile
    /// is running. The admin side may surface this in a toast; jobs
    /// terminate only on `CompilePluginResult`.
    CompilePluginProgress {
        job_id: String,
        stage: String,
        message: String,
    },

    /// Builder → admin: terminal result for a `CompilePluginRequest`.
    /// On success `wasm_bytes` is `Some` and the bytes are ready for
    /// `ArtifactStore::store` / `plugin_deploy_remote`. On failure the
    /// stderr text is the cargo output the agent needs to fix the
    /// source.
    CompilePluginResult {
        job_id: String,
        success: bool,
        wasm_bytes: Option<Vec<u8>>,
        stdout: String,
        stderr: String,
        duration_ms: u64,
    },

    // --- Application-layer heartbeat ---
    //
    // Separate from the kernel TCP keepalive (60s detection window, set
    // up in `tcp_protocol::apply_tcp_options`) and from the
    // tcp_listener's own `Ping`/`Pong` frame tags.  This heartbeat
    // travels through the *full* Cmd dispatch path on both sides — if
    // the wasm PluginManager or the egui input dispatcher wedges while
    // the kernel socket stays alive, the AppPing round-trip stops and
    // the admin learns the plugin host is hung even though TCP says
    // "fine."  Carries an opaque nonce + send-time so the admin can
    // measure round-trip latency and reject stale pongs.
    AppPing { nonce: u64, sent_at_ms: u64 },
    AppPong { nonce: u64, sent_at_ms: u64 },

    // --- Open-service-order auto-link (Stage 2 of the OA3-driven
    // service-binding feature).  Admin pulls the cached PrestaShop
    // candidates the client built at startup; client replies with the
    // match + open candidate list + live computer specs so the admin's
    // confirmation modal can render a per-field live-vs-presta merge.
    //
    // Pull-based by design: the client never volunteers the data
    // (avoids gratuitous traffic), the admin asks for it when it wants
    // to surface the suggestion modal.  `refresh = true` forces a
    // PrestaShop re-fetch (admin-side "Refresh suggestions" button),
    // otherwise the cached value from first-install is returned.
    RequestOpenServiceCandidates {
        refresh: bool,
    },

    /// Client → admin: the bundle the confirmation modal renders.
    /// `match_` is `None` when the OA3 lookup failed (no PrestaShop
    /// match — admin should fall back to the manual relink popup);
    /// `candidates` is empty when the customer has no open orders.
    OpenServiceCandidatesResponse {
        match_: Option<database::schema::service_match::PrestashopCustomerMatch>,
        candidates: Vec<database::schema::service_match::OpenServiceCandidate>,
        live_specs: Option<database::schema::SystemInformation>,
    },

    None,

    /// Run a custom staged stress scenario on the remote client via stress-runner.
    /// Persists stress_test_run / stress_test_metric / stress_test_event linked to service_order.
    RunRemoteScenario {
        stages: Vec<RemoteScenarioStage>,
        total_wall_secs: Option<u64>,
        #[serde(default)]
        repeat_until_total: bool,
        service_number: Option<String>,
        diagnostic_session_id: Option<String>,
        preset_label: Option<String>,
        notes: Option<String>,
    },

    /// Run multiple stressors concurrently on the remote client via stress-runner.
    /// Persists stress_test_run / stress_test_metric / stress_test_event linked to service_order.
    RunRemoteConcurrent {
        lanes: Vec<RemoteScenarioStage>,
        duration_secs: u64,
        service_number: Option<String>,
        diagnostic_session_id: Option<String>,
        preset_label: Option<String>,
        notes: Option<String>,
    },

    /// Admin → client: begin streaming the desktop of `monitor` at the given
    /// frame rate, JPEG `quality` (1..=100), and resolution `scale` (0..=1).
    DesktopStreamStart { monitor: u32, fps: u32, quality: u8, scale: f32 },
    /// Admin → client: stop desktop streaming.
    DesktopStreamStop,
    /// Admin → client: request the client's monitor list.
    DesktopListMonitors,
    /// Client → admin: the client's available monitors.
    DesktopMonitorList(Vec<crate::remote_desktop::DesktopMonitorInfo>),
}

/// One stage/lane of a remote stress scenario or concurrent run.
/// Wire-mirror of the MCP `ScenarioStageParam`; the client maps `stressor` → `stress_runner::Stressor`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RemoteScenarioStage {
    pub stressor: String,
    pub duration_secs: u64,
    #[serde(default)]
    pub threads: usize,
    pub memory_cap_mb: Option<u64>,
    pub disk_file_mb: Option<u64>,
    pub label: Option<String>,
}

/// RemoteScriptResult name emitted by the client for a `RunRemoteScenario`; matches the admin waiter.
pub const REMOTE_SCENARIO_RESULT_NAME: &str = "stress_scenario_run_remote";

/// RemoteScriptResult name emitted by the client for a `RunRemoteConcurrent`; matches the admin waiter.
pub const REMOTE_CONCURRENT_RESULT_NAME: &str = "stress_concurrent_run_remote";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EventLogEntry {
    pub level: String,
    pub time: String,
    pub source: String,
    pub event_id: u32,
    pub message: String,
}

/// One installed-program entry surfaced by the slice-3 Installed
/// Programs viewer. Built on the client side from a registry walk
/// of HKLM, HKLM\Wow6432Node, and HKCU's `Uninstall` subtrees.
///
/// The `id` field is the registry subkey *name* (e.g.
/// `"{C2C7E2E6-…}"` for MSI products or `"OBS-Studio"` for ad-hoc
/// installers) — that's the only stable identifier we have for
/// the uninstall round-trip, since `DisplayName` can be edited
/// post-install and isn't unique across `WOW6432Node` /
/// `LOCAL_MACHINE` collisions.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct InstalledProgram {
    /// Registry subkey name — the canonical id we pass back as
    /// the argument to `Cmd::UninstallProgram`.
    pub id: String,
    /// `DisplayName` value, or the subkey name as a fallback.
    pub name: String,
    /// `DisplayVersion` if the publisher set one.
    pub version: Option<String>,
    /// `Publisher` value.
    pub publisher: Option<String>,
    /// `InstallDate` formatted as `YYYYMMDD` (Windows convention).
    pub install_date: Option<String>,
    /// `EstimatedSize` in KiB if the publisher reported it.
    pub estimated_size_kb: Option<u64>,
    /// `UninstallString` — the raw command line the publisher
    /// registered for uninstalling the product. May or may not be
    /// silent. Used as the fallback when `quiet_uninstall_string`
    /// is missing and we can't synthesize a silent variant from
    /// it.
    pub uninstall_string: Option<String>,
    /// `QuietUninstallString` — when present, this is the
    /// publisher's pre-built no-UI uninstall command. Preferred
    /// when the admin requested `prefer_silent: true`.
    pub quiet_uninstall_string: Option<String>,
    /// Where the row was found, so the admin UI can show e.g.
    /// `"HKCU"` to distinguish per-user installs from system-wide.
    pub registry_hive: String,
    /// `true` if this row sits under a `WOW6432Node` path (i.e.
    /// is a 32-bit product on a 64-bit host). Lets the viewer
    /// surface that distinction in a column.
    pub is_wow6432: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WindowsService {
    pub name: String,
    pub display_name: String,
    pub status: String,
    pub start_type: String,
    pub pid: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ServiceActionType {
    Start,
    Stop,
    Restart,
    SetStartType(String),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ScheduledTask {
    pub name: String,
    pub path: String,
    pub state: String,
    pub last_run: Option<String>,
    pub next_run: Option<String>,
    pub description: String,
    pub triggers: Vec<String>,
    pub actions: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RegistryKeyInfo {
    pub name: String,
    pub path: String,
    pub subkey_count: u32,
    pub value_count: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RegistryValueEntry {
    pub name: String,
    pub kind: String,
    pub data: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum RegistryEdit {
    SetValue { path: String, name: String, kind: String, data: String },
    DeleteValue { path: String, name: String },
    CreateKey { path: String },
    DeleteKey { path: String },
}

// --- Startup Apps ---

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StartupApp {
    pub name: String,
    pub command: String,
    pub registry_path: String,
    pub state: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RemoteScriptItem {
    pub name: String,
    pub category: String,
    #[serde(default)]
    pub content: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RemoteScriptStatus {
    Running,
    Success,
    Failed,
}

/// A remote directory entry for filesystem browsing
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RemoteDirEntry {
    /// File or folder name
    pub name: String,
    /// Full path on remote machine
    pub path: String,
    /// Whether this is a directory
    pub is_directory: bool,
    /// File size in bytes (None for directories)
    pub size: Option<u64>,
    /// Last modified timestamp (ISO 8601 format)
    pub modified: Option<String>,
}

/// Shell command type for cross-platform shell execution
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellCommandType {
    /// Windows PowerShell
    PowerShell,
    /// Windows CMD
    Cmd,
    /// Unix Bash
    Bash,
    /// Unix sh
    Sh,
    /// Auto-detect based on OS
    Auto,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum FileSystemAction {
    Execute(String),
    CopyToClient(String),
    CopyFromClient(String),
    Delete(String),
    Select((Modifiers, String)),
    PreviewedFile(String),
    EnterDirectory(String),
    ExpandDirectory(String),
    GetNode(Node),
    RequestNewContents(String),
    NavigateHome,
    /// Create a new folder at the given path
    CreateFolder(String),
    /// Create a new file at the given path
    CreateFile(String),
    /// Rename a file/folder: (old_path, new_name)
    Rename(String, String),
    /// Run a script file on a connected remote client (full_path, filename)
    RunOnRemote(String, String),
}

/// Tag byte prepended to binary WebSocket messages carrying an `EguiFrameMessage`.
/// Terminal buffer messages start with zstd magic (0x28), so 0xEF is unambiguous.
pub const EGUI_FRAME_TAG: u8 = 0xEF;

/// Admin → client: serialized `plugins::remote::EguiInputEvent` for remote control.
pub const EGUI_INPUT_TAG: u8 = 0xEE;

/// Client → admin: serialized `remote_desktop::DesktopFrameMessage` (raster desktop capture).
pub const DESKTOP_FRAME_TAG: u8 = 0xED;

/// Admin → client: serialized `remote_desktop::DesktopInputEvent` for full-desktop control.
pub const DESKTOP_INPUT_TAG: u8 = 0xEC;

pub fn serialize_system_info(system_info: &SystemInformation) -> Vec<u8> {
    encode_to_vec(system_info, standard()).expect("Failed to serialize SystemInformation")
}

pub fn deserialize_command(bytes: &[u8]) -> Cmd {
    let (cmd, _) = decode_from_slice(bytes, standard()).expect("Failed to deserialize Cmd");
    cmd
}

use chrono::{DateTime, Datelike, Utc};
use jiff::civil::Date as JiffDate;

/// Extracts a jiff Date from a chrono DateTime<Utc>
pub fn to_jiff_date(dt: &DateTime<Utc>) -> JiffDate {
    JiffDate::new(
        dt.year() as i16,
        dt.month() as i8,
        dt.day() as i8,
    ).expect("Valid chrono date should perfectly map to a jiff date")
}

/// Returns a new DateTime<Utc> with the updated date, preserving the original time
pub fn apply_jiff_date(original: &DateTime<Utc>, new_date: &JiffDate) -> DateTime<Utc> {
    let naive_date = chrono::NaiveDate::from_ymd_opt(
        new_date.year() as i32,
        new_date.month() as u32,
        new_date.day() as u32,
    ).expect("Valid jiff date should perfectly map to a chrono date");

    // Grab the original time (hours, minutes, seconds)
    let original_time = original.time();
    
    // Combine the new date with the original time, and convert back to DateTime<Utc>
    // Note: .and_utc() is the modern, non-deprecated way to do this in chrono
    naive_date.and_time(original_time).and_utc()
}