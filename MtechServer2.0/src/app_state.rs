use crate::{
    pages::{account_settings_page::AccountMod, downloads_page::GithubRelease},
    tabs::{ai_playground::AiPlayground, github_issue::GithubIssue, logger::logger_ui},
    utilities::{
        displays::modals::{create_task_modal::Tur, ChatModalHandler, Modal, TaskModalHandler},
        ModalTypes,
    },
};
use crossbeam::channel::{self, Receiver, Sender};
use database::{
    schema::{
        prestashop_schema::PrestashopPayload, ConnectedClient, LiveTaskPayload, Notification,
        TaskNotePayload, TaskPayload, TicketPayload, User,
    },
    Database,
};
use displays::{ui_tools::toasts::Toasts, virtual_filesystem::FileSystem};
use eframe::{
    egui::{Align2, Context, FontData, FontDefinitions, FontFamily, FontId},
    CreationContext,
};
use egui_dock::{DockState, Node, NodeIndex, SurfaceIndex};
use gloo_worker::Spawnable;
use mtechserver::{
    live_worker::{LiveOutput, LiveWorker},
    webworker::WebWorker,
};
use std::{
    cell::Cell,
    collections::{BTreeMap, HashMap, HashSet},
    rc::Rc,
};
use surrealdb::Action;
use wasm_bindgen_futures::spawn_local;
use web_time::{Duration, Instant};
// use ratatui::Terminal;
use crate::{
    pages::{login_page::Login, signup_page::Signup},
    tabs::{terminal::chart::App, web_console::websockets::WebSocketClient},
    utilities::{
        displays::{
            chats::ChatView,
            modals::{create_task_modal::CreateTaskModal, task_modal::ModalAction, ModalHandler},
            tasks::task_layout::TaskLayout,
        },
        DisplayModal, ModalType, TaskUiActions,
    },
};
use anyhow::Error;
use displays::channel_manager::ChannelManager;
use log::{error, info};
use serde::Serialize;

#[derive(Serialize)]
pub struct MtechServer {
    #[serde(skip)]
    login: Login,
    #[serde(skip)]
    signup: Signup,
    pub account_mod: AccountMod,
    pub context: MtechServerContext,
    pub state: AppState,
    #[serde(skip)]
    pub tree: DockState<String>,
}

#[derive(Default, Serialize, Debug, PartialEq)]
pub enum MainPages {
    #[default]
    Tasks,
    ChatGpt,
    Downloads,
    WebConsole,
    AccountSettings,
}

#[derive(Serialize, Debug, PartialEq)]
pub enum AppState {
    Authenticated(MainPages),
    CreateAccount,
    NoAuth(String),
}

impl Default for AppState {
    fn default() -> Self {
        Self::NoAuth("Not Authenticated".to_string())
    }
}

pub struct NewTicketChannel {
    pub new_ticket: TicketPayload,
    pub new_task: (Action, LiveTaskPayload),
}

