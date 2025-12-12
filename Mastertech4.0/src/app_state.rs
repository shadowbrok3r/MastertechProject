use database::{schema::{prestashop_schema::PrestashopPayload, CarboniteResponse, ComputerData, CustomerData, DuplicateCheckResult, DuplicateResolution, GetKeysResponse, LiveTaskPayload, TaskNotePayload, TicketData, CONNECTED_CLIENT_TABLE}};
use displays::modals::DuplicateMergeModal;
use crate::{tabs::{file_browser::FileBrowser, github::self_updater::GithubRelease, scripts::EguiScriptsTab, tur_sheet::{get_ticket::SendRequest,scaffold::{self, HardwareTest}}, websockets::WebConsoleFrontend}};
use displays::{app_state::{default_tree, SharedContext}, channel_manager::ChannelManager, modals::task_modal::SpecialPartOrder, ui_tools::toasts::Toasts, virtual_filesystem::FileSystem};
use std::{collections::HashSet,path::PathBuf,sync::{atomic::AtomicBool, Arc, Mutex}};
use egui_dock::{DockState, NodeIndex, SurfaceIndex};
use crossbeam::channel::{Receiver, Sender};
use surrealdb::{sql::Uuid, RecordId};
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
    pub frontend: Option<WebConsoleFrontend>,

    #[cfg(target_os = "windows")]
    pub minidump_app: MiniDumpApp,
    pub file_browser: Arc<Mutex<FileBrowser>>,
    // pub terminal_frontend: Option<TerminalFrontend>,
    // pub terminal: Terminal<RataguiBackend>,
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
    pub show_ws_viewport: Arc<AtomicBool>,
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
    
    // Duplicate check and merge modal state
    pub duplicate_check_tx: Sender<DuplicateCheckResult>,
    pub duplicate_check_rx: Receiver<DuplicateCheckResult>,
    pub duplicate_merge_modal: Option<DuplicateMergeModal>,
    /// State for TUR submission workflow
    pub tur_submit_state: TurSubmitState,
    /// Pending TUR data waiting for duplicate resolution
    pub pending_tur_data: Option<PendingTurData>,
}

/// State machine for TUR submission workflow
#[derive(Debug, Clone, Default, PartialEq)]
pub enum TurSubmitState {
    #[default]
    Idle,
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
    pub send_specs: bool,
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
        let client_uuid = RecordId::from((CONNECTED_CLIENT_TABLE, Uuid::new_v4().to_string()));

        let send_specs = true;
        // if cfg!(target_os = "windows") { true } else { false };
        
        let mastertech_context = MastertechContext {
            shared_ctx: SharedContext::new(cc),
            // terminal: Terminal::new(backend).unwrap(),
            // terminal_frontend: None,
            client_friendly_name: String::new(),
            order_rows: Vec::new(),
            url: None,
            error: Default::default(),
            frontend: None,

            keys: GetKeysResponse {
                webroot_key: "Webroot Key".to_string(),
                superanti_key: "SuperAnti Key".to_string(),
            },

            task_data: LiveTaskPayload::default(),
            computer_data: ComputerData::default(),
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
            show_ws_viewport: Arc::new(AtomicBool::new(false)),
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
            client_title: Default::default(),
            
            // Duplicate check and merge modal
            duplicate_check_tx,
            duplicate_check_rx,
            duplicate_merge_modal: None,
            tur_submit_state: TurSubmitState::Idle,
            pending_tur_data: None,
        };
        
        let context = mastertech_context;

        Self {
            context,
            tree: tree.0,
        }
    }
}

