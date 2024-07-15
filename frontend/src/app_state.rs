use std::{cell::Cell, collections::{BTreeMap, HashMap, HashSet}, rc::Rc};
use anyhow::Error;
use crossbeam::channel::{self, Receiver, Sender};
use eframe::{egui::{Align2, Context, FontData, FontDefinitions, FontFamily, Ui, WidgetText}, CreationContext};
use egui_dock::{DockState, Node, NodeIndex, SurfaceIndex, TabViewer};
use crate::{tabs::{ai_playground::AiPlayground, github_issue::GithubIssue}, utilities::{displays::modals::{ChatModalHandler, Modal, TaskModalHandler}, get_data::update_task_notes, ui_tools::toasts::Toasts}};
use gloo_worker::Spawnable;
use log::info;
use ratatui::Terminal;
// use ratframe::NewCC;
use egui_ratatui::RataguiBackend;
use serde::Serialize;
use surrealdb::Action;
use wasm_bindgen_futures::spawn_local;
use web_time::{Duration, Instant};
use database::{schema::{ConnectedClient, LiveTaskPayload, TaskNotePayload, TaskPayload, TicketPayload, User}, Database};
use mtechserver::webworker::{Input, WebWorker};
use crate::{
    pages::{login_page::Login, signup_page::Signup}, tabs::{terminal::chart::App, toolbox::storage_api::FileSystem, web_console::websockets::WebSocketClient}, 
    utilities::{
        displays::{
            chats::ChatView, modals::{create_task_modal::CreateTaskModal, ModalHandler}, tasks::task_layout::TaskLayout
        }, 
        DisplayModal, ModalType,TaskUiActions
    }
};

pub const SECRET_KEY: &str = "lUVgT6KPAR7uPZriAC1QPqSTB9aW12oAmgegk6gO";
pub const ACCESS_KEY: &str = "DMAZwz4511ezKqEiF2vy";

#[derive(Serialize)]
pub struct MtechServer{
    #[serde(skip)]
    login: Login,
    #[serde(skip)]
    signup: Signup,
    pub context: MtechServerContext,
    pub state: AppState,
    #[serde(skip)]
    pub tree: DockState<String>,
}

#[derive(Default, Serialize, Debug, PartialEq)]
pub enum MainPages{
    #[default]
    Tasks,
    ChatGpt,
    Downloads,
    WebConsole,
}

#[derive(Serialize, Debug, PartialEq)]
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

pub struct NewTicketChannel {
    pub new_ticket: TicketPayload,
    pub new_task: (Action, LiveTaskPayload)
}

#[derive(Serialize)]
pub struct MtechServerContext{
    #[serde(skip)]
    pub current_user: Option<User>,
    pub task_map: BTreeMap<String, Vec<TaskPayload>>,
    // pub task: TaskPayload,
    ///Gets data from the first run of the main loop
    pub first_run: bool,

    pub clients: HashMap<String, ConnectedClient>,
    /// Database connection
    // #[serde(skip)]
    // pub database: Option<Database>,

    /// All contained task data from database
    pub live_tasks: Option<LiveTaskPayload>,
    pub tasks: Vec<TaskPayload>,
    pub store_users: Option<Vec<User>>,

    /// Receives task data over crossbeam channel
    #[serde(skip)]
    pub tasks_tx: Sender<(Action, TaskPayload)>,
    #[serde(skip)]
    pub tasks_rx: Receiver<(Action, TaskPayload)>,

    #[serde(skip)]
    pub initial_tasks_tx: Sender<Vec<TaskPayload>>,
    #[serde(skip)]
    pub initial_tasks_rx: Receiver<Vec<TaskPayload>>,

    #[serde(skip)]
    pub live_clients_tx: Sender<(Action, ConnectedClient)>,
    #[serde(skip)]
    pub live_clients_rx: Receiver<(Action, ConnectedClient)>,
    #[serde(skip)]
    pub live_tasks_tx: Sender<(Action, LiveTaskPayload)>,
    #[serde(skip)]
    pub live_tasks_rx: Receiver<(Action, LiveTaskPayload)>,
    #[serde(skip)]
    pub new_ticket_tx: Sender<NewTicketChannel>,
    #[serde(skip)]
    pub new_ticket_rx: Receiver<NewTicketChannel>,
    #[serde(skip)]
    pub notes_tx: Sender<(Action, TaskNotePayload)>,
    #[serde(skip)]
    pub notes_rx: Receiver<(Action, TaskNotePayload)>,