#[derive(Serialize)]
pub struct MtechServerContext {
    #[serde(skip)]
    pub current_user: Option<User>,
    pub task_map: BTreeMap<String, Vec<TaskPayload>>,
    ///Gets data from the first run of the main loop
    pub first_run: bool,
    pub clients: Vec<ConnectedClient>,
    /// All contained task data from database
    pub live_tasks: Option<LiveTaskPayload>,
    pub tasks: Vec<TaskPayload>,
    pub data_output: LiveOutput,
    pub store_users: Vec<User>,
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
    pub new_note_tx: Sender<TaskNotePayload>,
    #[serde(skip)]
    pub new_note_rx: Receiver<TaskNotePayload>,

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
    pub db_tx: Sender<anyhow::Result<Database, Error>>,
    #[serde(skip)]
    pub ui_actions_tx: Sender<TaskUiActions>,
    #[serde(skip)]
    pub ui_actions_rx: Receiver<TaskUiActions>,
    #[serde(skip)]
    pub connected_clients_tx: Sender<Vec<ConnectedClient>>,
    #[serde(skip)]
    pub connected_clients_rx: Receiver<Vec<ConnectedClient>>,
    #[serde(skip)]
    pub notification_tx: Sender<Vec<Notification>>,
    #[serde(skip)]
    pub notification_rx: Receiver<Vec<Notification>>,
    #[serde(skip)]
    pub live_output_tx: Sender<LiveOutput>,
    #[serde(skip)]
    pub live_output_rx: Receiver<LiveOutput>,
    #[serde(skip)]
    pub github_releases_channel: (Sender<Vec<GithubRelease>>, Receiver<Vec<GithubRelease>>),
    #[serde(skip)]
    pub bytes_channel: (Sender<(Vec<u8>, u64)>, Receiver<(Vec<u8>, u64)>),
    #[serde(skip)]
    pub tur_channel: (Sender<PrestashopPayload>, Receiver<PrestashopPayload>),

    #[serde(skip)]
    pub bridge: Option<gloo_worker::WorkerBridge<WebWorker>>,
    #[serde(skip)]
    pub live_bridge: Option<gloo_worker::WorkerBridge<LiveWorker>>,
    #[serde(skip)]
    pub data_update: Option<Rc<Cell<Option<Vec<String>>>>>,
    #[serde(skip)]
    pub live_data_update: Option<Rc<Cell<Option<LiveOutput>>>>,
    #[serde(skip)]
    pub file_system: FileSystem,
    #[serde(skip)]
    pub github_issue: GithubIssue,

    pub github_releases: Vec<GithubRelease>,
    /// Terminal setup for console tab
    // #[serde(skip)]
    // pub terminal: Terminal<RataguiBackend>,
    pub tur: Tur,
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
    pub ws_clients: HashMap<String, WebSocketClient>,
    pub undock_client: HashMap<String, bool>,
    pub wants_to_undock: bool,
    #[serde(skip)]
    pub text_to_send: String,

    /// Widgets / Modals / Ui for portions throughout the app
    pub new_note: bool,
    pub search_input: String,
    pub client_search_input: String,
    pub client_search_inputs: HashMap<String, String>,
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
    pub notifications: Vec<Notification>,
    pub read_notifications: bool,
    pub total_download_size: f32,
    pub download_progress: f32,
}

