use database::{schema::{prestashop_schema::PrestashopPayload, CarboniteResponse, ComputerData, CustomerData, DuplicateCheckResult, GetKeysResponse, LiveTaskPayload, TaskNotePayload, TicketData, COMPUTER_TABLE, CONNECTED_CLIENT_TABLE}};
use crate::{tabs::{file_browser::FileBrowser, github::self_updater::GithubRelease, scripts::EguiScriptsTab, tur_sheet::{get_ticket::SendRequest,scaffold::{self, HardwareTest}}}};
use displays::{app_state::{default_tree, SharedContext}, channel_manager::ChannelManager, modals::{DuplicateMergeModal, task_modal::SpecialPartOrder}, plugins::{DefaultEventDispatcher, PluginClientCommand, PluginManager}, ui_tools::toasts::Toasts, virtual_filesystem::FileSystem};
use std::{collections::HashSet,path::PathBuf,sync::{atomic::AtomicBool, Arc, Mutex, RwLock}};
use egui_dock::{DockState, NodeIndex, SurfaceIndex};
use crossbeam::channel::{Receiver, Sender};
use database::schema::RecordId;
use chrono::{DateTime, Utc};
use egui_file::FileDialog;
use eframe::egui::Align2;
use serde_json::Value;

#[cfg(target_os = "windows")]
use crate::tabs::minidump::MiniDumpApp;

pub struct MasterTechApp {
    pub context: MastertechContext,
    pub tree: DockState<String>,
}

pub struct MastertechContext {
    pub shared_ctx: SharedContext,
    pub order_rows: Vec<database::schema::prestashop_schema::OrderRow>,
    pub url: Option<String>,
    pub error: String,
    // `frontend: Option<WebConsoleFrontend>` was removed when the
    // GUI-side WebSocket-relay client (`tabs/websockets/mod.rs`)
    // was deleted in favor of the direct-TCP `tcp_listener` path
    // (with `terminal_mode::websockets::create_client` handling the
    // DB row creation).  The egui frame broadcast in `first_run`
    // now goes straight to `tcp_listener::broadcast_egui_frame`.

    #[cfg(target_os = "windows")]
    pub minidump_app: MiniDumpApp,
    pub file_browser: Arc<Mutex<FileBrowser>>,
    pub keys: GetKeysResponse,
    pub client: reqwest::Client,
    /// Sends requests and retrieves data from scaffold
    pub scaffold_request: SendRequest,

    pub current_antivirus: String,
    pub seb_info: Vec<CarboniteResponse>,
    pub opened_file: Option<PathBuf>,
    pub open_file_dialog: Option<FileDialog>,
    pub ram_test_cbox: HardwareTest, // We just need one of these...
    pub hdd_test_cbox: HardwareTest,
    pub ssd_test_cbox: HardwareTest,
    pub rx: Receiver<String>,
    pub open_tabs: HashSet<String>,

    pub date: DateTime<Utc>,

    pub toasts: Toasts,
    pub query_tasks_first_run: bool,
    pub get_specs: bool,
    pub send_specs: bool,
    pub spinner: bool,
    pub show_deferred_viewport: Arc<AtomicBool>,
    pub read_notifications: bool,
    pub task_data: LiveTaskPayload,
    pub ticket_data: TicketData,
    pub customer_data: CustomerData,
    pub computer_data: ComputerData,
    pub task_notes: Vec<TaskNotePayload>,
    pub service_details: Vec<database::schema::prestashop_schema::ServiceOrder>,

    pub client_uuid: RecordId,
    pub disks: Value,
    pub disk_num: usize,

    pub github_issue_title: String,
    pub github_issue_descript: String,
    pub github_issue_user: String,

    pub added_nodes: Vec<(SurfaceIndex, NodeIndex)>,

    pub prestashop_api_rx: Receiver<PrestashopPayload>,
    pub prestashop_api_tx: Sender<PrestashopPayload>,

    pub cps_keys_tx: Sender<Vec<GetKeysResponse>>,
    pub cps_keys_rx: Receiver<Vec<GetKeysResponse>>,

