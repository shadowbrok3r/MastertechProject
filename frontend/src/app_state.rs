use std::{cell::Cell, collections::HashSet, rc::Rc};
use crossbeam::channel::{self, Receiver, Sender};
use egui::{Ui, WidgetText};
use egui_dock::{DockState, Node, NodeIndex, SurfaceIndex, TabViewer};
use gloo_worker::Spawnable;
use log::{error, info};
use ratatui::Terminal;
use ratframe::{NewCC, RataguiBackend};
use serde::Serialize;
use surrealdb::Action;
use wasm_bindgen_futures::spawn_local;
use web_time::{Duration, Instant};
use database::{schema::{User, TaskPayload}, Database};
use mtechserver_two::webworker::WebWorker;
use crate::{pages::login_page::Login, tabs::terminal::chart::App, utilities::displays::{modal::{Modal, ModalHandler}, task_layout::TaskLayout}
//    utilities::get_tasks::{CompletedTasks, MyTasks, StoreTasks}
};

// pub trait LoginState{
//     fn login(&mut self, state: AppState);
//     fn logout(&mut self, state: AppState);
// }

#[derive(Serialize)]
pub struct MtechServer{ // <LoginState>
    #[serde(skip)]
    login: Login,
    pub context: MtechServerContext,
    pub state: AppState,
    #[serde(skip)]
    pub tree: DockState<String>,
}

#[derive(Default, Serialize, Debug)]
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

    /// Widgets / Modals / Ui for portions throughout the app
    pub task_layout: TaskLayout,
    // pub create_task_modal: Modal,
    pub modal_handler: ModalHandler,

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
    pub tasks_rx: Receiver<(Action, TaskPayload)>,
    #[serde(skip)]
    pub my_tasks_rx: Receiver<Vec<TaskPayload>>,
    #[serde(skip)]
    pub store_tasks_rx: Receiver<Vec<TaskPayload>>,
    #[serde(skip)]
    pub completed_tasks_rx: Receiver<Vec<TaskPayload>>,
    #[serde(skip)]
    pub store_users_rx: Receiver<Vec<User>>,


    /// Sends task data over crossbeam channel
    #[serde(skip)]
    pub tasks_tx: Sender<(Action, TaskPayload)>,
    #[serde(skip)]
    pub my_tasks_tx: Sender<Vec<TaskPayload>>,
    #[serde(skip)]
    pub store_tasks_tx: Sender<Vec<TaskPayload>>,
    #[serde(skip)]
    pub completed_tasks_tx: Sender<Vec<TaskPayload>>,
    #[serde(skip)]
    pub store_users_tx: Sender<Vec<User>>,


    /// Receives Database connection over crossbeam channel
    #[serde(skip)]
    pub db_rx: Receiver<Database>,
    /// Sends Database connection over crossbeam channel
    #[serde(skip)]
    pub db_tx: Sender<Database>,


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

        let (state, current_user) = check_authentication(db_tx.clone());

        let modal_handler = ModalHandler::default();
        
        
        
        let context = MtechServerContext{
            open_tabs,
            style: None,
            added_nodes,

            task_layout: TaskLayout::default(),
            modal_handler,
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
            store_tasks_tx, 
            store_tasks_rx,
            completed_tasks_tx, 
            completed_tasks_rx,

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
    db_tx: Sender<Database>
) -> (AppState, Option<User>){
    let cookie = wasm_cookies::get("jwt");
    let user_cookie = wasm_cookies::get("user");

    let mut state = AppState::default();
    let mut current_user = None;
    if let Some(cookie) = cookie{
        match cookie{
            Ok(c) => {
                
                state = AppState::Authenticated;
                // info!("self.state: {:?}", state);
                if let Some(user) = user_cookie{
                    match user{
                        Ok(usr) => {
                            // info!("Got user cookie! {c:?}");
                            let user = serde_json::from_str(&usr.as_str()).unwrap();
                            let db_tx = db_tx.clone();
                            current_user = Some(user);           

                            spawn_local(async move {
                                let database = Database::new(
                                    "".to_string(), 
                                    "".to_string(), 
                                    Some(c)
                                ).await;
                                if let Ok(db) = database{
                                    match db_tx.send(db){
                                        Ok(_) => {
                                            info!("Sent db connection across thread");
                                            drop(db_tx);
                                        },
                                        Err(err) => info!("Error sending db connection: {err:?}"),
                                    }
                                }

                            });
                            state = AppState::Authenticated;
                        },
                        Err(e) => {
                            error!("Error with user cookie: {e:?}");
                            state = AppState::NoAuth;
                        }
                    }
                }      
            },
            Err(e) => {
                error!("Error with cookie: {e:?}");
                state = AppState::NoAuth;
            }
        }
    }
    (state, current_user)
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