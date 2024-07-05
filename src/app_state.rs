use std::{collections::{HashMap, HashSet}, path::PathBuf, sync::{atomic::AtomicBool, Arc, Mutex}}; 
use anyhow::Error;
use chrono::{DateTime, Utc};
use crossbeam::channel::{Receiver, Sender};
use eframe::egui::{Align2, Color32, Context, FontData, FontDefinitions, FontFamily, Stroke, Ui, WidgetText};
use egui_toast::Toasts;
use serde_json::Value;
use egui_dock::{Node, NodeIndex, SurfaceIndex, DockState, TabViewer};
use crate::{database::{database::Database, schema::{ClientId, ComputerData, ConnectedClient, CustomerData, LiveTaskPayload, LocalSebData, PrestashopPayload, TaskNotePayload, TaskPayload, TicketData, User}, GetKeysResponse}, pages::login_page::Login, tabs::{file_browser::FileBrowser, mastertech_website::task_layout::TaskLayout, minidump::MiniDumpApp, tur_sheet::{get_ticket::SendRequest, scaffold::{self, HardwareTest}}, websockets::{websocket::TerminalFrontend, WebConsoleFrontend}}, utilities::TaskUiActions};
use egui_file::FileDialog;
use ratframe::NewCC;
use egui_ratatui::RataguiBackend;
// use uuid::Uuid;
// use ratatui::Terminal;

pub struct MasterTechApp {
    pub context: MastertechContext,
    pub tree: DockState<String>,
    pub state: AppState,
    login: Login,
}

#[derive(Default, Debug, PartialEq)]
pub enum MainPages{
    #[default]
    Tasks,
    Downloads,
    WebConsole,
}

#[derive(Debug, PartialEq)]
pub enum AppState{
    Authenticated(MainPages),
    CreateAccount,
    NoAuth(String),
}

impl Default for AppState{
    fn default() -> Self {
        Self::NoAuth("Not Authenticated".to_string())
    }
}

pub struct MastertechContext { 
    pub app_state_tx: Sender<AppState>,
    pub app_state_rx: Receiver<AppState>,

    pub current_user: Option<User>,
    pub url: Option<String>,
    pub error: String,
    pub frontend: Option<WebConsoleFrontend>,
    pub minidump_app: MiniDumpApp,
    pub file_browser: Arc<Mutex<FileBrowser>>,
    pub terminal_frontend: Option<TerminalFrontend>,
    // pub terminal: Terminal<RataguiBackend>,

    pub keys: GetKeysResponse,
    pub client: reqwest::Client,
    /// Sends requests and retrieves data from scaffold
    pub scaffold_request: SendRequest,
    
    pub current_antivirus: String,
    pub seb_info: Option<LocalSebData>,
    pub opened_file: Option<PathBuf>,
    pub open_file_dialog: Option<FileDialog>,

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
    pub specs_first_run: bool,
    pub file_browse_run: bool,
    pub query_tasks_first_run: bool,
    pub get_specs: bool,
    pub send_specs: bool,
    pub spinner: bool,
    
    pub style: Option<egui_dock::Style>,
    pub text_color: Color32,
    pub border_stroke_color: Stroke,
    pub frame_counter: u64,
    pub show_deferred_viewport: Arc<AtomicBool>,

    pub task_map: HashMap<String, Vec<TaskPayload>>,
    pub task_layouts: HashMap<String, TaskLayout>,

    pub task_payload: Option<Vec<TaskPayload>>,
    pub task_data: LiveTaskPayload,
    pub ticket_data: TicketData,
    pub customer_data: CustomerData,
    pub computer_data: ComputerData,
    pub task_notes: Vec<TaskNotePayload>,

    pub client_uuid: Option<ClientId>,
    pub disks: Value,
    pub disk_num: usize,

    pub github_issue_title: String,
    pub github_issue_descript: String,

    
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
    pub ui_actions_tx: Sender<TaskUiActions>,
    pub ui_actions_rx: Receiver<TaskUiActions>,

    pub store_users: Option<Vec<User>>,
    pub store_users_tx: Sender<Vec<User>>,
    pub store_users_rx: Receiver<Vec<User>>,
    pub initial_tasks_tx: Sender<Vec<TaskPayload>>,
    pub initial_tasks_rx: Receiver<Vec<TaskPayload>>,
    pub bytes_tx: Sender<(u64, u64)>,
    pub bytes_rx: Receiver<(u64, u64)>,
}

