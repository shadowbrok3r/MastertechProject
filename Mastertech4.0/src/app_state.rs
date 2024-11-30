use crossbeam::channel::{Receiver, Sender};
use database::{
    schema::{
        get_data::NewTicketChannel, prestashop_schema::PrestashopPayload, ComputerData, ConnectedClient, CustomerData, GetKeysResponse, LiveTaskPayload, LocalSebData, Notification, TaskNotePayload, TaskPayload, TicketData, User, CONNECTED_CLIENT_TABLE
    },
    Database,
};
use displays::{
    app_state::SharedContext, channel_manager::ChannelManager, egui_data_table::DataTable, modals::{modal_types::ModalTypes, task_modal::ModalAction}, ui_tools::{mention_handler::MentionHandler, theme_config::{set_custom_style, ThemeConfig}, toasts::Toasts}, virtual_filesystem::FileSystem
};
use eframe::egui::{Align2, Color32, Context, FontData, FontDefinitions, FontFamily, FontId, Stroke, Style};
use egui_dock::{DockState, Node, NodeIndex, SurfaceIndex};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc, Mutex},
};
use surrealdb::{sql::Uuid, Action, RecordId};
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
        stock::{MyRowData, MyRowViewer, RawStockData, SerialData},
        stock_quantities::{ExtraInventoryData, StockQuantityData, StockQuantityViewer},
        tur_sheet::{
            get_ticket::SendRequest,
            scaffold::{self, HardwareTest},
        },
        websockets::WebConsoleFrontend,
    }
};
use displays::{
    chats::ChatView,
    modals::{
        create_task_modal::CreateTaskModal, task_modal::SpecialPartOrder, ChatModalHandler,
        Modal, ModalHandler, TaskModalHandler, ModalType
    },
    tasks::task_layout::TaskLayout,
    DisplayModal,
};

pub struct MasterTechApp {
    pub context: MastertechContext,
    pub tree: DockState<String>,
    pub state: AppState,
    login: Login,
}

#[derive(Default, Debug, PartialEq)]
pub enum MainPages {
    #[default]
    Tasks,
    Downloads,
    WebConsole,
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
    pub task_modal_handler: TaskModalHandler,
    pub create_task_modal_handler: ModalHandler<CreateTaskModal>,
    pub chat_modal_handler: ChatModalHandler,
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

    pub connected_clients_tx: Sender<Vec<ConnectedClient>>,
    pub connected_clients_rx: Receiver<Vec<ConnectedClient>>,

    pub db_rx: Receiver<anyhow::Result<Database, Error>>,
    pub db_tx: Sender<anyhow::Result<Database, Error>>,
    pub cps_keys_tx: Sender<GetKeysResponse>,
    pub cps_keys_rx: Receiver<GetKeysResponse>,
    pub extra_stock_channel: (
        Sender<Vec<ExtraInventoryData>>,
        Receiver<Vec<ExtraInventoryData>>,
    ),
    pub notes_tx: Sender<(Action, TaskNotePayload)>,
    pub notes_rx: Receiver<(Action, TaskNotePayload)>,
    pub new_note_tx: Sender<TaskNotePayload>,
    pub new_note_rx: Receiver<TaskNotePayload>,

    pub store_users: Vec<User>,
    pub store_users_tx: Sender<Vec<User>>,
    
    pub store_users_rx: Receiver<Vec<User>>,
    pub initial_tasks_tx: Sender<Vec<TaskPayload>>,
    pub initial_tasks_rx: Receiver<Vec<TaskPayload>>,
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
    pub live_notification_tx: Sender<(Action, Notification)>,
    pub live_notification_rx: Receiver<(Action, Notification)>,
    pub notification_tx: Sender<Vec<Notification>>,
    pub notification_rx: Receiver<Vec<Notification>>,
    pub live_tasks_tx: Sender<(Action, LiveTaskPayload)>,
    pub live_tasks_rx: Receiver<(Action, LiveTaskPayload)>,
    pub new_ticket_tx: Sender<NewTicketChannel>,
    pub new_ticket_rx: Receiver<NewTicketChannel>,

    
    pub data_viewer: MyRowViewer,
    pub data_table: DataTable<MyRowData>,
    pub seb_channel: (Sender<Vec<Value>>, Receiver<Vec<Value>>),
    pub stock_data: RawStockData,
    pub stock_channel: (Sender<Vec<RawStockData>>, Receiver<Vec<RawStockData>>),
    pub serial_channel: (Sender<SerialData>, Receiver<SerialData>),
    /// Data viewer for Stock Quantities tab
    pub stock_quantity_viewer: StockQuantityViewer,
    /// Data for Stock Quantities tab
    pub stock_quantity_table: DataTable<StockQuantityData>,
    pub store_selection: u64,
    pub json_editor: JsonEditor,
    /// Theme settings
    pub theme_config: ThemeConfig,
    /// Button state for modifying theme config
    pub modify_theme: bool,
    /// The theme itself
    pub theme: Arc<Style>,

}

