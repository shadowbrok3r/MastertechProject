use crate::{channel_manager::ChannelManager, egui_data_table::DataTable, modals::{create_task_modal::Tur, task_modal::ModalAction, ModalType, ModalWindow}, tabs::{ai_playground::AiPlayground, json_viewer::{JsonEditor, JsonEditorState}, stock::{RawStockData, SerialData, SerialsData, SerialsViewer}, stock_quantities::{ExtraInventoryData, StockQuantityData, StockQuantityViewer}, task_audit::TaskAuditViewer}, tasks::task_layout::TaskLayout, ui_tools::{theme_config::{set_custom_style, ThemeConfig}, toasts::Toasts}, viewports::ViewportData, TaskUiActions};
use database::{schema::{get_data::NewTicketChannel, prestashop_schema::PrestashopPayload, ConnectedClient, LiveTaskPayload, Notification, TaskNotePayload, TaskPayload, User}, Database};
use eframe::{egui::{Align2, Context, FontData, FontDefinitions, FontFamily, Style}, CreationContext};
use crossbeam::channel::{self, Receiver, Sender};
use std::{collections::HashMap, sync::Arc};
use surrealdb::{Action, RecordId};
use serde::Serialize;
use anyhow::Error;

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
    #[serde(skip)]
    pub stock_channel: (Sender<Vec<RawStockData>>, Receiver<Vec<RawStockData>>),
    #[serde(skip)]
    pub serial_channel: (Sender<SerialData>, Receiver<SerialData>),
    #[serde(skip)]
    pub extra_stock_channel: (
        Sender<Vec<ExtraInventoryData>>,
        Receiver<Vec<ExtraInventoryData>>,
    ),
    #[serde(skip)]
    pub ai_thread_channel: (Sender<crate::openai::types::ThreadObject>, Receiver<crate::openai::types::ThreadObject>),
    #[serde(skip)]
    pub payload_len_channel: (Sender<u64>, Receiver<u64>),

    // Notifications and App State
    #[serde(skip)]
    pub notification_tx: Sender<Vec<Notification>>,
    #[serde(skip)]
    pub notification_rx: Receiver<Vec<Notification>>,
    #[serde(skip)]
    pub live_notification_tx: Sender<(Action, Notification)>,
    #[serde(skip)]
    pub live_notification_rx: Receiver<(Action, Notification)>,
    
    /// {UI actions channel for communication between UI components and main function}
    #[serde(skip)]
    pub ui_actions_tx: Sender<TaskUiActions>,
    /// {UI actions channel for communication between UI components and main function}
    #[serde(skip)]
    pub ui_actions_rx: Receiver<TaskUiActions>,
    #[serde(skip)]
    pub settings_sender: Sender<ThemeConfig>,
    #[serde(skip)]
    pub settings_receiver: Receiver<ThemeConfig>,

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
    /// Theme settings
    pub theme_config: ThemeConfig,
    /// Button state for modifying theme config
    #[serde(skip)]
    pub modify_theme: bool,
    /// The theme itself
    pub theme: Arc<Style>,
    // Other Components
    pub tur: Tur,
    pub close_modal: Option<String>,

    #[serde(skip)]
    pub json_editor: JsonEditor,
    #[serde(skip)]
    pub json_editor_state: JsonEditorState,
    /// generic data viewer (currently used for inventory tab)
    #[serde(skip)]
    pub serials_viewer: SerialsViewer,
    /// generic data table (currently used for inventory tab)
    #[serde(skip)]
    pub serials_table: DataTable<SerialsData>,
    /// Data viewer for Stock Quantities tab
    #[serde(skip)]
    pub stock_quantity_viewer: StockQuantityViewer,

    /// Data for Stock Quantities tab
    #[serde(skip)]
    pub stock_quantity_table: DataTable<StockQuantityData>,
    #[serde(skip)]
    pub task_audit_table: TaskAuditViewer,

    /// Just some testing for Ai capabilities
    #[serde(skip)]
    pub ai_playground: AiPlayground,
    #[serde(skip)]
    pub show_tasks_viewport: HashMap<RecordId, ViewportData>,
    pub switching_store: bool,
    pub refresh: bool,
    pub payload_len: u64,
}

