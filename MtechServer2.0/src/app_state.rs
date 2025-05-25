use crate::{pages::{account_settings_page::UserPreferences, downloads_page::GithubRelease, login_page::Login, signup_page::Signup}, tabs::github_issue::GithubIssue};
use displays::{app_state::SharedContext, channel_manager::ChannelManager, tabs::admin_console::client_interface::WebSocketClient};
use database::{schema::{prestashop_schema::PrestashopPayload, TaskPayload, UserSettings}, Database, WS_MASTER_URL};
use egui_dock::{DockState, Node, NodeIndex, SurfaceIndex};
use std::collections::{HashMap, HashSet};
use crossbeam::channel::{self, Receiver, Sender};
use eframe::CreationContext;
use serde_json::Value;
use serde::Serialize;
use anyhow::Error;

#[derive(Serialize)]
pub struct MtechServer {
    #[serde(skip)]
    login: Login,
    #[serde(skip)]
    signup: Signup,
    pub account_mod: UserPreferences,
    pub context: MtechServerContext,
    pub state: AppState,
    #[serde(skip)]
    pub tree: DockState<String>,
}

#[derive(Serialize, Default, Debug, PartialEq)]
pub enum MainPages {
    #[default]
    Tasks,
    ChatGpt,
    Downloads,
    WebConsole,
    UserPreferences,
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
    #[serde(skip)]
    pub app_state_tx: Sender<AppState>,
    #[serde(skip)]
    pub app_state_rx: Receiver<AppState>,
    /// {WebSocket clients by ID}
    #[serde(skip)]
    pub ws_clients: HashMap<String, WebSocketClient>,


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
    pub seb_channel: (Sender<Vec<Value>>, Receiver<Vec<Value>>),
    

    // UI and Application State Fields
    /// {Widgets / Modals / Ui for portions throughout the app}
    pub new_note: bool,
    pub search_input: String,
    pub client_search_input: String,
    pub seb_email: String,
    pub client_search_inputs: HashMap<String, String>,
    pub edited_task: TaskPayload,

    /// {Open tabs in the UI}
    pub open_tabs: HashSet<String>,
    #[serde(skip)]
    pub style: Option<egui_dock::Style>,
    #[serde(skip)]
    pub added_nodes: Vec<(SurfaceIndex, NodeIndex)>,

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

    // GitHub Issue Management
    /// {Used to create GitHub issues from the website}
    #[serde(skip)]
    pub github_issue: GithubIssue,
    /// The result of querying github for Mastertech releases
    pub github_releases: Vec<GithubRelease>,


    // // Webworker Communication
    #[serde(skip)]
    pub data_update: std::rc::Rc<
        std::cell::Cell<
            Option<
                Vec<u8>
            >
        >
    >,
    /// The actual communication bridge to / from our dummy worker
    #[serde(skip)]
    pub bridge: gloo_worker::WorkerBridge<crate::webworker::WebWorker>,
    #[serde(skip)]
    pub admin_console_data_helper: AdminConsoleDataHelper,
    /// Do we need to refresh the UI?
    pub refresh: bool,
}

