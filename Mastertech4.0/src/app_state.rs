use crossbeam::channel::{Receiver, Sender};
use database::{
    schema::{
        prestashop_schema::PrestashopPayload, ComputerData, CustomerData, GetKeysResponse, LiveTaskPayload, LocalSebData, Notification, TaskNotePayload, TaskPayload, TicketData, CONNECTED_CLIENT_TABLE
    },
    Database,
};
use displays::{
    app_state::SharedContext, channel_manager::ChannelManager, ui_tools::{mention_handler::MentionHandler, toasts::Toasts}, virtual_filesystem::FileSystem
};
use eframe::egui::{Align2, Color32, Context, Stroke};
use egui_dock::{DockState, Node, NodeIndex, SurfaceIndex};
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc, Mutex},
};
use surrealdb::{sql::Uuid, RecordId};
// use egui_ratatui::RataguiBackend;
use anyhow::Error;
use chrono::{DateTime, Utc};
use egui_file::FileDialog;
use serde_json::Value;

#[cfg(target_os = "windows")]
use crate::tabs::minidump::MiniDumpApp;

use crate::{
    pages::login_page::Login,
    tabs::{
        file_browser::FileBrowser,
        github::self_updater::GithubRelease,
        scripts::Scripts,
        seb_lookup::JsonEditor,
        tur_sheet::{
            get_ticket::SendRequest,
            scaffold::{self, HardwareTest},
        },
        websockets::WebConsoleFrontend,
    }
};
use displays::{
    chats::ChatView,
    modals::{task_modal::SpecialPartOrder, ModalType},
    tasks::task_layout::TaskLayout,
    // DisplayModal,
};

pub struct MasterTechApp {
    pub context: MastertechContext,
    pub tree: DockState<String>,
    pub state: AppState,
    login: Login,
}

#[derive(Serialize, Default, Debug, PartialEq)]
pub enum MainPages {
    #[default]
    Tasks,
    ChatGpt,
    Downloads,
    WebConsole,
    AccountSettings,
}

#[derive(Debug, PartialEq)]
pub enum AppState {
    Authenticated(MainPages),
    CreateAccount,
    NoAuth(String),
    Login,
}

impl Default for AppState {
    fn default() -> Self {
        Self::NoAuth("Not Authenticated".to_string())
    }
}

pub struct MastertechContext {
    pub shared_ctx: SharedContext,
    pub app_state_tx: Sender<AppState>,
    pub app_state_rx: Receiver<AppState>,

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
    pub seb_info: Option<LocalSebData>,
    pub opened_file: Option<PathBuf>,
    pub open_file_dialog: Option<FileDialog>,
    pub mention_handler: MentionHandler,
    pub ram_test_cbox: HardwareTest, // We just need one of these...
    pub hdd_test_cbox: HardwareTest,
    pub ssd_test_cbox: HardwareTest,

    pub output_text: String,

    pub database: Option<Database>,
    pub rx: Receiver<String>,
    pub ctx: Context,
    pub widget_size: f32,
    pub open_tabs: HashSet<String>,

    pub date: Option<DateTime<Utc>>,

    pub reader_bytes: u32,

    pub toasts: Toasts,
    pub animate_progress_bar: bool,
    pub first_run: bool,
    pub taco_first_run: bool,
    pub file_browse_run: bool,
    pub query_tasks_first_run: bool,
    pub get_specs: bool,
    pub send_specs: bool,
    pub spinner: bool,
    pub new_note: bool,
    pub style: Option<egui_dock::Style>,
    pub text_color: Color32,
    pub border_stroke_color: Stroke,
    pub frame_counter: u64,
    pub show_deferred_viewport: Arc<AtomicBool>,
    pub show_ws_viewport: Arc<AtomicBool>,
    pub read_notifications: bool,
    pub notifications: Vec<Notification>,

    pub task_map: HashMap<String, Vec<TaskPayload>>,
    pub task_layouts: HashMap<String, TaskLayout>,
    pub current_modal: ModalType,
    pub chat_modal: Option<ChatView>,
    pub task_data: LiveTaskPayload,
    pub ticket_data: TicketData,
    pub customer_data: CustomerData,
    pub computer_data: ComputerData,
    // pub computer_data_test: Arc<Mutex<ComputerData>>,
    pub task_notes: Vec<TaskNotePayload>,

    pub client_uuid: RecordId,
    pub disks: Value,
    pub disk_num: usize,

    pub github_issue_title: String,
    pub github_issue_descript: String,

    pub added_nodes: Vec<(SurfaceIndex, NodeIndex)>,

