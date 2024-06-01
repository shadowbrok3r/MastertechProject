use std::{cell::Cell, collections::{HashMap, HashSet}, rc::Rc};
use anyhow::Error;
use crossbeam::channel::{self, Receiver, Sender};
use egui::{Align2, Context, Ui, WidgetText};
use egui_dock::{DockState, Node, NodeIndex, SurfaceIndex, TabViewer};
use egui_toast::Toasts;
use gloo_worker::Spawnable;
use log::info;
use ratatui::Terminal;
use ratframe::{NewCC, RataguiBackend};
use serde::Serialize;
use serde_json::Value;
use surrealdb::Action;
use wasm_bindgen_futures::spawn_local;
use web_time::{Duration, Instant};
use database::{schema::{TaskPayload, TicketData, TicketPayload, User}, Database};
use mtechserver_two::webworker::WebWorker;
use crate::{pages::login_page::Login, tabs::terminal::chart::App, utilities::{displays::{modals::{Modal, ModalHandler, ModalType, ModalTypes}, task_layout::TaskLayout, Filters}, Task}
};

#[derive(Serialize)]
pub struct MtechServer{
    #[serde(skip)]
    login: Login,
    pub context: MtechServerContext,
    pub state: AppState,
    #[serde(skip)]
    pub tree: DockState<String>,
}

#[derive(Default, Serialize, Debug, PartialEq)]
pub enum AppState{
    Authenticated,
    #[default]
    NoAuth,
}


#[derive(Serialize)]
pub struct MtechServerContext{
    /// collection of all open tabs in ui
    pub open_tabs: HashSet<String>,
    /// egui dock styling
    #[serde(skip)]
    pub style: Option<egui_dock::Style>,
    #[serde(skip)]
    pub added_nodes: Vec<(SurfaceIndex, NodeIndex)>,

    #[serde(skip)]
    pub toasts: Toasts,

    /// Widgets / Modals / Ui for portions throughout the app
    pub task_layouts: HashMap<String, TaskLayout>,
    // pub create_task_modal: Modal,
    pub modal_handler: ModalHandler,
    pub modal_type: Option<ModalType>,
    pub ticket_map: HashMap<String, TicketPayload>,
    /// Terminal setup for console tab
    #[serde(skip)]
    pub terminal: Terminal<RataguiBackend>,


    /// example chart for console tab
    #[serde(skip)]
    pub chart_app: App,
    /// update period for chart
    pub tick_rate: Duration,
    /// last tick of example chart
    #[serde(skip)]
    pub last_tick: Instant,


    ///Gets data from the first run of the main loop
    pub first_run: bool,
    
    /// Database connection
    #[serde(skip)]
    pub database: Option<Database>,


    pub current_user: Option<User>,

    pub my_tasks_opened: bool,
    pub store_tasks_opened: bool,
    pub completed_tasks_opened: bool,

    /// All contained task data from database
    pub live_tasks: Option<TaskPayload>,
    pub my_tasks: Option<Vec<TaskPayload>>,
    pub store_tasks: Option<Vec<TaskPayload>>,
    pub completed_tasks: Option<Vec<TaskPayload>>,
    pub store_users: Option<Vec<User>>,


    /// Receives task data over crossbeam channel
    #[serde(skip)]
    pub tasks_tx: Sender<(Action, TaskPayload)>,
    #[serde(skip)]
    pub tasks_rx: Receiver<(Action, TaskPayload)>,

    #[serde(skip)]
    pub my_tasks_tx: Sender<Vec<TaskPayload>>,
    #[serde(skip)]
    pub my_tasks_rx: Receiver<Vec<TaskPayload>>,

    #[serde(skip)]
    pub store_users_tx: Sender<Vec<User>>,
    #[serde(skip)]
    pub store_users_rx: Receiver<Vec<User>>,