impl MtechServer {
    pub fn new(cc: &CreationContext<'_>) -> Self {
        let tree = default_tree();

        let (db_tx, db_rx) = channel::unbounded();
        let (app_state_tx, app_state_rx) = channel::unbounded::<AppState>();
        let github_releases_channel = <Vec<GithubRelease>>::create_unbounded_channel();
        let bytes_channel = <(Vec<u8>, u64)>::create_unbounded_channel();
        let tur_channel = PrestashopPayload::create_unbounded_channel();
        let seb_channel = <Vec<Value>>::create_unbounded_channel();

        let data_update = std::rc::Rc::new(std::cell::Cell::new(None));
        let sender = data_update.clone();
        let ctx = cc.egui_ctx.clone();

        let bridge = <crate::webworker::WebWorker as gloo_worker::Spawnable>::spawner()
            .callback(move |response| {
                sender.set(Some(response.tasks));
                ctx.request_repaint();
            })
            .spawn("./webworker.js");

        let shared_ctx = SharedContext::new(cc, tree.0.clone());
        let admin_console_data_helper = AdminConsoleDataHelper::new();
        
        let context = MtechServerContext {
            shared_ctx,
            first_run: true,
            bridge,
            data_update,

            // CHANNEL SENDERS / RECEIVERS
            db_tx,
            db_rx,
            app_state_tx,
            app_state_rx,
            github_releases_channel,
            bytes_channel,
            tur_channel,
            seb_channel,

            // MODALS / LAYOUTS
            edited_task: TaskPayload::default(),
            seb_email: String::new(),
            github_issue: GithubIssue::new(),
            github_releases: Vec::new(),
            url: format!("{WS_MASTER_URL}&room_id=0"),
            ws_clients: HashMap::new(),
            undock_client: HashMap::new(),
            wants_to_undock: false,
            error: Default::default(),

            search_input: String::new(),
            client_search_input: String::new(),
            client_search_inputs: HashMap::new(),
            open_tabs: tree.1,
            style: None,
            added_nodes: Vec::new(),
            new_note: false,
            total_download_size: 0.0,
            download_progress: 0.0,
            user_settings: UserSettings::default(),
            update_settings: false,
            get_settings: true,

            refresh: false,
            admin_console_data_helper,
        };

        Self {
            login: Login::default(),
            signup: Signup::default(),
            account_mod: UserPreferences::default(),
            state: AppState::default(),
            context,
            tree: tree.0,
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

    pub fn account_mut(&mut self) -> Option<&mut UserPreferences> {
        match self.state {
            AppState::Authenticated(MainPages::UserPreferences) => Some(&mut self.account_mod),
            _ => None,
        }
    }
}

pub struct AdminConsoleDataHelper {
    pub deser_data_update: std::rc::Rc<std::cell::Cell<Option<Vec<u8>>>>,
    /// The actual communication bridge to / from our dummy worker
    pub deser_bridge: gloo_worker::WorkerBridge<crate::deser_worker::DeserWorker>,
}

impl AdminConsoleDataHelper {
    pub fn new() -> Self {
        let deser_data_update = std::rc::Rc::new(std::cell::Cell::new(None));
        let deser_sender = deser_data_update.clone();
        let deser_bridge = <crate::deser_worker::DeserWorker as gloo_worker::Spawnable>::spawner()
        .callback(move |response| {
            deser_sender.set(Some(response.0));
        })
        .spawn("./deser_worker.js");

        Self {
            deser_data_update,
            deser_bridge,
        }
    }
}

// impl BinMsgHandler for AdminConsoleDataHelper {
//     fn handle_binary_message(&mut self, bin: &Vec<u8>) -> Vec<u8> {
//         self.deser_bridge.send(crate::deser_worker::Input(bin.clone()));
//         bin.to_vec()
//     }
// }

pub fn default_tree() -> (DockState<String>, HashSet<String>) {
    let mut open_tabs = HashSet::new();
    let mut tree = DockState::new(vec![
        "Store Tasks".to_owned(),
        "Completed Tasks".to_owned(),
        "SEB Lookup".to_owned(),
        "Company Stock".to_owned(),
        // "Customers".to_owned(),
        // "Database Editor".to_owned(),
        "Store Stock".to_owned(),
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
            "Ai".to_owned(),
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

    (tree, open_tabs)
}

#[cfg(target_arch="wasm32")]
pub fn check_authentication(db_tx: Sender<anyhow::Result<Database, Error>>) -> Result<(AppState, Option<database::schema::User>), Error> {
    let cookie = wasm_cookies::get("jwt");
    let user_cookie: Option<Result<String, wasm_cookies::FromUrlEncodingError>> = wasm_cookies::get("user");
    let mut state = AppState::default();
    let mut current_user: Option<database::schema::User> = None;
    if let (Some(cookie), Some(Ok(usr))) = (cookie, user_cookie) {
        use base64::{engine::general_purpose, Engine as _};
        fn decompress_string(input: &[u8]) -> String {
            let mut decompressed = Vec::new();
            let mut decompressor = brotli::Decompressor::new(input, 4096);
            std::io::copy(&mut decompressor, &mut decompressed).unwrap();
            String::from_utf8(decompressed).unwrap()
        }

        
        let decoded = general_purpose::STANDARD.decode(&usr)?;
        let decompressed = decompress_string(&decoded);
        
        current_user = Some(serde_json::from_str(&decompressed)?);
        log::info!("Deompressed data: {}\nDecoded: {}\nOriginal: {}", decompressed.len(), decoded.len(), usr.len());
        
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
    // log::info!("State // user   {:?} // {:?}", state, current_user);
    Ok((state, current_user))
}