impl SharedContext {
    pub fn new(cc: &CreationContext<'_>) -> Self {
        setup_custom_fonts(&cc.egui_ctx);

        let (ui_actions_tx, ui_actions_rx) = crossbeam::channel::unbounded::<TaskUiActions>();
        let (db_tx, db_rx) = channel::unbounded();
        let (initial_tasks_tx, initial_tasks_rx) = channel::bounded::<Vec<TaskPayload>>(2);
        let (store_users_tx, store_users_rx) = channel::unbounded::<Vec<User>>();
        let (tasks_tx, tasks_rx) = channel::unbounded::<(Action, TaskPayload)>();
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
        let bytes_channel = <(Vec<u8>, u64)>::create_unbounded_channel();
        let tur_channel = PrestashopPayload::create_unbounded_channel();
        
        let stock_channel = <Vec<RawStockData>>::create_unbounded_channel();
        let serial_channel = <SerialData>::create_unbounded_channel();
        let extra_stock_channel = <Vec<ExtraInventoryData>>::create_unbounded_channel();
        let ai_thread_channel = <crate::openai::types::ThreadObject>::create_unbounded_channel();
        let payload_len_channel = <u64>::create_unbounded_channel();
        let (settings_sender, settings_receiver) = crossbeam::channel::bounded::<ThemeConfig>(1);
        // let github_releases_channel = <Vec<GithubRelease>>::create_unbounded_channel();
        // let seb_channel = <Vec<Value>>::create_unbounded_channel();

        let mut serials_viewer = SerialsViewer::default();
        serials_viewer.stock_tx = Some(serial_channel.0.clone());

        let theme_config = ThemeConfig::default();
        let theme = set_custom_style(&theme_config);
        
        // let mut task_layouts = HashMap::new();
        // task_layouts.insert("CompletedTasks".to_string(), Vec::new());
        // task_layouts.insert("StoreTasks".to_string(), Vec::new());
        // task_layouts.insert("MyTasks".to_string(), Vec::new());
        
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
            payload_len: 0,

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
            settings_sender, settings_receiver,
            bytes_channel,
            tur_channel,
            stock_channel,
            serial_channel,
            extra_stock_channel,
            ai_thread_channel,
            payload_len_channel,
            // github_releases_channel,
            // seb_channel,
            undock_client: HashMap::new(),
            wants_to_undock: false,
            clients: Vec::new(),
            opened_modals: HashMap::new(),
            read_notifications: false,
            new_note: false,
            // ws_clients: HashMap::new(),

            // Other Components
            json_editor: JsonEditor::default(),
            json_editor_state: JsonEditorState::SettingsPage,
            serials_table: DataTable::<SerialsData>::default(),
            serials_viewer,
            stock_quantity_viewer: StockQuantityViewer::default(),
            stock_quantity_table: DataTable::<StockQuantityData>::default(),
            ai_playground: AiPlayground::default(),
            task_audit_table: TaskAuditViewer::new(),
            tur: Tur::default(),
            close_modal: None,
            theme_config,
            theme,
            modify_theme: false,
            show_tasks_viewport: HashMap::new(),
            switching_store: false,
            refresh: false,
        }
    }

    pub fn handle_modals(&mut self, ctx: &Context) {

        for (title, modal_type) in self.opened_modals.iter_mut() {
            // let input = RawInput::default();
            // let full_output = ctx.run(input, |ctx| {
                // info!("Got a new modal: {title:?}");
                let action = modal_type.ui(ctx, title.clone(), 750., 850.);
                if let Some(action) = action {
                    if let ModalAction::Close = action {
                        self.close_modal = Some(title.clone());
                    }
                }
            // });
            // for (x, _y) in full_output.viewport_output.iter() {
            //     info!("Viewport ID: {x:?}");
            // }
        }
        if let Some(modal) = &self.close_modal {
            self.opened_modals.remove_entry(modal);
            self.close_modal = None;
        }
    }
}
fn setup_custom_fonts(ctx: &Context) {
    // Start with the default fonts (we will be adding to them rather than replacing them).
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "Monaspace".to_owned(),
        FontData::from_static(include_bytes!("../../MtechServer2.0/assets/fonts/MonaspaceNeon-Regular.otf")),
    ); // .ttf and .otf supported
    // Put my font first (highest priority):
    fonts
        .families
        .get_mut(&FontFamily::Proportional)
        .unwrap()
        .insert(0, "Monaspace".to_owned());
    fonts.font_data.insert(
        "Regular".to_owned(),
        FontData::from_static(include_bytes!("../../MtechServer2.0/assets/fonts/MonaspaceNeon-Regular.otf")),
    );
    fonts.families.insert(
        FontFamily::Name("Regular".into()),
        vec!["Regular".to_owned()],
    );
    fonts.font_data.insert(
        "Bold".to_owned(),
        FontData::from_static(include_bytes!("../../MtechServer2.0/assets/fonts/MonaspaceNeon-Bold.otf")),
    );
    fonts
        .families
        .insert(FontFamily::Name("Bold".into()), vec!["Bold".to_owned()]);
    // Tell egui to use these fonts:
    ctx.set_fonts(fonts);
}