    #[serde(skip)]
    pub store_users_tx: Sender<Vec<User>>,
    #[serde(skip)]
    pub store_users_rx: Receiver<Vec<User>>,

    #[serde(skip)]
    pub app_state_tx: Sender<AppState>,
    #[serde(skip)]
    pub app_state_rx: Receiver<AppState>,

    /// Receives / Sends Database connection over crossbeam channel
    #[serde(skip)]
    pub db_rx: Receiver<anyhow::Result<Database, Error>>,
    #[serde(skip)]
    pub db_tx:  Sender<anyhow::Result<Database, Error>>,
    #[serde(skip)]
    pub ui_actions_tx: Sender<TaskUiActions>,
    #[serde(skip)]
    pub ui_actions_rx: Receiver<TaskUiActions>,
    #[serde(skip)]
    pub connected_clients_tx: Sender<Vec<ConnectedClient>>,
    #[serde(skip)]
    pub connected_clients_rx: Receiver<Vec<ConnectedClient>>,

    #[serde(skip)]
    pub bridge: Option<gloo_worker::WorkerBridge<WebWorker>>,
    #[serde(skip)]
    pub data_update: Option<Rc<Cell<Option<Vec<String>>>>>,
    #[serde(skip)]
    pub file_system: FileSystem,
    #[serde(skip)]
    pub github_issue: GithubIssue,
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
    pub url: String,
    pub error: String,
    #[serde(skip)]
    pub ws_client: Option<WebSocketClient>,
    #[serde(skip)]
    pub text_to_send: String,

    /// Widgets / Modals / Ui for portions throughout the app
    pub new_note: bool,
    pub search_input: String,
    pub edited_task: TaskPayload,
    #[serde(skip)]
    pub task_layouts: HashMap<String, TaskLayout>,
    pub rerun_filtering_my_tasks: bool,
    pub rerun_filtering_store_tasks: bool,
    pub rerun_filtering_completed: bool,
    #[serde(skip)]
    pub ai_playground: AiPlayground,
    #[serde(skip)]
    pub current_modal: ModalType,
    #[serde(skip)]
    pub task_modal_handler: TaskModalHandler,
    pub create_task_modal_handler: ModalHandler<CreateTaskModal>,
    #[serde(skip)]
    pub chat_modal_handler: ChatModalHandler,
    #[serde(skip)]
    pub chat_modal: Option<ChatView>,
    /// collection of all open tabs in ui
    pub open_tabs: HashSet<String>,
    /// egui dock styling
    #[serde(skip)]
    pub style: Option<egui_dock::Style>,
    #[serde(skip)]
    pub added_nodes: Vec<(SurfaceIndex, NodeIndex)>,
    #[serde(skip)]
    pub toasts: Toasts,
}