    // pub presta_data: PrestaDataChannel<T>,
    pub db_data_receiver: Receiver<Vec<TaskPayload>>,
    pub db_data_sender: Sender<Vec<TaskPayload>>,
    pub prestashop_api_rx: Receiver<PrestashopPayload>,
    pub prestashop_api_tx: Sender<PrestashopPayload>,
    pub computer_specs_tx: Sender<ComputerData>,
    pub computer_specs_rx: Receiver<ComputerData>,

    pub db_rx: Receiver<anyhow::Result<Database, Error>>,
    pub db_tx: Sender<anyhow::Result<Database, Error>>,
    pub cps_keys_tx: Sender<GetKeysResponse>,
    pub cps_keys_rx: Receiver<GetKeysResponse>,

    pub bytes_tx: Sender<(u64, u64)>,
    pub bytes_rx: Receiver<(u64, u64)>,
    pub tur_channel: (Sender<PrestashopPayload>, Receiver<PrestashopPayload>),
    pub scripts: Scripts,
    pub progress: (f32, f32),
    pub special_part_order: SpecialPartOrder,
    pub toolbox: FileSystem,
    pub minio_files: (Sender<Vec<String>>, Receiver<Vec<String>>),
    pub copied_items_tx: Sender<String>,
    pub copied_items_rx: Receiver<String>,
    pub github_releases: Vec<GithubRelease>,
    pub bytes_channel: (Sender<(Vec<u8>, u64)>, Receiver<(Vec<u8>, u64)>),
    pub github_releases_channel: (Sender<Vec<GithubRelease>>, Receiver<Vec<GithubRelease>>),
    
    pub seb_channel: (Sender<Vec<Value>>, Receiver<Vec<Value>>),
    pub json_editor: JsonEditor,
    pub update_settings: bool,
    pub get_settings: bool,
    pub seb_email: String,
    pub client_friendly_name: String,
    pub client_title: String
}