impl MasterTechApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_custom_fonts(&cc.egui_ctx);

        let mut tree = DockState::new(vec![
            "TUR Sheet".to_owned(),
            "Tasks".to_owned(),
            // "Part Order".to_owned(),
            // "Minidump Analysis".to_owned(),
            "SEB Lookup".to_owned(),
            "Downloads".to_owned(),
            "Stock".to_owned(),
            "Stock Quantity".to_owned(),
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

        // let _backend = RataguiBackend::new_with_fonts(
        //     10,
        //     10,
        //     "Regular".into(),
        //     "Bold".into(),
        //     "Oblique".into(),
        //     "BoldOblique".into(),
        // );

        let (tx, rx) = crossbeam::channel::bounded::<String>(1);
        let tx_scaffold = tx.clone();
        let (db_data_sender, db_data_receiver) =
            crossbeam::channel::unbounded::<Vec<TaskPayload>>();
        let (prestashop_api_tx, prestashop_api_rx) = crossbeam::channel::unbounded();
        let (computer_specs_tx, computer_specs_rx) = crossbeam::channel::unbounded();
        let (db_tx, db_rx) = crossbeam::channel::unbounded();
        let (cps_keys_tx, cps_keys_rx) = crossbeam::channel::unbounded::<GetKeysResponse>();
        let (app_state_tx, app_state_rx) = crossbeam::channel::unbounded::<AppState>();
        let (connected_clients_tx, connected_clients_rx) =
            crossbeam::channel::unbounded::<Vec<ConnectedClient>>();
        let (store_users_tx, store_users_rx) = crossbeam::channel::unbounded::<Vec<User>>();
        let (initial_tasks_tx, initial_tasks_rx) =
            crossbeam::channel::unbounded::<Vec<TaskPayload>>();
        let (bytes_tx, bytes_rx) = crossbeam::channel::unbounded::<(u64, u64)>();
        let minio_files = <Vec<String>>::create_unbounded_channel();
        let (copied_items_tx, copied_items_rx) = crossbeam::channel::unbounded();
        let bytes_channel = <(Vec<u8>, u64)>::create_unbounded_channel();
        let github_releases_channel = <Vec<GithubRelease>>::create_unbounded_channel();
        let stock_channel = <Vec<RawStockData>>::create_unbounded_channel();
        let serial_channel = <SerialData>::create_unbounded_channel();
        let seb_channel = <Vec<Value>>::create_unbounded_channel();
        let extra_stock_channel = <Vec<ExtraInventoryData>>::create_unbounded_channel();
        let tur_channel = PrestashopPayload::create_unbounded_channel();
        let (notes_tx, notes_rx) = crossbeam::channel::unbounded::<(Action, TaskNotePayload)>();
        let (new_note_tx, new_note_rx) = crossbeam::channel::unbounded::<TaskNotePayload>();
        let (live_notification_tx, live_notification_rx) = crossbeam::channel::unbounded::<(Action, Notification)>();
        let (live_tasks_tx, live_tasks_rx) = crossbeam::channel::unbounded::<(Action, LiveTaskPayload)>();
        let (new_ticket_tx, new_ticket_rx) = crossbeam::channel::unbounded::<NewTicketChannel>();
        let (notification_tx, notification_rx) = crossbeam::channel::unbounded::<Vec<Notification>>();

        let mut data_viewer = MyRowViewer::default();
        data_viewer.stock_tx = Some(serial_channel.0.clone());
        let client_uuid = RecordId::from((CONNECTED_CLIENT_TABLE, Uuid::new_v4().to_string()));
        
        let theme_config = ThemeConfig::default();
        let theme = set_custom_style(&theme_config);

        let mastertech_context = MastertechContext {
            shared_ctx: SharedContext::default(),
            // terminal: Terminal::new(backend).unwrap(),
            // terminal_frontend: None,
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
            store_users: Vec::new(),
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
            open_tabs,

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
            task_modal_handler: TaskModalHandler::default(),
            create_task_modal_handler: ModalHandler::default(),
            chat_modal: None,
            chat_modal_handler: ChatModalHandler::default(),

            db_data_receiver,
            db_data_sender,
            prestashop_api_tx,
            prestashop_api_rx,
            computer_specs_tx,
            computer_specs_rx,
            app_state_tx,
            app_state_rx,
            connected_clients_tx,
            connected_clients_rx,
            bytes_tx,
            bytes_rx,
            db_tx,
            db_rx,
            cps_keys_tx,
            cps_keys_rx,
            copied_items_tx,
            copied_items_rx,
            notes_tx,
            notes_rx,
            new_note_tx,
            new_note_rx,
            github_releases_channel,
            tur_channel,
            live_notification_tx,
            live_notification_rx,
            live_tasks_tx, live_tasks_rx,
            new_ticket_tx, new_ticket_rx,
            notification_tx, notification_rx,

            store_users_tx,
            store_users_rx,
            initial_tasks_tx,
            initial_tasks_rx,
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
            data_table: DataTable::<MyRowData>::default(),
            data_viewer,
            stock_data: RawStockData::default(),
            stock_channel,
            serial_channel,
            store_selection: 76,
            seb_channel,
            json_editor: JsonEditor::default(),
            extra_stock_channel,
            stock_quantity_viewer: StockQuantityViewer::default(),
            stock_quantity_table: DataTable::<StockQuantityData>::default(),
            theme,
            theme_config,
            modify_theme: false,
        };
        
        let context = mastertech_context;

        Self {
            context,
            tree,
            login: Login::default(),
            state: AppState::default(),
        }
    }
}