impl MasterTechApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_custom_fonts(&cc.egui_ctx);

        let mut tree = DockState::new(
            vec!["TUR Sheet".to_owned(), "Tasks".to_owned(), "Minidump Analysis".to_owned()]
        );
        tree.translations.tab_context_menu.eject_button = "Undock".to_owned();

        let [_a, _b] = tree.main_surface_mut()
            .split_left(NodeIndex::root(),0.30, vec!["File Browser 📂".to_owned(),]);
        let [_a, b] = tree.main_surface_mut()
            .split_below(NodeIndex::root(),0.65, vec!["Console".to_owned(),"Websockets".to_owned()]);
        let [_, _] = tree.main_surface_mut()
            .split_left(b, 0.45, vec!["System Information".to_owned(),"Bug Tracker".to_owned()]);
        let [_, _] = tree.main_surface_mut()
            .split_left(b,0.20,vec!["Scripts".to_owned()]);

        let mut open_tabs = HashSet::new();
        for node in tree[SurfaceIndex::main()].iter() {
            if let Node::Leaf { tabs, .. } = node {
                for tab in tabs {open_tabs.insert(tab.clone());}
            }
        }

        let _backend = RataguiBackend::new_with_fonts(
            10,
            10,
            "Regular".into(),
            "Bold".into(),
            "Oblique".into(),
            "BoldOblique".into(),
        );

        let (tx, rx) = crossbeam::channel::bounded::<String>(1);
        let tx_scaffold = tx.clone();
        let (db_data_sender, db_data_receiver) = crossbeam::channel::unbounded::<Vec<TaskPayload>>();
        let (prestashop_api_tx, prestashop_api_rx) = crossbeam::channel::unbounded();
        let (computer_specs_tx, computer_specs_rx) = crossbeam::channel::unbounded();
        let (db_tx, db_rx) = crossbeam::channel::unbounded();
        let (cps_keys_tx,cps_keys_rx) = crossbeam::channel::unbounded::<GetKeysResponse>();
        let (app_state_tx,app_state_rx) = crossbeam::channel::unbounded::<AppState>();
        let (connected_clients_tx, connected_clients_rx) = crossbeam::channel::unbounded::<Vec<ConnectedClient>>();
        let (ui_actions_tx, ui_actions_rx) = crossbeam::channel::unbounded::<TaskUiActions>();
        let (store_users_tx,store_users_rx) = crossbeam::channel::unbounded::<Vec<User>>();
        let (initial_tasks_tx, initial_tasks_rx) = crossbeam::channel::unbounded::<Vec<TaskPayload>>();
        let (bytes_tx, bytes_rx) = crossbeam::channel::unbounded::<(u64, u64)>();

        let context = MastertechContext {
            current_user: None,
            // terminal: Terminal::new(backend).unwrap(),
            terminal_frontend: None,

            url: None,
            error: Default::default(),
            frontend: None,

            keys: GetKeysResponse { 
                webroot_key: "Webroot Key".to_string(), 
                superanti_key: "SuperAnti Key".to_string() 
            },

            task_payload: None,
            task_data: LiveTaskPayload::default(),
            computer_data: ComputerData::default(),
            ticket_data: TicketData::default(),
            customer_data: CustomerData::default(),
            task_notes: Vec::new(),

            seb_info: None,

            disks: Value::Array(vec![]),
            disk_num: 0,
            store_users: None,
            scaffold_request: SendRequest{ tx: tx_scaffold },
            client: reqwest::Client::new(),
            file_browser: Arc::new(Mutex::new(FileBrowser::new())),
            current_antivirus: "".to_string(),
            opened_file: None,
            open_file_dialog: None,

            database: None,
            
            ram_test_cbox: scaffold::HardwareTest::RamNotTested,
            hdd_test_cbox: scaffold::HardwareTest::HddNotTested,
            ssd_test_cbox: scaffold::HardwareTest::SsdNotTested,
            minidump_app: MiniDumpApp::default(),
            output_text: "".to_string(),

            client_uuid: None,
            rx,

            task_layouts: HashMap::new(),
            ui_actions_tx, ui_actions_rx,
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

            specs_first_run: true,
            file_browse_run: false,
            query_tasks_first_run: true,
            get_specs: false,
            spinner: false,

            style: None,
            text_color: Color32::from_rgb(255, 204, 230),
            border_stroke_color: Stroke::new(1.0, Color32::from_rgb_additive(150, 62, 124)),

            frame_counter: 0,
            show_deferred_viewport: Arc::new(AtomicBool::new(false)),


            db_data_receiver,  db_data_sender,
            prestashop_api_tx, prestashop_api_rx,
            computer_specs_tx, computer_specs_rx,
            app_state_tx, app_state_rx,
            connected_clients_tx, connected_clients_rx,
            bytes_tx, bytes_rx,
            db_tx, db_rx,
            cps_keys_tx, cps_keys_rx,
            store_users_tx, store_users_rx,
            initial_tasks_tx,  initial_tasks_rx,
            github_issue_title: String::new(),
            github_issue_descript: String::new(),
        };

        Self { context, tree, login: Login::default(), state: AppState::default() }
    }
}