impl MasterTechApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let tree = default_tree();
        let (tx, rx) = crossbeam::channel::bounded::<String>(1);
        let tx_scaffold = tx.clone();
        let (db_data_sender, db_data_receiver) =
            crossbeam::channel::unbounded::<Vec<TaskPayload>>();
        let (prestashop_api_tx, prestashop_api_rx) = crossbeam::channel::unbounded();
        let (computer_specs_tx, computer_specs_rx) = crossbeam::channel::unbounded();
        let (db_tx, db_rx) = crossbeam::channel::unbounded();
        let (cps_keys_tx, cps_keys_rx) = crossbeam::channel::unbounded::<GetKeysResponse>();
        let (app_state_tx, app_state_rx) = crossbeam::channel::unbounded::<AppState>();
        let (bytes_tx, bytes_rx) = crossbeam::channel::unbounded::<(u64, u64)>();
        let minio_files = <Vec<String>>::create_unbounded_channel();
        let (copied_items_tx, copied_items_rx) = crossbeam::channel::unbounded();
        let bytes_channel = <(Vec<u8>, u64)>::create_unbounded_channel();
        let github_releases_channel = <Vec<GithubRelease>>::create_unbounded_channel();
        let seb_channel = <Vec<Value>>::create_unbounded_channel();
        let tur_channel = PrestashopPayload::create_unbounded_channel();

        let client_uuid = RecordId::from((CONNECTED_CLIENT_TABLE, Uuid::new_v4().to_string()));
        
        let mastertech_context = MastertechContext {
            shared_ctx: SharedContext::new(cc),
            // terminal: Terminal::new(backend).unwrap(),
            // terminal_frontend: None,
            client_friendly_name: String::new(),
            url: None,
            error: Default::default(),
            frontend: None,

            keys: GetKeysResponse {
                webroot_key: "Webroot Key".to_string(),
                superanti_key: "SuperAnti Key".to_string(),
            },

            task_data: LiveTaskPayload::default(),
            computer_data: ComputerData::default(),
            // computer_data_test: Arc::new(Mutex::new(ComputerData::default())),
            ticket_data: TicketData::default(),
            customer_data: CustomerData::default(),
            task_notes: Vec::new(),

            seb_info: None,

            disks: Value::Array(vec![]),
            disk_num: 0,
            scaffold_request: SendRequest { tx: tx_scaffold },
            client: reqwest::Client::new(),
            file_browser: Arc::new(Mutex::new(FileBrowser::new())),
            current_antivirus: "".to_string(),
            opened_file: None,
            open_file_dialog: None,
            mention_handler: MentionHandler::default(),

            database: None,

            ram_test_cbox: scaffold::HardwareTest::RamNotTested,
            hdd_test_cbox: scaffold::HardwareTest::HddNotTested,
            ssd_test_cbox: scaffold::HardwareTest::SsdNotTested,
            #[cfg(target_os = "windows")]
            minidump_app: MiniDumpApp::default(),
            output_text: "".to_string(),

            client_uuid,
            rx,

            task_layouts: HashMap::new(),
            task_map: HashMap::new(),
            //////////////////////////////////////////
            /*          Widgets and UI elements     */
            //////////////////////////////////////////
            toasts: Toasts::new().anchor(Align2::RIGHT_TOP, (5.0, 5.0)),
            ctx: Context::default(),
            widget_size: 135.0,
            open_tabs: tree.1,

            date: None,
            animate_progress_bar: false,
            reader_bytes: 0,

            send_specs: false,

            first_run: true,
            taco_first_run: false,
            file_browse_run: false,
            query_tasks_first_run: true,
            get_specs: false,
            spinner: false,
            new_note: false,
            read_notifications: false,
            notifications: Vec::new(),
            style: None,
            text_color: Color32::from_rgb(255, 204, 230),
            border_stroke_color: Stroke::new(1.0, Color32::from_rgb_additive(150, 62, 124)),

            frame_counter: 0,
            show_deferred_viewport: Arc::new(AtomicBool::new(false)),
            show_ws_viewport: Arc::new(AtomicBool::new(false)),
            added_nodes: Vec::new(),

            current_modal: ModalType::Null,
            chat_modal: None,

            db_data_receiver,
            db_data_sender,
            prestashop_api_tx,
            prestashop_api_rx,
            computer_specs_tx,
            computer_specs_rx,
            app_state_tx,
            app_state_rx,
            bytes_tx,
            bytes_rx,
            db_tx,
            db_rx,
            cps_keys_tx,
            cps_keys_rx,
            copied_items_tx,
            copied_items_rx,
            github_releases_channel,
            tur_channel,

            github_issue_title: String::new(),
            github_issue_descript: String::new(),
            scripts: Scripts::default(),
            progress: (0.0, 0.0),
            special_part_order: SpecialPartOrder::default(),
            toolbox: FileSystem::new(),
            minio_files,
            github_releases: Vec::new(),
            bytes_channel,

            // Data table shit
            seb_channel,
            json_editor: JsonEditor::default(),
            update_settings: false,
            get_settings: true,
            seb_email: String::new(),
            client_title: String::new()
        };
        
        let context = mastertech_context;

        Self {
            context,
            tree: tree.0,
            login: Login::default(),
            state: AppState::default(),
        }
    }

    /// Private method to access login state only within NoAuth context
    pub fn login_mut(&mut self) -> Option<&mut Login> {
        match self.state {
            AppState::Login => Some(&mut self.login),
            AppState::Authenticated(MainPages::Tasks) => None,
            _ => None,
        }
    }
}



pub fn default_tree() -> (DockState<String>, HashSet<String>) {
    let mut tree = DockState::new(vec![
        "TUR Sheet".to_owned(),
        "My Tasks".to_owned(),
        "Store Tasks".to_owned(),
        "Completed Tasks".to_owned(),
        // "Part Order".to_owned(),
        // "Minidump Analysis".to_owned(),
        "SEB Lookup".to_owned(),
        "Downloads".to_owned(),
        "Store Stock".to_owned(),
        "Company Stock".to_owned(),
        "Ai".to_owned(),
    ]);
    tree.translations.tab_context_menu.eject_button = "Undock".to_owned();

    let [_a, _b] = tree.main_surface_mut().split_left(
        NodeIndex::root(),
        0.30,
        vec!["File Browser 📂".to_owned(), "Logs".to_owned()],
    );
    let [_a, b] = tree.main_surface_mut().split_below(
        NodeIndex::root(),
        0.65,
        vec!["Console".to_owned(), "Websockets".to_owned()],
    );
    let [_, _] = tree.main_surface_mut().split_left(
        b,
        0.45,
        vec!["SysInfo".to_owned(), "Bug Tracker".to_owned()],
    );
    let [_, _] = tree.main_surface_mut().split_left(
        b,
        0.20,
        vec!["Scripts".to_owned(), "ToolBox".to_owned()],
    );

    let mut open_tabs = HashSet::new();
    for node in tree[SurfaceIndex::main()].iter() {
        if let Node::Leaf { tabs, .. } = node {
            for tab in tabs {
                open_tabs.insert(tab.clone());
            }
        }
    }
    (tree, open_tabs)
}