impl MtechServer {
    pub fn new(cc: &CreationContext<'_>) -> Self {
        // if let Some(storage) = cc.storage {return eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default();}
        setup_custom_fonts(&cc.egui_ctx);

        let mut tree = DockState::new(vec![
            "Store Tasks".to_owned(),
            "Completed Tasks".to_owned(), //"Quote Fullfilled".to_owned(), "Aging Tasks".to_owned(),
            "Web Console".to_owned(),
            "Customers".to_owned(),
            "Json Viewer".to_owned(),
        ]);
        let [_a, b] = tree.main_surface_mut().split_below(
            NodeIndex::root(),
            0.6,
            vec![
                "My Tools".to_owned(),
                "Bug Report".to_owned(),
                "Logs".to_string(),
            ],
        );
        //"Terminal".to_owned(),
        let [_, _] = tree.main_surface_mut().split_left(
            b,
            0.78,
            vec!["My Tasks".to_owned(), "Ai Playground".to_owned()],
        );
        tree.translations.tab_context_menu.eject_button = "Undock".to_owned();
        let mut open_tabs = HashSet::new();
        for node in tree[SurfaceIndex::main()].iter() {
            if let Node::Leaf { tabs, .. } = node {
                for tab in tabs {
                    open_tabs.insert(tab.clone());
                }
            }
        }

        let ctx = cc.egui_ctx.clone();
        let data_update = Rc::new(std::cell::Cell::new(None));
        let sender = data_update.clone();
        let live_data_update = Rc::new(std::cell::Cell::new(None));
        let live_sender = live_data_update.clone();
        let context = ctx.clone();
        let bridge = <WebWorker as Spawnable>::spawner()
            .callback(move |response| {
                sender.set(Some(response.buckets));
                context.request_repaint();
            })
            .spawn("./dummy_worker.js");

        let live_bridge = <LiveWorker as Spawnable>::spawner()
            .callback(move |response| {
                live_sender.set(Some(response));
                ctx.request_repaint();
            })
            .spawn("./live_worker.js");

        let (db_tx, db_rx) = channel::unbounded();
        let (initial_tasks_tx, initial_tasks_rx) = channel::bounded::<Vec<TaskPayload>>(2);
        let (store_users_tx, store_users_rx) = channel::unbounded::<Vec<User>>();
        let (tasks_tx, tasks_rx) = channel::unbounded::<(Action, TaskPayload)>();
        let (app_state_tx, app_state_rx) = channel::unbounded::<AppState>();
        let (live_tasks_tx, live_tasks_rx) = channel::unbounded::<(Action, LiveTaskPayload)>();
        let (live_clients_tx, live_clients_rx) = channel::unbounded::<(Action, ConnectedClient)>();
        let (ui_actions_tx, ui_actions_rx) = channel::unbounded::<TaskUiActions>();
        let (connected_clients_tx, connected_clients_rx) =
            channel::unbounded::<Vec<ConnectedClient>>();
        let (notes_tx, notes_rx) = channel::unbounded::<(Action, TaskNotePayload)>();
        let (new_ticket_tx, new_ticket_rx) = channel::unbounded::<NewTicketChannel>();
        let (new_note_tx, new_note_rx) = channel::unbounded::<TaskNotePayload>();
        let (notification_tx, notification_rx) = channel::unbounded::<Vec<Notification>>();
        let (live_output_tx, live_output_rx) = channel::unbounded::<LiveOutput>();
        let github_releases_channel = <Vec<GithubRelease>>::create_unbounded_channel();
        let bytes_channel = <(Vec<u8>, u64)>::create_unbounded_channel();
        let tur_channel = PrestashopPayload::create_unbounded_channel();

        let mut tasks = Vec::new();
        tasks.push(TaskPayload::default());

        let context = MtechServerContext {
            current_user: None,
            first_run: true,
            clients: Vec::new(),

            task_map: BTreeMap::new(),
            live_tasks: None,
            tasks,
            data_output: LiveOutput::default(),
            store_users: Vec::new(),

            // CHANNEL SENDERS / RECEIVERS
            db_tx,
            db_rx,
            live_tasks_tx,
            live_tasks_rx,
            live_clients_tx,
            live_clients_rx,
            tasks_tx,
            tasks_rx,
            initial_tasks_tx,
            initial_tasks_rx,
            app_state_tx,
            app_state_rx,
            store_users_tx,
            store_users_rx,
            ui_actions_tx,
            ui_actions_rx,
            connected_clients_tx,
            connected_clients_rx,
            new_ticket_tx,
            new_ticket_rx,
            notes_tx,
            notes_rx,
            new_note_tx,
            new_note_rx,
            notification_tx,
            notification_rx,
            live_output_tx,
            live_output_rx,
            github_releases_channel,
            bytes_channel,
            tur_channel,

            // MODALS / LAYOUTS
            tur: Tur::default(),
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
            github_releases: Vec::new(),
            // TERMINAL STUFF
            // terminal: Terminal::new(backend).unwrap(),
            tick_rate: Duration::from_millis(30),
            chart_app: App::new(),
            last_tick: Instant::now(),
            // url: format!("{}websocket?room_id=0&role=master", dotenv::var("WS_URL").unwrap()),
            url: "wss://sock.master-tech.app/websocket?room_id=0&role=master".to_string(),
            ws_clients: HashMap::new(),
            undock_client: HashMap::new(),
            wants_to_undock: false,
            error: Default::default(),
            // client_layout: None,
            text_to_send: Default::default(),
            // MISC / EVERYTHING ELSE
            bridge: Some(bridge),
            live_bridge: Some(live_bridge),
            live_data_update: Some(live_data_update),
            data_update: Some(data_update),
            search_input: String::new(),
            client_search_input: String::new(),
            client_search_inputs: HashMap::new(),
            open_tabs,
            style: None,
            added_nodes: Vec::new(),
            new_note: false,
            toasts: Toasts::new().anchor(Align2::RIGHT_TOP, (5.0, 5.0)),
            notifications: Vec::new(),
            read_notifications: false,
            total_download_size: 0.0,
            download_progress: 0.0,
        };

        Self {
            login: Login::default(),
            signup: Signup::default(),
            account_mod: AccountMod::default(),
            state: AppState::default(),
            context,
            tree,
        }
    }

