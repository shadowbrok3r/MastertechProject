use crate::{
    mtechserver::set_custom_style, pages::{account_settings_page::AccountMod, downloads_page::GithubRelease}, tabs::{
        ai_playground::AiPlayground,
        github_issue::GithubIssue,
        json_viewer::{JsonEditor, JsonEditorState},
        stock::{MyRowData, MyRowViewer, RawStockData, SerialData},
        stock_quantities::{ExtraInventoryData, StockQuantityData, StockQuantityViewer},
    }, utilities::{
        displays::modals::{create_task_modal::Tur, ChatModalHandler, Modal, TaskModalHandler},
        ModalTypes,
    }
};
use async_openai_wasm::types::ThreadObject;
use crossbeam::channel::{self, Receiver, Sender};
use database::{
    schema::{
        prestashop_schema::PrestashopPayload, ConnectedClient, LiveTaskPayload, Notification, TaskNotePayload, TaskPayload, TicketPayload, User, UserSettings
    },
    Database, DATABASE,
};
use displays::{
    egui_data_table::DataTable, ui_tools::toasts::Toasts, virtual_filesystem::FileSystem,
};
use eframe::{
    egui::{scroll_area::ScrollBarVisibility, Align, Align2, Button, Color32, Context, DragValue, FontData, FontDefinitions, FontFamily, FontId, Layout, Rounding, ScrollArea, Stroke, Style, Ui, Vec2, Widget},
    CreationContext,
};
use egui_dock::{DockState, Node, NodeIndex, SurfaceIndex};
use serde_json::Value;
use std::{collections::{BTreeMap, HashMap, HashSet}, sync::Arc};
use surrealdb::Action;
use wasm_bindgen_futures::spawn_local;
use web_time::{Duration, Instant};
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
use log::info;
use serde::{Deserialize, Serialize};