/// Private method to access login state only within NoAuth context
impl MasterTechApp{
    pub fn login_mut(&mut self) -> Option<&mut Login> {
        match self.state{
            AppState::NoAuth(_) => Some(&mut self.login),
            AppState::Authenticated(MainPages::Tasks) => None,
            _ => None
        }
    }
}


impl TabViewer for MastertechContext {
    type Tab = String;

    fn ui(&mut self, ui: &mut Ui, tab: &mut Self::Tab) {

        match tab.as_str() {
            "TUR Sheet" => self.tur_sheet(ui),
            "Console" => self.output_console(ui),
            "Scripts" => self.scripts(ui),
            "File Browser 📂" => self.file_browse(ui),
            "System Information" => self.system_information(ui),
            "Minidump Analysis" => self.mini_dump(ui),
            // "Profiler" => self.puffin_profiler(ui),
            "QC ☑️" => self.quality_check(ui),
            "Tasks" => self.mastertech_website(ui),
            "Bug Tracker" => self.github(ui),
            "Websockets" => self.websockets(ui),
            _ => {
                let sysinfo_tab = &"System Information".to_string();
                if ui.label(tab.as_str()).clicked(){
                    if tab.as_str() == sysinfo_tab{
                        self.specs_first_run = true;
                    }
                };
            }
        }
    }

    fn context_menu(&mut self, ui: &mut Ui, tab: &mut Self::Tab, _surface_index: SurfaceIndex, _node_index: NodeIndex) {
        match tab.as_str() {
            "TUR Sheet" => self.simple_demo_menu(ui),
            "File Browser 📂" => self.file_browser_popup(ui),
            _ => {
                ui.label(tab.to_string());
                ui.label("This is a context menu");
            }
        }
    }
    
    fn title(&mut self, tab: &mut Self::Tab) -> WidgetText {
        tab.as_str().into()
    }
    
    fn on_close(&mut self, tab: &mut Self::Tab) -> bool {
        self.open_tabs.remove(tab);
        true
    }
    
    fn on_add(&mut self, _surface_index: SurfaceIndex, _node_index: NodeIndex) {
        
        // for node in tree[SurfaceIndex::main()].iter() {
        //     if let Node::Leaf { tabs, .. } = node {
        //         for tab in tabs {
        //             open_tabs.insert(tab.clone());
        //         }
        //     }
        // }
        // self.open_tabs.insert(surface_index.);
    }

}


fn setup_custom_fonts(ctx: &Context) {
    // Start with the default fonts (we will be adding to them rather than replacing them).
    let mut fonts = FontDefinitions::default();

    // Install my own font (maybe supporting non-latin characters).
    // .ttf and .otf files supported.
    fonts.font_data.insert(
        "Regular".to_owned(),
        FontData::from_static(include_bytes!("./assets/fonts/Iosevka-Regular.ttf")),
    );
    fonts.families.insert(
        FontFamily::Name("Regular".into()),
        vec!["Regular".to_owned()],
    );
    fonts.font_data.insert(
        "Bold".to_owned(),
        FontData::from_static(include_bytes!("./assets/fonts/Iosevka-Bold.ttf")),
    );
    fonts.families.insert(
        FontFamily::Name("Bold".into()),
        vec!["Bold".to_owned()],
    );

    fonts.font_data.insert(
        "Oblique".to_owned(),
        FontData::from_static(include_bytes!("./assets/fonts/Iosevka-Oblique.ttf")),
    );
    fonts.families.insert(
        FontFamily::Name("Oblique".into()),
        vec!["Oblique".to_owned()],
    );

    fonts.font_data.insert(
        "BoldOblique".to_owned(),
        FontData::from_static(include_bytes!(
            "./assets/fonts/Iosevka-BoldOblique.ttf"
        )),
    );
    fonts.families.insert(
        FontFamily::Name("BoldOblique".into()),
        vec!["BoldOblique".to_owned()],
    );

    // Tell egui to use these fonts:
    ctx.set_fonts(fonts);
}