impl MtechServer{
    pub fn new(cc: &CreationContext<'_>) -> Self {
        // if let Some(storage) = cc.storage {return eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default();}
        setup_custom_fonts(&cc.egui_ctx);

        let mut tree = DockState::new(vec!["Store Tasks".to_owned(),"Completed Tasks".to_owned(), "Quote Fullfilled".to_owned(), 
            "Aging Tasks".to_owned(), "Web Console".to_owned()]);
        let [_a, b] = tree.main_surface_mut().split_below(NodeIndex::root(),0.6, vec!["My Tools".to_owned(), "Bug Report".to_owned()]);
        //"Terminal".to_owned(), 
        let [_, _] = tree.main_surface_mut().split_left(b,0.78,vec!["My Tasks".to_owned(), "Ai Playground".to_owned()]);
        tree.translations.tab_context_menu.eject_button = "Undock".to_owned();
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

        let ctx = cc.egui_ctx.clone();
        let data_update = Rc::new(std::cell::Cell::new(None));
        let sender = data_update.clone();
        let bridge = <WebWorker as Spawnable>::spawner()
            .callback(move |response| {
                sender.set(Some(response.buckets));
                ctx.request_repaint();
            }).spawn("./dummy_worker.js");

        bridge.send(Input {
            url: "https://storage-api.master-tech.app".to_string(),
            access_key: ACCESS_KEY.to_string(),
            secret_key: SECRET_KEY.to_string(),
        });

        let (db_tx, db_rx) = channel::unbounded();
        let (initial_tasks_tx, initial_tasks_rx) = channel::bounded::<Vec<TaskPayload>>(1);
        let (store_users_tx,store_users_rx) = channel::unbounded::<Vec<User>>();
        let (tasks_tx, tasks_rx) = channel::unbounded::<(Action, TaskPayload)>();
        let (app_state_tx,app_state_rx) = channel::unbounded::<AppState>();
        let (live_tasks_tx, live_tasks_rx) = channel::unbounded::<(Action, LiveTaskPayload)>();
        let (live_clients_tx, live_clients_rx) = channel::unbounded::<(Action, ConnectedClient)>();
        let (ui_actions_tx, ui_actions_rx) = channel::unbounded::<TaskUiActions>();
        let (connected_clients_tx, connected_clients_rx) = channel::unbounded::<Vec<ConnectedClient>>();
        let (notes_tx, notes_rx) = channel::unbounded::<(Action, TaskNotePayload)>();
        let (new_ticket_tx, new_ticket_rx) = channel::bounded::<NewTicketChannel>(1);
        
        let mut tasks = Vec::new();
        tasks.push(TaskPayload::default());

        let context = MtechServerContext{
            current_user: None,
            first_run: true,
            clients: HashMap::new(),

            task_map: BTreeMap::new(),
            live_tasks: None,
            tasks,
            store_users: None,

            // CHANNEL SENDERS / RECEIVERS
            db_tx, db_rx,
            live_tasks_tx, live_tasks_rx,
            live_clients_tx, live_clients_rx,
            tasks_tx, tasks_rx,
            initial_tasks_tx,  initial_tasks_rx,
            app_state_tx, app_state_rx,
            store_users_tx, store_users_rx,
            ui_actions_tx, ui_actions_rx,
            connected_clients_tx, connected_clients_rx,
            new_ticket_tx, new_ticket_rx,
            notes_tx, notes_rx,

            // MODALS / LAYOUTS
            ai_playground: AiPlayground::default(),
            edited_task: TaskPayload::default(),
            task_layouts: HashMap::new(),
            rerun_filtering_my_tasks: false,
            rerun_filtering_store_tasks: false,
            rerun_filtering_completed: false,
            current_modal: ModalType::Null,
            task_modal_handler: TaskModalHandler::default(),
            create_task_modal_handler: ModalHandler::default(),
            chat_modal: None,
            chat_modal_handler: ChatModalHandler::default(),

            file_system: FileSystem::new(),
            github_issue: GithubIssue::new(),
            // TERMINAL STUFF
            terminal: Terminal::new(backend).unwrap(),
            tick_rate: Duration::from_millis(30),
            chart_app: App::new(),
            last_tick: Instant::now(),
            // url: format!("{}websocket?room_id=0&role=master", dotenv::var("WS_URL").unwrap()),
            url: "wss://sock.master-tech.app/websocket?room_id=0&role=master".to_string(),
            ws_client: None,
            error: Default::default(),
            // client_layout: None,
            text_to_send: Default::default(),
            // MISC / EVERYTHING ELSE
            bridge: Some(bridge),
            data_update: Some(data_update),
            search_input: String::new(),
            open_tabs,
            style: None,
            added_nodes: Vec::new(),
            new_note: false,
            toasts: Toasts::new().anchor(Align2::RIGHT_TOP, (5.0, 5.0)),
        };
        
        Self { login: Login::default(), signup: Signup::default(), state: AppState::default(), context, tree }
    }

    // fn _canvas_id() -> String { "mtech_canvas".into() }
}

