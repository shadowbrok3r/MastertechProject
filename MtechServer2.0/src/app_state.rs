use crate::{
    pages::{login_page::Login, signup_page::Signup, account_settings_page::AccountMod, downloads_page::GithubRelease}, 
    tabs::{
        ai_playground::AiPlayground,
        github_issue::GithubIssue,
        json_viewer::{JsonEditor, JsonEditorState},
        stock::{MyRowData, MyRowViewer, RawStockData, SerialData},
        stock_quantities::{ExtraInventoryData, StockQuantityData, StockQuantityViewer},
        terminal::chart::App, 
        web_console::websockets::WebSocketClient
    }
};
use displays::{
    app_state::SharedContext, channel_manager::ChannelManager, chats::ChatView, egui_data_table::DataTable, modals::{
        create_task_modal::Tur, ModalWindow, task_modal::ModalAction, ModalType
    }, ui_tools::{theme_config::{set_custom_style, ThemeConfig}, toasts::Toasts}, virtual_filesystem::FileSystem
};
use database::{schema::{get_data::NewTicketChannel, prestashop_schema::PrestashopPayload, ConnectedClient, LiveTaskPayload, Notification, TaskNotePayload, TaskPayload, User, UserSettings}, Database};
use eframe::{egui::{Align2, Context, FontData, FontDefinitions, FontFamily, Style}, CreationContext};
use log::info;
use std::{collections::{BTreeMap, HashMap, HashSet},sync::Arc};
use egui_dock::{DockState, Node, NodeIndex, SurfaceIndex};
use crossbeam::channel::{self, Receiver, Sender};
use async_openai_wasm::types::ThreadObject;
use web_time::{Duration, Instant};
use surrealdb::Action;
use serde_json::Value;
use serde::Serialize;
use anyhow::Error;

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

#[derive(Serialize)]
pub struct MtechServerContext {
    pub shared_ctx: SharedContext,
    // User and Client Related Fields
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

    /// {Current UI modal}
    #[serde(skip)]
    pub opened_modals: HashMap<String, ModalType>,
    pub close_modal: Option<String>,
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

        let theme_config = ThemeConfig::default();
        let theme = set_custom_style(&theme_config);


        let context = MtechServerContext {
            shared_ctx: SharedContext::default(),
            first_run: true,
            clients: Vec::new(),

            task_map: BTreeMap::new(),
            live_tasks: None,
            close_modal: None,
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
            opened_modals: HashMap::new(),
            chat_modal: None,
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
        for (title, modal_type) in self.opened_modals.iter_mut() {
            info!("Got a new modal: {title:?}");
            let action = modal_type.ui(ctx, title.clone(), 750., 850.);
            if let Some(action) = action {
                if let ModalAction::Close = action {
                    self.close_modal = Some(title.clone());
                }
            }
        
        }
        if let Some(modal) = &self.close_modal {
            self.opened_modals.remove_entry(modal);
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
        wasm_bindgen_futures::spawn_local(async move {
            let database =
                Database::new("".to_string(), "".to_string(), Some(cookie.unwrap())).await;
            match db_tx.try_send(database) {
                Ok(_) => {
                    log::info!("Sent DB");
                    drop(db_tx);
                }
                Err(err) => log::error!("sending db connection: {err:?}"),
            }
        });
        state = AppState::Authenticated(MainPages::Tasks);
    }
    log::info!("State // user   {:?} // {:?}", state, current_user);
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