    #[serde(skip)]
    pub ticket_data_tx: Sender<Option<Value>>,
    #[serde(skip)]
    pub ticket_data_rx: Receiver<Option<Value>>,

    /// Receives Database connection over crossbeam channel
    #[serde(skip)]
    pub db_rx: Receiver<anyhow::Result<Database, Error>>,
    /// Sends Database connection over crossbeam channel
    #[serde(skip)]
    pub db_tx:  Sender<anyhow::Result<Database, Error>>,


    #[serde(skip)]
    pub bridge: Option<gloo_worker::WorkerBridge<WebWorker>>,
    pub data_update: Option<Rc<Cell<Option<u32>>>>,
}

impl TabViewer for MtechServerContext {
    type Tab = String;

    fn ui(&mut self, ui: &mut Ui, tab: &mut Self::Tab) {

        match tab.as_str() {
            "Lil menu" => self.simple_demo_menu(ui),
            "Terminal" => self.terminal(ui),
            "Store Tasks" => self.store_tasks(ui),
            "My Tasks" => self.my_tasks(ui),
            "Web Console" => self.web_console(ui),
            "Completed Tasks" => self.completed_tasks(ui),
            _ => { } 
        }
    }

    fn context_menu(&mut self, ui: &mut Ui, tab: &mut Self::Tab, _surface_index: SurfaceIndex, _node_index: NodeIndex) {
        match tab.as_str() {
            "My Tasks" => self.simple_demo_menu(ui),
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
    
    fn on_add(&mut self, surface_index: SurfaceIndex, node_index: NodeIndex) {
        self.added_nodes.push((surface_index, node_index));
    }

    fn add_popup(&mut self, ui: &mut Ui, _surface_index: SurfaceIndex, _node_index: NodeIndex) {
        ui.set_width(100.0);
        let tabs = &[
            &"Terminal".to_string(),
            &"Web Console".to_string(),
            &"Store Tasks".to_string(),
            &"My Tasks".to_string(),
            &"Completed Tasks".to_string()
        ];

        for tab in tabs{
            if ui.selectable_label(self.open_tabs.contains(*tab), *tab)
                .clicked()
            {
                if !self.open_tabs.contains(*tab){
                    self.on_add(SurfaceIndex::main(), NodeIndex::root());
                }
            }
        }
    }

}


impl NewCC for MtechServer{
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // if let Some(storage) = cc.storage {return eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default();}
        let mut tree = DockState::new(
            vec![
                "My Tasks".to_owned(),
                "Store Tasks".to_owned(),
                "Completed Tasks".to_owned(),
            ]
        );

        tree.translations.tab_context_menu.eject_button = "Undock".to_owned();

        
        // let [_a, _b] = tree
        //     .main_surface_mut()
        //     .split_left(
        //         NodeIndex::root(),
        //         0.10, 
        //         vec![
        //             "Store Tasks".to_owned(),
        // ]);

        let [_a, b] = tree
            .main_surface_mut()
            .split_below(
                NodeIndex::root(),
                0.65, 
                vec![
                    "Terminal".to_owned(),
            ]
        );

        let [_, _] = tree
            .main_surface_mut()
            .split_left(
            b,
            0.45,
            vec!["Web Console".to_owned()],
        );

        // let [_, _] = tree
        //     .main_surface_mut()
        //     .split_left(
        //     b,
        //     0.20,
        //     vec!["Scripts".to_owned()],
        // );


        let mut open_tabs = HashSet::new();

        for node in tree[SurfaceIndex::main()].iter() {
            if let Node::Leaf { tabs, .. } = node {
                for tab in tabs {
                    open_tabs.insert(tab.clone());
                }
            }
        }
        
        
        let backend = RataguiBackend::new_with_fonts(
            10,
            10,
            "Regular".into(),
            "Bold".into(),
            "Oblique".into(),
            "BoldOblique".into(),
        );

        let terminal = Terminal::new(backend).unwrap();
        let tick_rate = Duration::from_millis(30);
        let chart_app = App::new();
        let last_tick = Instant::now();

        let (db_tx, db_rx) = channel::unbounded();
        let (my_tasks_tx, my_tasks_rx) = channel::unbounded::<Vec<TaskPayload>>();
        let (store_tasks_tx, store_tasks_rx) = channel::unbounded::<Vec<TaskPayload>>();
        let (completed_tasks_tx, completed_tasks_rx) = channel::unbounded::<Vec<TaskPayload>>();
        let (store_users_tx,store_users_rx) = channel::unbounded::<Vec<User>>();

        let (tasks_tx, tasks_rx) = channel::unbounded::<(Action, TaskPayload)>();
        let (ticket_data_tx, ticket_data_rx) = channel::unbounded::<Option<Value>>();


        let ctx = cc.egui_ctx.clone();
        let data_update = Rc::new(std::cell::Cell::new(None));
        let sender = data_update.clone();
        let bridge = <WebWorker as Spawnable>::spawner()
            .callback(move |response| {
                sender.set(Some(response.0));
                ctx.request_repaint();
            })
            .spawn("./dummy_worker.js");

        setup_custom_fonts(&cc.egui_ctx);

        let added_nodes = Vec::new();

        let mut state = AppState::default();
        let mut current_user = None;

        match check_authentication(db_tx.clone()){
            Ok(d) => {
                info!("Got auth ok");
                state = d.0;
                current_user = d.1;
            },
            Err(e) => {
                info!("Error with auth: {e:?}");
                state = AppState::NoAuth;
                current_user = None;
            },
        }

        let modal_handler = ModalHandler::default();
        
        let context = MtechServerContext{
            open_tabs,
            style: None,
            added_nodes,

            toasts: Toasts::new().anchor(Align2::RIGHT_TOP, (5.0, 5.0)),


            task_layouts: HashMap::new(),
            modal_handler,
            modal_type: None,
            ticket_map: HashMap::new(),
            terminal,
            chart_app,
            tick_rate,
            last_tick,

            first_run: true,
            database: None,
            db_tx, 
            db_rx,

            current_user,

            live_tasks: None,
            my_tasks: None,
            store_tasks: None,
            completed_tasks: None,
            store_users: None,

            my_tasks_opened: false,
            store_tasks_opened: false,
            completed_tasks_opened: false,

            tasks_tx,
            tasks_rx,
            my_tasks_tx, 
            my_tasks_rx,
            ticket_data_tx,
            ticket_data_rx,

            store_users_tx,
            store_users_rx,

            bridge: Some(bridge),
            data_update: Some(data_update),
        };
        
        Self {
            login: Login::default(),
            context,
            tree,
            state
        }
    }

    fn canvas_id() -> String { "mtech_canvas".into() }
}

impl MtechServerContext{
    pub fn initialize_task_layout(
        &mut self, 
        page: &str, 
        tasks: Vec<TaskPayload>, 
        col_names: Vec<String>, 
        database: Database,
        filters: Vec<Filters>,
        ticket_data_tx: Sender<Option<Value>>
    ) {
        if !self.task_layouts.contains_key(page) {
            let task_layout_opts = TaskLayout::new(
                tasks.to_owned(),
                filters,
                col_names,
                database,
                ticket_data_tx
            );

            self.task_layouts.insert(page.to_string(), task_layout_opts);
        }
    }