    pub current_antivirus_tx: Sender<Vec<(String, Option<bool>)>>,
    pub current_antivirus_rx: Receiver<Vec<(String, Option<bool>)>>,
    pub computer_data_tx: Sender<ComputerData>,
    pub computer_data_rx: Receiver<ComputerData>,
    pub bytes_tx: Sender<(u64, u64)>,
    pub bytes_rx: Receiver<(u64, u64)>,
    pub scripts_tab: EguiScriptsTab,
    pub progress: (f32, f32),
    pub special_part_order: SpecialPartOrder,
    pub toolbox: FileSystem,
    pub copied_items_tx: Sender<String>,
    pub copied_items_rx: Receiver<String>,
    pub github_releases: Vec<GithubRelease>,
    pub bytes_channel: (Sender<(Vec<u8>, u64)>, Receiver<(Vec<u8>, u64)>),
    pub github_releases_channel: (Sender<Vec<GithubRelease>>, Receiver<Vec<GithubRelease>>),
    pub seb_channel: (Sender<Vec<CarboniteResponse>>, Receiver<Vec<CarboniteResponse>>),
    pub get_settings: bool,
    pub client_friendly_name: String,
    pub client_title: String,
    pub friendly_name_tx: Sender<String>,
    pub friendly_name_rx: Receiver<String>,
    
    // Duplicate check and merge modal state
    pub duplicate_check_tx: Sender<DuplicateCheckResult>,
    pub duplicate_check_rx: Receiver<DuplicateCheckResult>,
    pub duplicate_merge_modal: Option<DuplicateMergeModal>,
    /// State for TUR submission workflow
    pub tur_submit_state: TurSubmitState,
    /// Pending TUR data waiting for duplicate resolution
    pub pending_tur_data: Option<PendingTurData>,
    pub plugin_manager: Arc<RwLock<PluginManager>>,
    pub plugin_manager_registered: bool,
    pub plugin_cmd_rx: Receiver<PluginClientCommand>,
    pub egui_frame_rx: Option<Receiver<displays::plugins::EguiFrameMessage>>,
    pub egui_input_tx: Option<Sender<displays::plugins::EguiInputEvent>>,
    // `ws_auto_connected` and `last_reconnect_attempt` were retired
    // along with the GUI-side WS-relay (`tabs/websockets/mod.rs`).
    // The direct-TCP `tcp_listener` path owns the connection now and
    // tracks its own retry state in `admin_transport.rs`.
}

/// State machine for TUR submission workflow
#[derive(Debug, Clone, Default, PartialEq)]
pub enum TurSubmitState {
    #[default]
    Idle,
    /// User clicked submit, waiting for confirmation (5-second countdown)
    AwaitingConfirmation,
    CheckingDuplicates,
    AwaitingResolution,
    Submitting,
}

/// Holds the TUR data while waiting for duplicate resolution
#[derive(Debug, Clone)]
pub struct PendingTurData {
    pub task_data: LiveTaskPayload,
    pub ticket_data: TicketData,
    pub customer_data: CustomerData,
    pub computer_data: ComputerData,
    pub task_notes: Vec<TaskNotePayload>,
    /// Stored duplicate check result for applying resolutions
    pub duplicate_check_result: Option<DuplicateCheckResult>,
    pub send_specs: bool,
    /// Timestamp when confirmation countdown started
    pub confirmation_start: Option<std::time::Instant>,
}