    pub fn login_mut(&mut self) -> Option<&mut Login> {
        match self.state {
            AppState::NoAuth(_) => Some(&mut self.login),
            AppState::Authenticated(MainPages::Tasks) => None,
            _ => None,
        }
    }

    pub fn signup_mut(&mut self) -> Option<&mut Signup> {
        match self.state {
            AppState::CreateAccount => Some(&mut self.signup),
            _ => None,
        }
    }

    pub fn account_mut(&mut self) -> Option<&mut AccountMod> {
        match self.state {
            AppState::Authenticated(MainPages::AccountSettings) => Some(&mut self.account_mod),
            _ => None,
        }
    }
}

impl MtechServerContext {
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
                            self.store_users.clone(),
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
                        // ui.set_min_size(Vec2::new(600., 600.));
                        // ui.set_max_size(Vec2::new(800., 800.));
                        ui.style_mut().override_font_id = Some(FontId::proportional(13.0));

                        if let Some(_new_message) = chat_modal.ui(ui) {
                            spawn_local(async move {});
                        }
                    },
                );
            }
            _ => {}
        }
    }
}

// #[cfg(target_arch="wasm32")]
pub fn check_authentication(
    db_tx: Sender<anyhow::Result<Database, Error>>,
) -> Result<(AppState, Option<User>), Error> {
    let cookie = wasm_cookies::get("jwt");
    let user_cookie = wasm_cookies::get("user");
    let mut state = AppState::default();
    let mut current_user: Option<User> = None;
    if let (Some(cookie), Some(usr)) = (cookie, user_cookie) {
        current_user = Some(serde_json::from_str(usr?.as_str())?);
        let _user = current_user.clone();
        let db_tx = db_tx.clone();
        spawn_local(async move {
            let database =
                Database::new("".to_string(), "".to_string(), Some(cookie.unwrap())).await;
            match db_tx.try_send(database) {
                Ok(_) => {
                    info!("Sent DB");
                    drop(db_tx);
                }
                Err(err) => error!("sending db connection: {err:?}"),
            }
        });
        state = AppState::Authenticated(MainPages::Tasks);
    }
    info!("State // user   {:?} // {:?}", state, current_user);
    Ok((state, current_user))
}

fn setup_custom_fonts(ctx: &Context) {
    // Start with the default fonts (we will be adding to them rather than replacing them).
    let mut fonts = FontDefinitions::default();

    fonts.font_data.insert(
        "Monaspace".to_owned(),
        FontData::from_static(include_bytes!("../assets/fonts/MonaspaceNeon-Regular.otf")),
    ); // .ttf and .otf supported

    // Put my font first (highest priority):
    fonts
        .families
        .get_mut(&FontFamily::Proportional)
        .unwrap()
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
    fonts
        .families
        .insert(FontFamily::Name("Bold".into()), vec!["Bold".to_owned()]);

    // Tell egui to use these fonts:
    ctx.set_fonts(fonts);
}
