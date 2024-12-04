use std::collections::HashMap;

use anyhow::Error;
use crossbeam::channel::{self, Receiver, Sender};
use database::{schema::{get_data::NewTicketChannel, prestashop_schema::PrestashopPayload, ConnectedClient, LiveTaskPayload, Notification, TaskNotePayload, TaskPayload, User}, Database};
use eframe::egui::Align2;
use serde::Serialize;
use surrealdb::Action;

use crate::{channel_manager::ChannelManager, modals::{create_task_modal::Tur, ModalType}, tasks::task_layout::TaskLayout, ui_tools::toasts::Toasts, TaskUiActions};




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

#[derive(Serialize)]
pub struct SharedContext {
    // User and Client Related Fields
    /// {Sends users from database}
    #[serde(skip)]
    pub store_users_tx: Sender<Vec<User>>,
    /// {Receives users from database}
    #[serde(skip)]
    pub store_users_rx: Receiver<Vec<User>>,
    /// {Connected clients}
    pub clients: Vec<ConnectedClient>,
    /// {Currently logged-in user}
    pub current_user: Option<User>,
    /// {Users in the store}
    pub store_users: Vec<User>,
    /// {Task layouts for different tabs}
    #[serde(skip)]
    pub task_layouts: HashMap<String, TaskLayout>,
    pub rerun_filtering_my_tasks: bool,
    pub rerun_filtering_store_tasks: bool,
    pub rerun_filtering_completed: bool,
    /// {All task data}
    pub tasks: Vec<TaskPayload>,

    /// {Current UI modal}
    #[serde(skip)]
    pub opened_modals: HashMap<String, ModalType>,
    
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
    // #[serde(skip)]
    // pub ws_clients: HashMap<String, WebSocketClient>,

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
    // #[serde(skip)]
    // pub github_releases_channel: (Sender<Vec<GithubRelease>>, Receiver<Vec<GithubRelease>>),
    #[serde(skip)]
    pub bytes_channel: (Sender<(Vec<u8>, u64)>, Receiver<(Vec<u8>, u64)>),
    #[serde(skip)]
    pub tur_channel: (Sender<PrestashopPayload>, Receiver<PrestashopPayload>),
    // #[serde(skip)]
    // pub stock_channel: (Sender<Vec<RawStockData>>, Receiver<Vec<RawStockData>>),
    // #[serde(skip)]
    // pub serial_channel: (Sender<SerialData>, Receiver<SerialData>),
    // #[serde(skip)]
    // pub seb_channel: (Sender<Vec<Value>>, Receiver<Vec<Value>>),
    // #[serde(skip)]
    // pub extra_stock_channel: (
    //     Sender<Vec<ExtraInventoryData>>,
    //     Receiver<Vec<ExtraInventoryData>>,
    // ),
    // #[serde(skip)]
    // pub ai_thread_channel: (Sender<ThreadObject>, Receiver<ThreadObject>),

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
    
    /// {UI actions channel for communication between UI components and main function}
    #[serde(skip)]
    pub ui_actions_tx: Sender<TaskUiActions>,
    /// {UI actions channel for communication between UI components and main function}
    #[serde(skip)]
    pub ui_actions_rx: Receiver<TaskUiActions>,

    #[serde(skip)]
    pub toasts: Toasts,
    pub notifications: Vec<Notification>,
    
    /// store selection for inventory view
    pub store_selection: u64,
    pub read_notifications: bool,
    pub new_note: bool,
    /// tracking for which client we want to undock
    /// into a floating UI when we click the undock button
    pub undock_client: HashMap<String, bool>,
    /// The undock button was clicked for a ConnectedClient
    pub wants_to_undock: bool,
    // Other Components
    pub tur: Tur,
}

impl Default for SharedContext {
    fn default() -> Self {

        // setup_custom_fonts(&cc.egui_ctx);


        // let open_tabs = HashSet::new();
        // let tree = default_tree(open_tabs.clone());


        let (ui_actions_tx, ui_actions_rx) = crossbeam::channel::unbounded::<TaskUiActions>();
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
        // let github_releases_channel = <Vec<GithubRelease>>::create_unbounded_channel();
        let bytes_channel = <(Vec<u8>, u64)>::create_unbounded_channel();
        let tur_channel = PrestashopPayload::create_unbounded_channel();
        // let stock_channel = <Vec<RawStockData>>::create_unbounded_channel();
        // let serial_channel = <SerialData>::create_unbounded_channel();
        // let extra_stock_channel = <Vec<ExtraInventoryData>>::create_unbounded_channel();
        // let seb_channel = <Vec<Value>>::create_unbounded_channel();
        // let ai_thread_channel = <ThreadObject>::create_unbounded_channel();

        // let mut data_viewer = MyRowViewer::default();
        // data_viewer.stock_tx = Some(serial_channel.0.clone());

        // let theme_config = ThemeConfig::default();
        // let theme = set_custom_style(&theme_config);

        Self {
            current_user: None,
            tasks: Vec::new(),
            store_users: Vec::new(),
            ui_actions_tx,
            ui_actions_rx,
            task_layouts: HashMap::new(),
            rerun_filtering_my_tasks: false,
            rerun_filtering_store_tasks: false,
            rerun_filtering_completed: false,
            store_selection: 76,

            toasts: Toasts::new().anchor(Align2::RIGHT_TOP, (5.0, 5.0)),
            notifications: Vec::new(),
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
            // github_releases_channel,
            bytes_channel,
            tur_channel,
            // stock_channel,
            // serial_channel,
            // seb_channel,
            // extra_stock_channel,
            // ai_thread_channel,
            undock_client: HashMap::new(),
            wants_to_undock: false,
            clients: Vec::new(),
            opened_modals: HashMap::new(),
            read_notifications: false,
            new_note: false,
            // ws_clients: HashMap::new(),

                // Other Components
            tur: Tur::default(),
        }
    }
}

// fn setup_custom_fonts(ctx: &Context) {
//     // Start with the default fonts (we will be adding to them rather than replacing them).
//     let mut fonts = FontDefinitions::default();

//     fonts.font_data.insert(
//         "Monaspace".to_owned(),
//         FontData::from_static(include_bytes!("../assets/fonts/MonaspaceNeon-Regular.otf")),
//     ); // .ttf and .otf supported

//     // Put my font first (highest priority):
//     fonts
//         .families
//         .get_mut(&FontFamily::Proportional)
//         .unwrap()
//         .insert(0, "Monaspace".to_owned());

//     fonts.font_data.insert(
//         "Regular".to_owned(),
//         FontData::from_static(include_bytes!("../assets/fonts/MonaspaceNeon-Regular.otf")),
//     );
//     fonts.families.insert(
//         FontFamily::Name("Regular".into()),
//         vec!["Regular".to_owned()],
//     );
//     fonts.font_data.insert(
//         "Bold".to_owned(),
//         FontData::from_static(include_bytes!("../assets/fonts/MonaspaceNeon-Bold.otf")),
//     );
//     fonts
//         .families
//         .insert(FontFamily::Name("Bold".into()), vec!["Bold".to_owned()]);

//     // Tell egui to use these fonts:
//     ctx.set_fonts(fonts);
// }