// use gloo_worker::Spawnable;
// use mtechserver::{webworker::WebWorker};
// use ratatui::Terminal;


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
    // User and Client Related Fields
    /// {Currently logged-in user}
    #[serde(skip)]
    pub current_user: Option<User>,
    /// {Users in the store}
    pub store_users: Vec<User>,
    /// {Sends users from database}
    #[serde(skip)]
    pub store_users_tx: Sender<Vec<User>>,
    /// {Receives users from database}
    #[serde(skip)]
    pub store_users_rx: Receiver<Vec<User>>,
    /// {Connected clients}
    pub clients: Vec<ConnectedClient>,
    /// {Transmits connected clients over crossbeam channel}
    #[serde(skip)]
    pub connected_clients_tx: Sender<Vec<ConnectedClient>>,
    #[serde(skip)]
    pub connected_clients_rx: Receiver<Vec<ConnectedClient>>,
    #[serde(skip)]
    pub live_clients_tx: Sender<(Action, ConnectedClient)>,
    #[serde(skip)]
    pub live_clients_rx: Receiver<(Action, ConnectedClient)>,
    /// {WebSocket clients by ID}
    #[serde(skip)]
    pub ws_clients: HashMap<String, WebSocketClient>,

    // Task Related Fields
    /// {Map of tasks by key}
    pub task_map: BTreeMap<String, Vec<TaskPayload>>,
    /// {Live task payload from database}
    pub live_tasks: Option<LiveTaskPayload>,
    /// {All task data}
    pub tasks: Vec<TaskPayload>,
    /// {Task transmission channel over crossbeam}
    #[serde(skip)]
    pub tasks_tx: Sender<(Action, TaskPayload)>,
    #[serde(skip)]
    pub tasks_rx: Receiver<(Action, TaskPayload)>,
    #[serde(skip)]
    pub initial_tasks_tx: Sender<Vec<TaskPayload>>,
    #[serde(skip)]
    pub initial_tasks_rx: Receiver<Vec<TaskPayload>>,
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

    // Communication with other Services
    /// {Database communication channel}
    #[serde(skip)]
    pub db_rx: Receiver<anyhow::Result<Database, Error>>,
    #[serde(skip)]
    pub db_tx: Sender<anyhow::Result<Database, Error>>,
    #[serde(skip)]
    pub github_releases_channel: (Sender<Vec<GithubRelease>>, Receiver<Vec<GithubRelease>>),
    #[serde(skip)]
    pub bytes_channel: (Sender<(Vec<u8>, u64)>, Receiver<(Vec<u8>, u64)>),
    #[serde(skip)]
    pub tur_channel: (Sender<PrestashopPayload>, Receiver<PrestashopPayload>),
    #[serde(skip)]
    pub stock_channel: (Sender<Vec<RawStockData>>, Receiver<Vec<RawStockData>>),
    #[serde(skip)]
    pub serial_channel: (Sender<SerialData>, Receiver<SerialData>),
    #[serde(skip)]
    pub seb_channel: (Sender<Vec<Value>>, Receiver<Vec<Value>>),
    #[serde(skip)]
    pub extra_stock_channel: (
        Sender<Vec<ExtraInventoryData>>,
        Receiver<Vec<ExtraInventoryData>>,
    ),
    #[serde(skip)]
    pub ai_thread_channel: (Sender<ThreadObject>, Receiver<ThreadObject>),
    

    // UI and Application State Fields
    /// {Widgets / Modals / Ui for portions throughout the app}
    pub new_note: bool,
    pub search_input: String,
    pub client_search_input: String,
    pub seb_email: String,
    pub client_search_inputs: HashMap<String, String>,
    pub edited_task: TaskPayload,
    /// {Task layouts for different tabs}
    #[serde(skip)]
    pub task_layouts: HashMap<String, TaskLayout>,
    pub rerun_filtering_my_tasks: bool,
    pub rerun_filtering_store_tasks: bool,
    pub rerun_filtering_completed: bool,
    /// {Current UI modal}
    #[serde(skip)]
    pub current_modal: ModalType,
    #[serde(skip)]
    pub task_modal_handler: TaskModalHandler,
    pub create_task_modal_handler: ModalHandler<CreateTaskModal>,
    #[serde(skip)]
    pub chat_modal_handler: ChatModalHandler,
    #[serde(skip)]
    pub chat_modal: Option<ChatView>,
    /// {Open tabs in the UI}
    pub open_tabs: HashSet<String>,
    #[serde(skip)]
    pub style: Option<egui_dock::Style>,
    #[serde(skip)]
    pub added_nodes: Vec<(SurfaceIndex, NodeIndex)>,
    #[serde(skip)]
    pub toasts: Toasts,
    pub notifications: Vec<Notification>,
    pub read_notifications: bool,
    #[serde(skip)]
    pub json_editor: JsonEditor,
    #[serde(skip)]
    pub json_editor_state: JsonEditorState,
    /// generic data viewer (currently used for inventory tab)
    #[serde(skip)]
    pub data_viewer: MyRowViewer,
    /// generic data table (currently used for inventory tab)
    #[serde(skip)]
    pub data_table: DataTable<MyRowData>,
    /// store selection for inventory view
    pub store_selection: u64,
    /// Data viewer for Stock Quantities tab
    #[serde(skip)]
    pub stock_quantity_viewer: StockQuantityViewer,
    /// Data for Stock Quantities tab
    #[serde(skip)]
    pub stock_quantity_table: DataTable<StockQuantityData>,

    // Ui State Management Channels
    /// {UI actions channel for communication between UI components and main function}
    #[serde(skip)]
    pub ui_actions_tx: Sender<TaskUiActions>,
    #[serde(skip)]
    pub ui_actions_rx: Receiver<TaskUiActions>,

    // System Data and Settings
    pub user_settings: UserSettings,
    pub update_settings: bool,
    pub get_settings: bool,
    /// {Output log from live operations}

    // Miscellaneous Fields
    /// {Gets data from the first run of the main loop}
    pub first_run: bool,
    /// tracking for which client we want to undock
    /// into a floating UI when we click the undock button
    pub undock_client: HashMap<String, bool>,
    /// The undock button was clicked for a ConnectedClient
    pub wants_to_undock: bool,
    /// URL to use for communication to a ConnectedClient
    /// via a websocket connection
    pub url: String,
    /// Error from our ConnectedClient connection
    pub error: String,
    /// When downloading mastertech from the website
    pub total_download_size: f32,
    /// progress of downloading mastertech
    pub download_progress: f32,

    // Virtual File System
    /// {Virtual file system display}
    #[serde(skip)]
    pub file_system: FileSystem,

    // GitHub Issue Management
    /// {Used to create GitHub issues from the website}
    #[serde(skip)]
    pub github_issue: GithubIssue,
    /// The result of querying github for Mastertech releases
    pub github_releases: Vec<GithubRelease>,

    // Notifications and App State
    #[serde(skip)]
    pub notification_tx: Sender<Vec<Notification>>,
    #[serde(skip)]
    pub notification_rx: Receiver<Vec<Notification>>,
    #[serde(skip)]
    pub live_notification_tx: Sender<(Action, Notification)>,
    #[serde(skip)]
    pub live_notification_rx: Receiver<(Action, Notification)>,
    #[serde(skip)]
    pub app_state_tx: Sender<AppState>,
    #[serde(skip)]
    pub app_state_rx: Receiver<AppState>,

    // // Webworker Communication
    // /// Data from our Dummy Worker
    // #[serde(skip)]
    // pub data_update: Option<Rc<Cell<Option<Vec<String>>>>>,
    // /// The actual communication bridge to / from our dummy worker
    // #[serde(skip)]
    // pub bridge: Option<gloo_worker::WorkerBridge<WebWorker>>,

    // Other Components
    pub tur: Tur,
    /// This is a test chart
    /// for running a TUI in egui
    /// currently not being used
    #[serde(skip)]
    pub chart_app: App,
    /// Tick rate for Chart
    pub tick_rate: Duration,
    /// Track last tick for Chart
    #[serde(skip)]
    pub last_tick: Instant,
    /// Just some testing for Ai capabilities
    #[serde(skip)]
    pub ai_playground: AiPlayground,
    /// Do we need to refresh the UI?
    pub refresh: bool,
    /// Theme settings
    pub theme_config: ThemeConfig,
    /// Button state for modifying theme config
    #[serde(skip)]
    pub modify_theme: bool,
    /// The theme itself
    pub theme: Arc<Style>
}