    pub fn handle_modals(&mut self, ctx: &Context){
        let modal_type = self.modal_type.clone();
        let db = self.database.clone();
        let task_opt = if let Some(ModalType::TaskModal(ref id)) = modal_type{
            self.find_task_by_id(id).cloned()
        } else { None };
        
        let ticket = if let Some(ModalType::TaskModal(ref id)) = modal_type{
            self.ticket_map.get(id)
        } else { None };
        
        if let Some(mut modal_type) = modal_type {
            match modal_type {
                ModalType::CreateTaskModal => {
                    self.modal_handler.ui(
                        ctx, 
                        || Modal::new("Create Task").default_height(800.0).min_width(700.0).full_span_content(true),
                        move |ui, _stay_open| modal_type.create_task_modal(ui) );
                },
                ModalType::TaskModal(ref id) => {
                    if let Some(ticket) = ticket{
                        if let Some(task) = task_opt {
                            if let Some(db) = db{
                                
                                self.modal_handler.ui(
                                    ctx, 
                                    || Modal::new("Ticket Information").default_height(800.0).min_width(700.0).full_span_content(true),
                                    move |ui, _stay_open| modal_type.task_modal(ui, db, &task, ticket));
                            }
                        }
                    }
                },
                _ => {},
            }
        }
        
        
    }