impl MastertechContext {
    pub fn handle_modals(&mut self, ctx: &Context) {
        match &mut self.current_modal {
            ModalType::TaskModal(task_modal) => {
                let task_name = task_modal.task.task_name.clone();
                self.task_modal_handler.ui(
                    ctx,
                    || Modal::new(&task_name).default_height(600.0).min_width(680.),
                    move |ui, open, page_state| {
                        ui.set_max_width(500.);
                        let action = task_modal.display(ui, page_state.to_owned());
                        if let Some(action) = action {
                            if let ModalAction::Close = action {
                                *open = false;
                            }
                            *page_state = action;
                        }
                    },
                );
            }
            ModalType::CreateTaskModal(create_task_modal) => {
                self.create_task_modal_handler.ui(
                    ctx,
                    || {
                        CreateTaskModal::new(
                            "Create Task",
                            self.shared_ctx.store_users.clone(),
                            self.tur_channel.0.clone(),
                        )
                        .default_height(600.0)
                        .min_width(680.)
                    },
                    |ui, open, page_state| {
                        let action = create_task_modal.display(ui, page_state.to_owned());
                        if let Some(action) = action {
                            // This will allow me to close the modal
                            // upon ModalAction::Close (when creating a task)
                            if let ModalAction::Close = action {
                                *open = false;
                            }
                            // Otherwise, handle the according ModalAction
                            *page_state = action;
                        }
                    },
                );
            }
            ModalType::ChatView(chat_modal) => {
                self.chat_modal_handler.ui(
                    ctx,
                    || Modal::new("Chats"),
                    move |ui, _stay_open, _page_state| {
                        ui.style_mut().override_font_id = Some(FontId::proportional(13.0));
                        if let Some(_new_message) = chat_modal.ui(ui) {}
                    },
                );
            }
            _ => {}
        }
    }
}
/// Private method to access login state only within NoAuth context
impl MasterTechApp {
    pub fn login_mut(&mut self) -> Option<&mut Login> {
        match self.state {
            AppState::Login => Some(&mut self.login),
            AppState::Authenticated(MainPages::Tasks) => None,
            _ => None,
        }
    }
}

fn setup_custom_fonts(ctx: &Context) {
    // Start with the default fonts (we will be adding to them rather than replacing them).
    let mut fonts = FontDefinitions::default();

    fonts.font_data.insert(
        "Monaspace".to_owned(),
        FontData::from_static(include_bytes!("./assets/fonts/MonaspaceNeon-Light.otf")),
    ); // .ttf and .otf supported

    // Put my font first (highest priority):
    fonts
        .families
        .get_mut(&FontFamily::Proportional)
        .unwrap()
        .insert(0, "Monaspace".to_owned());

    fonts.font_data.insert(
        "Regular".to_owned(),
        FontData::from_static(include_bytes!("./assets/fonts/MonaspaceNeon-Regular.otf")),
    );
    fonts.families.insert(
        FontFamily::Name("Regular".into()),
        vec!["Regular".to_owned()],
    );
    fonts.font_data.insert(
        "Bold".to_owned(),
        FontData::from_static(include_bytes!("./assets/fonts/MonaspaceNeon-Bold.otf")),
    );
    fonts
        .families
        .insert(FontFamily::Name("Bold".into()), vec!["Bold".to_owned()]);

    // Tell egui to use these fonts:
    ctx.set_fonts(fonts);
}