impl MtechServer {
    pub fn new(cc: &CreationContext<'_>) -> Self {
        // if let Some(storage) = cc.storage {return eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default();}
        setup_custom_fonts(&cc.egui_ctx);

        // let mut tree = DockState::new(vec![
        //     "Store Tasks".to_owned(),
        //     "Completed Tasks".to_owned(),
        //     "Customers".to_owned(),
        //     "Json Viewer".to_owned(),
        //     "Query Builder".to_owned(),
        // ]);

        let open_tabs = HashSet::new();
        let tree = default_tree(open_tabs.clone());

        // if let Some(existing_dock_state) = cc.storage {
        //     if let Some(settings) = existing_dock_state.get_string("user_settings") {
        //         if let Ok(user_settings) = serde_json::from_str::<UserSettings>(&settings) {
        //             info!("Got user settings");
        //             let startup_tabs = user_settings.startup_tabs;
        //             if let Ok(state) = serde_json::from_value::<DockState<String>>(startup_tabs) {
        //                 info!("Got DockState");
        //                 for x in state.iter_all_nodes() {
        //                     info!("All Tabs: {:?}, {:?}", x.1, x.0);
        //                 }
        //                 tree = state;
        //             } else {
        //                 tree = default_tree(open_tabs.clone());
        //             }
        //         } else {
        //             info!("No user settings, using default UI layout");
        //             tree = default_tree(open_tabs.clone());
        //         }
        //     }
        // } else {
        //     info!("No user settings, using default UI layout");
        //     tree = default_tree(open_tabs.clone());
        // }

        // let ctx = cc.egui_ctx.clone();
        // let data_update = Rc::new(std::cell::Cell::new(None));
        // let sender = data_update.clone();
        // let context = ctx.clone();
        // let bridge = <WebWorker as Spawnable>::spawner()
        //     .callback(move |response| {
        //         sender.set(Some(response.buckets));
        //         context.request_repaint();
        //     })
        //     .spawn("./dummy_worker.js");

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
        let (live_notification_tx, live_notification_rx) =
            channel::unbounded::<(Action, Notification)>();
        let (notification_tx, notification_rx) = channel::unbounded::<Vec<Notification>>();
        let github_releases_channel = <Vec<GithubRelease>>::create_unbounded_channel();
        let bytes_channel = <(Vec<u8>, u64)>::create_unbounded_channel();
        let tur_channel = PrestashopPayload::create_unbounded_channel();
        let stock_channel = <Vec<RawStockData>>::create_unbounded_channel();
        let serial_channel = <SerialData>::create_unbounded_channel();
        let extra_stock_channel = <Vec<ExtraInventoryData>>::create_unbounded_channel();
        let seb_channel = <Vec<Value>>::create_unbounded_channel();
        let ai_thread_channel = <ThreadObject>::create_unbounded_channel();

        let mut data_viewer = MyRowViewer::default();
        data_viewer.stock_tx = Some(serial_channel.0.clone());

        let mut tasks = Vec::new();
        tasks.push(TaskPayload::default());
        let theme_config = ThemeConfig::default();
        let theme = set_custom_style(&theme_config);

        let context = MtechServerContext {
            current_user: None,
            first_run: true,
            clients: Vec::new(),

            task_map: BTreeMap::new(),
            live_tasks: None,
            tasks,
            // data_output: LiveOutput::default(),
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
            live_notification_tx,
            live_notification_rx,
            github_releases_channel,
            bytes_channel,
            tur_channel,
            extra_stock_channel,
            seb_channel,
            ai_thread_channel,

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
            seb_email: String::new(),

            file_system: FileSystem::new(),
            github_issue: GithubIssue::new(),
            github_releases: Vec::new(),

            tick_rate: Duration::from_millis(30),
            chart_app: App::new(),
            last_tick: Instant::now(),
            url: "wss://sock.master-tech.app/websocket?room_id=0&role=master".to_string(),
            ws_clients: HashMap::new(),
            undock_client: HashMap::new(),
            wants_to_undock: false,
            error: Default::default(),

            // MISC / EVERYTHING ELSE
            // bridge: Some(bridge),
            // data_update: Some(data_update),
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
            json_editor: JsonEditor::default(),
            json_editor_state: JsonEditorState::SettingsPage,
            user_settings: UserSettings::default(),
            update_settings: false,
            get_settings: false,
            data_table: DataTable::<MyRowData>::default(),
            stock_quantity_viewer: StockQuantityViewer::default(),
            stock_quantity_table: DataTable::<StockQuantityData>::default(),

            data_viewer,
            stock_channel,
            serial_channel,
            store_selection: 76,
            refresh: false,
            theme_config,
            theme,
            modify_theme: false
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

pub fn default_tree(mut open_tabs: HashSet<String>) -> DockState<String> {
    let mut tree = DockState::new(vec![
        "Store Tasks".to_owned(),
        "Completed Tasks".to_owned(),
        "SEB Lookup".to_owned(),
        "Stock Quantity".to_owned(),
        // "Customers".to_owned(),
        // "Json Viewer".to_owned(),
        // "Query Builder".to_owned(),
        "Stock".to_owned(),
        "Logs".to_owned(),
    ]);

    // let [_a, b] =
    //     tree.main_surface_mut()
    //         .split_below(NodeIndex::root(), 0.65, vec!["My Tools".to_owned()]);

    // let [_, _] = tree
    //     .main_surface_mut()
    //     .split_right(b, 0.5, vec!["Bug Report".to_owned()]);

    // "Terminal".to_owned(),

    let [_, _] = tree.main_surface_mut().split_below(// .split_left(
        NodeIndex::root(), // b,
        0.6,
        vec![
            "My Tasks".to_owned(),
            "Bug Report".to_owned(),
            // "Task Audit".to_owned(),
            "Ai Playground".to_owned(),
        ],
    );

    tree.translations.tab_context_menu.eject_button = "Undock".to_owned();

    for node in tree[SurfaceIndex::main()].iter() {
        if let Node::Leaf { tabs, .. } = node {
            for tab in tabs {
                open_tabs.insert(tab.clone());
            }
        }
    }

    tree
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
                            // spawn_local(async move {});
                        }
                    },
                );
            }
            _ => {}
        }
    }
}