    pub fn find_task_by_id(&mut self, id: &String) -> Option<&mut TaskPayload> {
        for task_layout in self.task_layouts.values_mut() {
            if let Some(task) = task_layout.tasks.iter_mut().find(|task| task.id.as_ref().map(|t_id| t_id.0.id.to_string()) == Some(id.to_string())) {
                return Some(task);
            }
        }
        None
    }
}

impl MtechServer{
    // Private method to access login state only within NoAuth context
    pub fn login_mut(&mut self) -> Option<&mut Login> {
        match self.state{
            AppState::NoAuth => Some(&mut self.login),
            AppState::Authenticated => None
        }
    }

}

pub fn check_authentication(
    db_tx: Sender<anyhow::Result<Database, Error>>
) -> Result<(AppState, Option<User>), Error>{
    let cookie = wasm_cookies::get("jwt");
    let user_cookie = wasm_cookies::get("user");

    let mut state = AppState::default();
    let mut current_user = None;

    if let Some(cookie) = cookie{

        if let Some(usr) = user_cookie{
            current_user = Some(serde_json::from_str(usr?.as_str())?);
            let db_tx = db_tx.clone();
                    

            spawn_local(async move {
                let database = Database::new(
                    "".to_string(), 
                    "".to_string(), 
                    Some(cookie.unwrap())
                ).await;
                
                match db_tx.send(database){
                    Ok(_) => {
                        info!("Sent DB");
                        drop(db_tx);
                    },
                    Err(err) => info!("Error sending db connection: {err:?}"),
                }
            });
            state = AppState::Authenticated;
        }
    }
    info!("State // user   {:?} // {:?}", state, current_user);
    Ok((state, current_user))
}


fn setup_custom_fonts(ctx: &egui::Context) {
    // Start with the default fonts (we will be adding to them rather than replacing them).
    let mut fonts = egui::FontDefinitions::default();

    // Install my own font (maybe supporting non-latin characters).
    // .ttf and .otf files supported.
    fonts.font_data.insert(
        "Regular".to_owned(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/Iosevka-Regular.ttf")),
    );
    fonts.families.insert(
        egui::FontFamily::Name("Regular".into()),
        vec!["Regular".to_owned()],
    );
    fonts.font_data.insert(
        "Bold".to_owned(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/Iosevka-Bold.ttf")),
    );
    fonts.families.insert(
        egui::FontFamily::Name("Bold".into()),
        vec!["Bold".to_owned()],
    );

    fonts.font_data.insert(
        "Oblique".to_owned(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/Iosevka-Oblique.ttf")),
    );
    fonts.families.insert(
        egui::FontFamily::Name("Oblique".into()),
        vec!["Oblique".to_owned()],
    );

    fonts.font_data.insert(
        "BoldOblique".to_owned(),
        egui::FontData::from_static(include_bytes!(
            "../assets/fonts/Iosevka-BoldOblique.ttf"
        )),
    );
    fonts.families.insert(
        egui::FontFamily::Name("BoldOblique".into()),
        vec!["BoldOblique".to_owned()],
    );

    // Tell egui to use these fonts:
    ctx.set_fonts(fonts);
}