impl MtechServerContext{
    pub fn handle_modals(&mut self, ctx: &Context){
        match &mut self.current_modal {
            ModalType::TaskModal(task_modal) => {
                let task_name = task_modal.task.task_name.clone();
                if let Some(notes) = &task_modal.task.task_note {
                    info!("Notes: {:?}", notes);
                }
                
                self.task_modal_handler.ui(
                    ctx, 
                    || Modal::new(&task_name).default_height(600.0),
                    move |ui, _stay_open, page_state| {
                        let action = task_modal.display(ui, page_state.to_owned());
                        // info!("Modal stuff");
                        // if let Some(notes) = &task_modal.task.task_note{
                        //     info!("Notes: {:?}", notes);
                        // }
                        if let Some(action) = action{
                            *page_state = action;
                        }
                    });
            },
            ModalType::CreateTaskModal(create_task_modal) => {
                let response = self.create_task_modal_handler.ui(
                    ctx, 
                    || CreateTaskModal::new("Create Task", self.store_users.clone()),
                    |ui, _stay_open, page_state| create_task_modal.display(ui, page_state.to_owned()));

                if let Some(response) = response{
                    if let Some(_action) = response{
                        // create_task_modal.set_state(action);
                    }
                }
            },
            ModalType::ChatView(chat_modal) => {
                info!("opening chat");
                self.chat_modal_handler.ui(
                    ctx, 
                    || Modal::new("Chats").default_height(600.0),
                    move |ui, _stay_open, _page_state| {
                        if let Some(_new_message) = chat_modal.ui(ui){
                            spawn_local(async move { });
                            // let _ = update_task_notes(new_message).await;
                            
                        } // task_modal.chat_view.insert_note(payload.1);
                    });
            }
            _ => {},
        }
    }
}

/// Private method to access login state only within NoAuth context
impl MtechServer{
    pub fn login_mut(&mut self) -> Option<&mut Login> {
        match self.state{
            AppState::NoAuth(_) => Some(&mut self.login),
            AppState::Authenticated(MainPages::Tasks) => None,
            _ => None
        }
    }
    pub fn signup_mut(&mut self) -> Option<&mut Signup> {
        match self.state{
            AppState::CreateAccount => Some(&mut self.signup),
            _ => None
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
    if let (Some(cookie), Some(usr)) = (cookie, user_cookie){
        current_user = Some(serde_json::from_str(usr?.as_str())?);
        let db_tx = db_tx.clone();
        spawn_local(async move {
            let database = Database::new(
                "".to_string(), 
                "".to_string(), 
                Some(cookie.unwrap())
            ).await;
            
            match db_tx.try_send(database){
                Ok(_) => {
                    info!("Sent DB");
                    drop(db_tx);
                },
                Err(err) => info!("Error sending db connection: {err:?}"),
            }
        });
        state = AppState::Authenticated(MainPages::Tasks);
    }
    info!("State // user   {:?} // {:?}", state, current_user);
    Ok((state, current_user))
}

impl TabViewer for MtechServerContext {
    type Tab = String;

    fn ui(&mut self, ui: &mut Ui, tab: &mut Self::Tab) {

        match tab.as_str() {
            "Lil menu" => self.simple_demo_menu(ui),
            "Terminal" => self.terminal(ui),
            "My Tools" => self.toolbox(ui),
            "Store Tasks" => self.store_tasks(ui),
            "My Tasks" => self.my_tasks(ui),
            "Ai Playground" => self.ai_playground(ui),
            "Web Console" => self.web_console(ui),
            "Completed Tasks" => self.completed_tasks(ui),
            "Bug Report" => self.github(ui),
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
            &"Bug Report".to_string(),
            &"Terminal".to_string(),
            &"My Tools".to_string(),
            &"Web Console".to_string(),
            &"Store Tasks".to_string(),
            &"My Tasks".to_string(),
            &"Ai Playground".to_string(),
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

fn setup_custom_fonts(ctx: &Context) {
    // Start with the default fonts (we will be adding to them rather than replacing them).
    let mut fonts = FontDefinitions::default();

    fonts.font_data.insert("Monaspace".to_owned(),
   FontData::from_static(include_bytes!("../assets/fonts/MonaspaceNeon-Light.otf"))); // .ttf and .otf supported

    // Put my font first (highest priority):
    fonts.families.get_mut(&FontFamily::Proportional).unwrap()
        .insert(0, "Monaspace".to_owned());

    fonts.font_data.insert(
        "Regular".to_owned(),
        FontData::from_static(include_bytes!("../assets/fonts/MonaspaceNeon-Regular.otf")),
    );
    fonts.families.insert(
        FontFamily::Name("Regular".into()),
        vec!["Regular".to_owned()],
    );
    fonts.font_data.insert(
        "Bold".to_owned(),
        FontData::from_static(include_bytes!("../assets/fonts/MonaspaceNeon-Bold.otf")),
    );
    fonts.families.insert(
        FontFamily::Name("Bold".into()),
        vec!["Bold".to_owned()],
    );

    // Tell egui to use these fonts:
    ctx.set_fonts(fonts);
}