#[cfg(target_arch="wasm32")]
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
                Err(err) => log::error!("sending db connection: {err:?}"),
            }
        });
        state = AppState::Authenticated(MainPages::Tasks);
    }
    info!("State // user   {:?} // {:?}", state, current_user);
    Ok((state, current_user))
}

#[derive(Serialize, Clone, Deserialize, Debug, PartialEq)]
pub struct ThemeConfig {
    /// Editor background
    pub background_color: Color32,
    /// Editor foreground
    pub foreground_color: Color32,
    /// Background for inactive widgets
    pub widget_bg_fill: Color32,
    /// Weak background for widgets
    pub widget_weak_bg_fill: Color32,
    /// Widget background stroke color
    pub widget_bg_stroke_color: Color32,
    /// Widget foreground stroke color
    pub widget_fg_stroke_color: Color32,
    /// Background for hovered widgets
    pub hovered_bg_fill: Color32,
    /// Weak background for hovered widgets
    pub hovered_weak_bg_fill: Color32,
    /// Stroke for hovered
    pub hovered_bg_stroke_color: Color32,
    /// Foreground for hovered
    pub hovered_fg_stroke_color: Color32,
    /// Background for active widgets
    pub active_bg_fill: Color32,
    /// Weak background for active widgets
    pub active_weak_bg_fill: Color32,
    /// Stroke for active widgets
    pub active_bg_stroke_color: Color32,
    /// Foreground for active widgets
    pub active_fg_stroke_color: Color32,
    /// Background for open widgets
    pub open_bg_fill: Color32,
    /// Weak background for open widgets
    pub open_weak_bg_fill: Color32,
    /// Stroke for open widgets
    pub open_bg_stroke_color: Color32,
    /// Foreground for open widgets
    pub open_fg_stroke_color: Color32,
    /// Selection background
    pub selection_bg_fill: Color32,
    /// Selection stroke
    pub selection_stroke_color: Color32,
    /// Subtle background
    pub faint_bg_color: Color32,
    /// Very dark background for contrast
    pub extreme_bg_color: Color32,
    /// Code block background
    pub code_bg_color: Color32,
    /// Border color for windows/panels
    pub border_color: Color32,
    /// Default text color
    pub text_color: Color32,
    /// Error text color
    pub error_color: Color32,
    /// Warning text color
    pub warn_color: Color32,
    /// Hyperlink color
    pub link_color: Color32,
    /// Window stroke color
    pub window_stroke_color: Color32,
    /// Uniform rounding for visuals
    pub rounding: Rounding,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            background_color: Color32::from_rgb(10, 10, 13),
            foreground_color: Color32::from_rgb(169, 177, 214),
            widget_bg_fill: Color32::from_rgb(20, 20, 22),
            widget_weak_bg_fill: Color32::from_rgb(20, 20, 22),
            widget_bg_stroke_color: Color32::from_rgb(50, 50, 60),
            widget_fg_stroke_color: Color32::from_rgb(169, 177, 214),
            hovered_bg_fill: Color32::from_rgb(35, 35, 40),
            hovered_weak_bg_fill: Color32::from_rgb(40, 40, 45),
            hovered_bg_stroke_color: Color32::from_rgba_premultiplied(120, 20, 120, 100),
            hovered_fg_stroke_color: Color32::from_rgb(155, 104, 227),
            active_bg_fill: Color32::from_rgb(28, 28, 28),
            active_weak_bg_fill: Color32::from_rgb(28, 28, 28),
            active_bg_stroke_color: Color32::from_rgb(90, 90, 100),
            active_fg_stroke_color: Color32::from_rgb(169, 177, 214),
            open_bg_fill: Color32::from_rgb(30, 30, 35),
            open_weak_bg_fill: Color32::from_rgb(35, 35, 40),
            open_bg_stroke_color: Color32::from_rgb(100, 100, 110),
            open_fg_stroke_color: Color32::from_rgb(169, 177, 214),
            selection_bg_fill: Color32::from_rgba_premultiplied(90, 55, 88, 90),
            selection_stroke_color: Color32::from_rgba_premultiplied(81, 92, 126, 50),
            faint_bg_color: Color32::from_rgb(20, 20, 25),
            extreme_bg_color: Color32::from_rgb(15, 15, 20),
            code_bg_color: Color32::from_rgb(20, 20, 27),
            border_color: Color32::from_rgb(16, 16, 23),
            text_color: Color32::from_rgb(219, 199, 245),
            error_color: Color32::from_rgb(227, 104, 176),
            warn_color: Color32::from_rgb(191, 33, 101),
            link_color: Color32::from_rgb(155, 104, 227),
            window_stroke_color: Color32::from_rgb(42, 195, 222),
            rounding: Rounding::same(4.0),
        }
    }
}