impl MasterTechApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let tree = default_tree();
        let (tx, rx) = crossbeam::channel::bounded::<String>(1);
        let tx_scaffold = tx.clone();
        let (prestashop_api_tx, prestashop_api_rx) = crossbeam::channel::unbounded();
        let (cps_keys_tx, cps_keys_rx) = crossbeam::channel::unbounded::<Vec<GetKeysResponse>>();
        let (bytes_tx, bytes_rx) = crossbeam::channel::unbounded::<(u64, u64)>();
        let (copied_items_tx, copied_items_rx) = crossbeam::channel::unbounded();
        let (computer_data_tx, computer_data_rx) = crossbeam::channel::unbounded();
        let (current_antivirus_tx, current_antivirus_rx) = crossbeam::channel::unbounded();
        
        let bytes_channel = <(Vec<u8>, u64)>::create_unbounded_channel();
        let github_releases_channel = <Vec<GithubRelease>>::create_unbounded_channel();
        let seb_channel = <Vec<CarboniteResponse>>::create_unbounded_channel();
        let (duplicate_check_tx, duplicate_check_rx) = crossbeam::channel::unbounded::<DuplicateCheckResult>();
        let (friendly_name_tx, friendly_name_rx) = crossbeam::channel::bounded::<String>(1);

        let sys = sysinfo::System::new_all();
        let hostname = sysinfo::System::host_name().unwrap_or_default();
        let cpu_brand = sys.cpus().first().map(|c| c.brand().trim().to_string()).unwrap_or_default();
        let client_hash = crate::filesystem::system_info::generate_client_id(hostname.clone(), cpu_brand.clone());
        let url_string = format!("{}:{}", hostname, client_hash.split_at(9).0);
        let client_uuid = RecordId::new(CONNECTED_CLIENT_TABLE.to_string(), url_string.clone());
        let client_title = url_string.clone();
        let ws_url = database::websocket_url_with_room(
            if cfg!(debug_assertions) {
                database::WS_CLIENT_URL_LOCAL
            } else {
                database::WS_CLIENT_URL
            },
            &url_string,
            "client",
        );
        log::info!("Client ID: {url_string} | WS URL: {ws_url}");

        let send_specs = true;
        // if cfg!(target_os = "windows") { true } else { false };
        
        let (plugin_dispatcher, plugin_cmd_rx) = DefaultEventDispatcher::new();
        let plugin_manager = {
            let mut mgr = PluginManager::new();
            mgr.set_dispatcher(plugin_dispatcher);
            Arc::new(RwLock::new(mgr))
        };

        let mastertech_context = MastertechContext {
            shared_ctx: SharedContext::new(cc),
            // terminal: Terminal::new(backend).unwrap(),
            // terminal_frontend: None,
            client_friendly_name: String::new(),
            order_rows: Vec::new(),
            url: Some(ws_url),
            error: Default::default(),

            keys: GetKeysResponse {
                webroot_key: "Webroot Key".to_string(),
                superanti_key: "SuperAnti Key".to_string(),
            },

            task_data: LiveTaskPayload::default(),
            computer_data: {
                // Pin computer_data.id to the canonical hostname:client_hash[..9]
                // form at startup so any code that reads it before spec-gather
                // finishes (notably the auto-connect path in first_run.rs that
                // writes `connected_client.computer`) sees the same id that
                // get_computer_data() will eventually upsert into the computer
                // table. Otherwise the default random UUID leaks into
                // connected_client.computer and produces a dangling reference.
                //
                // One-off repair for already-dangling rows written before this
                // fix (run once against prod):
                //   UPDATE connected_client
                //   SET computer = type::thing('computer', connection_string)
                //   WHERE computer NOT IN (SELECT VALUE id FROM computer);
                let mut cd = ComputerData::default();
                cd.id = RecordId::new(COMPUTER_TABLE.to_string(), url_string.clone());
                cd.hostname = hostname;
                cd.cpu = cpu_brand;
                cd
            },
            ticket_data: TicketData::default(),
            customer_data: CustomerData::default(),
            task_notes: Vec::new(),
            service_details: Vec::new(),
            
            seb_info: Vec::new(),

            disks: Value::Array(vec![]),
            disk_num: 0,
            scaffold_request: SendRequest { tx: tx_scaffold },
            client: reqwest::Client::new(),
            file_browser: Arc::new(Mutex::new(FileBrowser::new())),
            current_antivirus: "".to_string(),
            opened_file: None,
            open_file_dialog: None,

            ram_test_cbox: scaffold::HardwareTest::RamNotTested,
            hdd_test_cbox: scaffold::HardwareTest::HddNotTested,
            ssd_test_cbox: scaffold::HardwareTest::SsdNotTested,
            #[cfg(target_os = "windows")]
            minidump_app: MiniDumpApp::default(),

            client_uuid,
            rx,

            //////////////////////////////////////////
            /*          Widgets and UI elements     */
            //////////////////////////////////////////
            toasts: Toasts::new().anchor(Align2::RIGHT_TOP, (5.0, 5.0)),
            // ctx: Context::default(),
            open_tabs: tree.1,

            date: chrono::offset::Utc::now(),

            send_specs,

            query_tasks_first_run: true,
            get_specs: false,
            spinner: false,
            read_notifications: false,
            show_deferred_viewport: Arc::new(AtomicBool::new(false)),
            added_nodes: Default::default(),

            prestashop_api_tx, prestashop_api_rx,
            bytes_tx, bytes_rx,
            cps_keys_tx, cps_keys_rx,
            copied_items_tx, copied_items_rx,
            computer_data_tx, computer_data_rx,
            current_antivirus_tx, current_antivirus_rx,
            github_releases_channel,

            github_issue_title: Default::default(),
            github_issue_descript: Default::default(),
            github_issue_user: Default::default(),
            scripts_tab: EguiScriptsTab::new(),
            progress: (0.0, 0.0),
            special_part_order: Default::default(),
            github_releases: Default::default(),
            toolbox: FileSystem::new(),
            bytes_channel,

            // Data table shit
            seb_channel,
            get_settings: true,
            client_title,
            friendly_name_tx,
            friendly_name_rx,
            
            // Duplicate check and merge modal
            duplicate_check_tx,
            duplicate_check_rx,
            duplicate_merge_modal: None,
            tur_submit_state: TurSubmitState::Idle,
            pending_tur_data: None,
            plugin_manager,
            plugin_manager_registered: false,
            plugin_cmd_rx,
            egui_frame_rx: None,
            egui_input_tx: None,
        };
        
        let context = mastertech_context;

        Self {
            context,
            tree: tree.0,
        }
    }
}

