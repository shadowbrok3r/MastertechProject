use std::{collections::HashSet, path::PathBuf, sync::{Mutex, atomic::AtomicBool, Arc}}; 
use chrono::{DateTime, Utc};
use eframe::egui::{Color32, Context, Stroke, Ui, WidgetText};
use serde_json::Value;
use egui_dock::{Node, NodeIndex, SurfaceIndex, DockState, TabViewer};
use uuid::Uuid;
use crate::{database::{database::Database, schema::{ComputerData, LocalSebData, TaskPayload}, GetKeysResponse, PreTicketData}, handle_api::scaffold::{HardwareTest, Salesman, Techs}};
use egui_file::FileDialog;
use crate::{
    filesystem::file_browser::FileBrowser,
    handle_api::{ api_request::SendRequest, scaffold},
    minidump::minidump_main::MiniDumpApp
};

#[cfg(target_os="windows")]


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
            "Profiler" => self.puffin_profiler(ui),
            "QC ☑️" => self.quality_check(ui),
            "Tasks" => self.mastertech_website(ui),
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

pub struct MastertechContext { 
    pub so_number: String,
    pub recommendations: String,

    pub ticket_info: PreTicketData,
    pub keys: GetKeysResponse,

    pub file_browser: Arc<Mutex<FileBrowser>>,
    pub client: reqwest::Client,

    /// Sends requests and retrieves data from scaffold
    pub scaffold_request: SendRequest,

    pub current_antivirus: String,
    pub seb_info: Option<LocalSebData>,
    pub opened_file: Option<PathBuf>,
    pub open_file_dialog: Option<FileDialog>,
    pub minidump_app: MiniDumpApp,

    pub salesman_cbox: Salesman,
    pub techs_cbox: Techs,
    pub ram_test_cbox: HardwareTest, // We just need one of these...
    pub hdd_test_cbox: HardwareTest,
    pub ssd_test_cbox: HardwareTest,

    pub output_text: String,
    
    pub client_uuid: Uuid,
    pub connect_to_ws: bool,
    pub system_info: ComputerData,
    pub disks: Value,
    pub disk_num: usize,

    pub database: Option<Database>,
    pub rx: Option<crossbeam::channel::Receiver<String>>,
    pub ctx: Context,
    pub widget_size: f32,
    pub open_tabs: HashSet<String>,
    pub show_close_buttons: bool,
    pub show_add_buttons: bool,
    pub draggable_tabs: bool,
    pub show_tab_name_on_hover: bool,

    pub date: Option<DateTime<Utc>>,
    
    pub reader_bytes: u32,

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
    pub ticket_data: Option<Vec<TaskPayload>>,

    pub db_data_receiver: crossbeam::channel::Receiver<Vec<TaskPayload>>,
    pub db_data_sender: crossbeam::channel::Sender<Vec<TaskPayload>>,
}

pub struct MasterTechApp {
    pub context: MastertechContext,
    pub tree: DockState<String>,
}

impl Default for MasterTechApp {
    fn default() -> Self {
        let mut tree = DockState::new(
            vec![
                "TUR Sheet".to_owned(), 
                "Minidump Analysis".to_owned(), 
            ]
        );

        tree.translations.tab_context_menu.eject_button = "Undock".to_owned();

        
        let [_a, _b] = tree
            .main_surface_mut()
            .split_left(
                NodeIndex::root(),
                0.30, 
                vec![
                    "File Browser 📂".to_owned(),
        ]);

        let [_a, b] = tree
            .main_surface_mut()
            .split_below(
                NodeIndex::root(),
                0.65, 
                vec![
                    "Console".to_owned(),
                    "Tasks".to_owned()
            ]
        );

        let [_, _] = tree
            .main_surface_mut()
            .split_left(
            b,
            0.45,
            vec!["System Information".to_owned()],
        );

        let [_, _] = tree
            .main_surface_mut()
            .split_left(
            b,
            0.20,
            vec!["Scripts".to_owned()],
        );



        let mut open_tabs = HashSet::new();

        for node in tree[SurfaceIndex::main()].iter() {
            if let Node::Leaf { tabs, .. } = node {
                for tab in tabs {
                    open_tabs.insert(tab.clone());
                }
            }
        }

        // Create watch channel with a default value
        let (tx, rx) = crossbeam::channel::bounded::<String>(1);
        let tx_scaffold = tx.clone();
        let (db_data_sender, db_data_receiver) = crossbeam::channel::unbounded::<Vec<TaskPayload>>();

        let scaffold_request = SendRequest{ tx: tx_scaffold };

        let minidump_app = MiniDumpApp::default();

        let context = MastertechContext {
            so_number: "".to_string(),
            recommendations: "".to_string(),

            ticket_info: PreTicketData::default(),

            keys: GetKeysResponse { 
                webroot_key: "Webroot Key".to_string(), 
                superanti_key: "SuperAnti Key".to_string() 
            },

            seb_info: None,
            system_info: ComputerData::default(),
            disks: Value::Array(vec![]),
            disk_num: 0,

            scaffold_request,
            client: reqwest::Client::new(),
            file_browser: Arc::new(Mutex::new(FileBrowser::new())),
            current_antivirus: "".to_string(),
            opened_file: None,
            open_file_dialog: None,

            database: None,

            salesman_cbox: scaffold::Salesman::Jake, 
            techs_cbox: scaffold::Techs::Logan, 
            
            ram_test_cbox: scaffold::HardwareTest::RamNotTested,
            hdd_test_cbox: scaffold::HardwareTest::HddNotTested,
            ssd_test_cbox: scaffold::HardwareTest::SsdNotTested,
            minidump_app,
            output_text: "".to_string(),

            connect_to_ws: false,
            client_uuid: Uuid::new_v4(),
            rx: Some(rx),

            //////////////////////////////////////////
            /*          Widgets and UI elements     */
            //////////////////////////////////////////
            ctx: Context::default(),
            widget_size: 135.0,
            open_tabs,
            show_close_buttons: true,
            show_add_buttons: true,
            draggable_tabs: true,
            show_tab_name_on_hover: false,
    
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
            ticket_data: None,
            db_data_receiver, 
            db_data_sender
        };

        Self { context, tree }
    }
}