impl ThemeConfig {
    pub fn edit_ui(&mut self, ui: &mut Ui) -> (bool, Self) {
        let mut ret = (false, self.clone());
        ui.horizontal(|ui| {
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                let reset = Button::new("Reset to Default")
                    .min_size(Vec2::new(70., 25.))
                    .stroke(Stroke::new(1., self.warn_color))
                    .ui(ui);
                
                if reset.clicked() {
                    spawn_local(async move {
                        let theme = ThemeConfig::default();
                        match DATABASE 
                            .query("UPDATE $auth.id SET user_settings.color_scheme = $color_settings")
                            .bind(("color_settings", theme.clone()))
                            .await 
                        {
                            Ok(res) => info!("Res: {res:?}"),
                            Err(e) => info!("Error updating User Settings: {e:?}"),
                        }
                    });

                    ret = (true, ThemeConfig::default());
                }
            });
            ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                let save = Button::new("Save")
                    .min_size(Vec2::new(70., 25.))
                    .stroke(Stroke::new(1., self.warn_color))
                    .ui(ui);
                
                if save.clicked() {
                    let color_settings = self.clone();
                    spawn_local(async move {
                        match DATABASE
                            .query("UPDATE $auth.id SET user_settings.color_scheme = $color_settings")
                            .bind(("color_settings", color_settings.clone()))
                            .await {
                                Ok(res) => info!("Result: {res:?}"),
                                Err(e) => info!("Error updating User Settings: {e:?}"),
                            }
                    });
                    ret = (true, self.clone());
                }
            });
        });

        ui.add_space(10.);

        ScrollArea::vertical()
            .scroll_bar_visibility(ScrollBarVisibility::AlwaysVisible)
            .max_height(600.)
            .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.heading("Widget Colors:");
            });

            // Widget Colors
            ui.horizontal(|ui| {
                ui.label("Widget Background Fill"); 
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.widget_bg_fill);
                });
            });

            ui.horizontal(|ui| {
                ui.label("Widget Weak Background Fill");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.widget_weak_bg_fill);
                });
            });

            ui.horizontal(|ui| {
                ui.label("Widget Background Stroke Color");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.widget_bg_stroke_color);
                });
            });
            ui.horizontal(|ui| {
                ui.label("Widget Foreground Stroke Color");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.widget_fg_stroke_color);
                });
            });

            ui.separator();
            ui.add_space(10.);

            // Hovered Colors
            ui.vertical_centered(|ui| {
                ui.heading("Hovered Colors:");
            });

            ui.horizontal(|ui| {   
                ui.label("Hovered Background Fill");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.hovered_bg_fill);
                });
            });

            ui.horizontal(|ui| {
                ui.label("Hovered Weak Background Fill");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.hovered_weak_bg_fill);
                });
            });

            ui.horizontal(|ui| {
                ui.label("Hovered Background Stroke Color");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.hovered_bg_stroke_color);
                });
            });

            ui.horizontal(|ui| {
                ui.label("Hovered Foreground Stroke Color");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.hovered_fg_stroke_color);
                });
            });

            ui.separator();
            ui.add_space(10.);

            // Active Colors
            ui.vertical_centered(|ui| {
                ui.heading("Active Colors:");
            });

            ui.horizontal(|ui| {
                ui.label("Active Background Fill");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.active_bg_fill);
                });
            });

            ui.horizontal(|ui| {
                ui.label("Active Weak Background Fill");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.active_weak_bg_fill);
                });
            });

            ui.horizontal(|ui| {
                ui.label("Active Background Stroke Color");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.active_bg_stroke_color);
                });
            });

            ui.horizontal(|ui| {
                ui.label("Active Foreground Stroke Color");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.active_fg_stroke_color);
                });
            });

            ui.separator();
            ui.add_space(10.);

            // Open Colors
            ui.vertical_centered(|ui| {
                ui.heading("Open Colors:");
            });

            ui.horizontal(|ui| {
                ui.label("Open Background Fill");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.open_bg_fill);
                });
            });

            ui.horizontal(|ui| {
                ui.label("Open Weak Background Fill");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.open_weak_bg_fill);
                });
            });

            ui.horizontal(|ui| {
                ui.label("Open Background Stroke Color");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.open_bg_stroke_color);
                });
            });

            ui.horizontal(|ui| {
                ui.label("Open Foreground Stroke Color");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.open_fg_stroke_color);
                });
            });

            ui.separator();
            ui.add_space(10.);

            // Selection Colors
            ui.vertical_centered(|ui| {
                ui.heading("Selection Colors:");
            });

            ui.horizontal(|ui| {
                ui.label("Selection Background Fill");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.selection_bg_fill);
                });
            });

            ui.horizontal(|ui| {
                ui.label("Selection Stroke Color");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.selection_stroke_color);
                });
            });

            ui.separator();
            ui.add_space(10.);

            // Other Colors
            ui.vertical_centered(|ui| {
                ui.heading("Other Colors:");
            });
            
            ui.horizontal(|ui| {
                ui.label("Background Color");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.background_color);
                });
            });

            ui.horizontal(|ui| {
                ui.label("Foreground Color");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.foreground_color);
                });
            });

            ui.horizontal(|ui| {
                ui.label("Border Color");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.border_color);
                });
            });

            ui.horizontal(|ui| {
                ui.label("Text Color");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.text_color);
                });
            });

            ui.horizontal(|ui| {
                ui.label("Error Color");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.error_color);
                });
            });

            ui.horizontal(|ui| {
                ui.label("Warning Color");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.warn_color);
                });
            });

            ui.horizontal(|ui| {
                ui.label("Link Color");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.link_color);
                });
            });

            ui.horizontal(|ui| {
                ui.label("Faint Background Color");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.faint_bg_color);
                });
            });

            ui.horizontal(|ui| {
                ui.label("Extreme Background Color");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.extreme_bg_color);
                });
            });

            ui.horizontal(|ui| {
                ui.label("Code Background Color");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.code_bg_color);
                });
            });

            ui.separator();
            ui.add_space(10.);

            // Strokes
            ui.vertical_centered(|ui| {
                ui.heading("Strokes:");
            });

            ui.horizontal(|ui| {
                ui.label("Window Stroke:");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.window_stroke_color);
                });
            });

            ui.separator();
            ui.add_space(10.);

            // Rounding
            ui.vertical_centered(|ui| {
                ui.heading("Rounding:");
            });
            
            ui.add(DragValue::new(&mut self.rounding.nw).speed(0.1).prefix("NW:"));
            ui.add(DragValue::new(&mut self.rounding.ne).speed(0.1).prefix("NE:"));
            ui.add(DragValue::new(&mut self.rounding.sw).speed(0.1).prefix("SW:"));
            ui.add(DragValue::new(&mut self.rounding.se).speed(0.1).prefix("SE:"));
        });

        ret
    }
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
