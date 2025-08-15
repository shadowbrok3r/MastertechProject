use crate::{channel_manager::ChannelManager, modals::{create_task_modal::Tur, task_modal::ModalAction, ModalType, ModalWindow}, pages::{account_settings::UserPreferences, login_page::Login, signup_page::Signup}, tabs::{admin_console::AdminConsole, ai_playground::AiPlayground, database_viewer::DatabaseEditor, github::{GithubIssue, GithubRelease}, koth::Koth, presta_order::PrestashopOrderForm, raw_queries::QueryEditor, resource_monitor::ResourceMonitor, scene::SceneEditor, stock::StockTable, task_audit::TaskAuditViewer, tasks::task_layout::{LayoutConfig, TaskLayout}, user_chat::UserChat}, ui_tools::{theme_config::{set_custom_style, ThemeConfig}, toasts::Toasts}, viewports::ViewportData, virtual_filesystem::FileSystem, TaskUiActions};
use database::{schema::{get_data::NewTicketChannel, prestashop_schema::PrestashopPayload, CarboniteResponse, ConnectedClient, LiveTaskPayload, Notification, Status, Store, TaskNotePayload, User}, Database};
use eframe::{egui::{Align2, Context, FontData, FontDefinitions, FontFamily, Style}, CreationContext};
use std::{collections::{BTreeMap, HashMap}, sync::Arc};
use crossbeam::channel::{self, Receiver, Sender};
use surrealdb::{Action, RecordId};
use egui_dock::DockState;
use serde::Serialize;
use anyhow::Error;

#[derive(Serialize, Default, Debug, PartialEq)]
pub enum MainPages {
    #[default]
    Tasks,
    Downloads,
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
pub struct SharedContext {
    pub state: AppState,
    pub tree: DockState<String>,
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
    pub task_layouts: HashMap<String, TaskLayout>,
    /// {All task data}
    pub tasks: Vec<LiveTaskPayload>,

    pub query_editor: QueryEditor,

    #[serde(skip)]
    pub app_state_tx: Sender<AppState>,
    #[serde(skip)]
    pub app_state_rx: Receiver<AppState>,

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
    /// {Task transmission channel over crossbeam}
    #[serde(skip)]
    pub tasks_tx: Sender<(Action, LiveTaskPayload)>,
    #[serde(skip)]
    pub tasks_rx: Receiver<(Action, LiveTaskPayload)>,
    #[serde(skip)]
    pub initial_tasks_tx: Sender<Vec<LiveTaskPayload>>,
    #[serde(skip)]
    pub initial_tasks_rx: Receiver<Vec<LiveTaskPayload>>,
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
    pub bytes_channel: (Sender<(Vec<u8>, u64)>, Receiver<(Vec<u8>, u64)>),
    #[serde(skip)]
    pub tur_channel: (Sender<PrestashopPayload>, Receiver<PrestashopPayload>),
    #[serde(skip)]
    pub ai_thread_channel: (Sender<crate::openai::types::ThreadObject>, Receiver<crate::openai::types::ThreadObject>),
    #[serde(skip)]
    pub seb_channel: (Sender<Vec<CarboniteResponse>>, Receiver<Vec<CarboniteResponse>>),
    #[serde(skip)]
    pub github_releases_channel: (Sender<Vec<GithubRelease>>, Receiver<Vec<GithubRelease>>),
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
    pub live_user_tx: Sender<(Action, User)>,
    #[serde(skip)]
    pub live_user_rx: Receiver<(Action, User)>,
    
    /// {UI actions channel for communication between UI components and main function}
    #[serde(skip)]
    pub ui_actions_tx: Sender<TaskUiActions>,
    /// {UI actions channel for communication between UI components and main function}
    #[serde(skip)]
    pub ui_actions_rx: Receiver<TaskUiActions>,
    #[serde(skip)]
    pub settings_sender: Sender<Style>,
    #[serde(skip)]
    pub settings_receiver: Receiver<Style>,

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
    pub task_audit_table: TaskAuditViewer,
    #[serde(skip)]
    pub database_viewer: DatabaseEditor,
    /// Just some testing for Ai capabilities
    #[serde(skip)]
    pub ai_playground: AiPlayground,
    /// Enhanced AI playground with MCP diagnostic capabilities
    #[cfg(not(target_arch = "wasm32"))]
    #[serde(skip)]
    pub enhanced_ai_playground: crate::tabs::ai_playground::enhanced::EnhancedAiPlayground,
    #[serde(skip)]
    pub show_tasks_viewport: HashMap<RecordId, ViewportData>,
    pub refresh: bool,
    #[serde(skip)]
    pub timer: Option<web_time::Instant>,
    #[serde(skip)]
    pub filesystem: FileSystem,
    #[serde(skip)]
    pub resource_mon: ResourceMonitor,
    #[serde(skip)]
    pub web_console_layout: AdminConsole,
    pub room_id: String,
    #[serde(skip)]
    pub associated_notes_tx: Sender<Vec<TaskNotePayload>>,
    #[serde(skip)]
    pub associated_notes_rx: Receiver<Vec<TaskNotePayload>>,
    #[serde(skip)]
    pub layout_configs: Option<HashMap<String, LayoutConfig>>,
    pub scene_editor: SceneEditor,
    pub user_chat: UserChat,
    pub pending_store: Option<Store>,
    pub task_index: HashMap<String, LiveTaskPayload>, // Index by task ID
    pub search_results: Option<Vec<LiveTaskPayload>>, // Store global search results
    pub account_mod: UserPreferences,
    #[serde(skip)]
    login: Login,
    #[serde(skip)]
    signup: Signup,
    pub first_run: bool,
    // GitHub Issue Management
    /// {Used to create GitHub issues from the website}
    #[serde(skip)]
    pub github_issue: GithubIssue,
    /// The result of querying github for Mastertech releases
    pub github_releases: Vec<GithubRelease>,
    pub notification_modal: Option<Notification>,
    pub admin_notification_text: String,
    #[serde(skip)]
    pub koth: Koth,
    #[serde(skip)]
    pub prestashop_order_form: PrestashopOrderForm,
    #[serde(skip)]
    pub stock_tables: StockTable,
}

impl SharedContext {
    pub fn new(cc: &CreationContext<'_>, tree: DockState<String>) -> Self {
        setup_custom_fonts(&cc.egui_ctx);

        let (ui_actions_tx, ui_actions_rx) = crossbeam::channel::unbounded::<TaskUiActions>();
        let (db_tx, db_rx) = channel::unbounded();
        let (initial_tasks_tx, initial_tasks_rx) = channel::bounded::<Vec<LiveTaskPayload>>(2);
        let (store_users_tx, store_users_rx) = channel::unbounded::<Vec<User>>();
        let (tasks_tx, tasks_rx) = channel::unbounded::<(Action, LiveTaskPayload)>();
        let (live_tasks_tx, live_tasks_rx) = channel::unbounded::<(Action, LiveTaskPayload)>();
        let (live_clients_tx, live_clients_rx) = channel::unbounded::<(Action, ConnectedClient)>();
        let (associated_notes_tx, associated_notes_rx) = channel::unbounded::<Vec<TaskNotePayload>>();
        let (connected_clients_tx, connected_clients_rx) = channel::unbounded::<Vec<ConnectedClient>>();
        let (notes_tx, notes_rx) = channel::unbounded::<(Action, TaskNotePayload)>();
        let (new_ticket_tx, new_ticket_rx) = channel::unbounded::<NewTicketChannel>();
        let (new_note_tx, new_note_rx) = channel::unbounded::<TaskNotePayload>();
        let (live_notification_tx, live_notification_rx) = channel::unbounded::<(Action, Notification)>();
        let (live_user_tx, live_user_rx) = channel::unbounded::<(Action, User)>();
        let (notification_tx, notification_rx) = channel::unbounded::<Vec<Notification>>();
        let bytes_channel = <(Vec<u8>, u64)>::create_unbounded_channel();
        let tur_channel = PrestashopPayload::create_unbounded_channel();
        let ai_thread_channel = <crate::openai::types::ThreadObject>::create_unbounded_channel();
        let (settings_sender, settings_receiver) = crossbeam::channel::bounded::<Style>(1);
        let seb_channel = <Vec<CarboniteResponse>>::create_unbounded_channel();
        let (app_state_tx, app_state_rx) = channel::unbounded::<AppState>();
        let github_releases_channel = <Vec<GithubRelease>>::create_unbounded_channel();
        
        let theme_config = ThemeConfig::default();
        let theme = set_custom_style(&theme_config);
        let web_console_layout = AdminConsole::new(BTreeMap::new(), Vec::new());
        let filesystem = FileSystem::new();

        Self {
            tree,
            prestashop_order_form: PrestashopOrderForm::new(),
            koth: Koth::default(),
            first_run: true,
            notification_modal: None,
            admin_notification_text: Default::default(),
            login: Login::default(),
            signup: Signup::default(),
            state: AppState::default(),
            github_issue: GithubIssue::new(),
            github_releases: Vec::new(),
            account_mod: UserPreferences::default(),
            database_viewer: DatabaseEditor::default(),
            query_editor: QueryEditor::default(),
            github_releases_channel,
            task_index: HashMap::new(),
            layout_configs: None,
            current_user: None,
            tasks: Vec::new(),
            store_users: Vec::new(),
            task_layouts: HashMap::new(),
            store_selection: 76,
            scene_editor: SceneEditor::default(),
            toasts: Toasts::new().anchor(Align2::RIGHT_TOP, (5.0, 5.0)),
            notifications: Vec::new(),
            db_tx, db_rx,
            live_tasks_tx, live_tasks_rx,
            ui_actions_tx, ui_actions_rx,
            live_clients_tx, live_clients_rx,
            tasks_tx, tasks_rx, 
            associated_notes_tx, associated_notes_rx,
            initial_tasks_tx, initial_tasks_rx,
            store_users_tx, store_users_rx, 
            app_state_tx, app_state_rx,
            connected_clients_tx, connected_clients_rx,
            new_ticket_tx, new_ticket_rx,
            notes_tx, notes_rx,
            live_user_tx, live_user_rx,
            new_note_tx, new_note_rx,
            notification_tx, notification_rx,
            live_notification_tx, live_notification_rx,
            settings_sender, settings_receiver,
            bytes_channel,
            tur_channel,
            ai_thread_channel,
            seb_channel,
            undock_client: HashMap::new(),
            wants_to_undock: false,
            clients: Vec::new(),
            opened_modals: HashMap::new(),
            read_notifications: false,
            new_note: false,
            search_results: None,
            stock_tables: StockTable::default(),

            // Other Components
            ai_playground: AiPlayground::default(),
            #[cfg(not(target_arch = "wasm32"))]
            enhanced_ai_playground: crate::tabs::ai_playground::enhanced::EnhancedAiPlayground::default(),
            task_audit_table: TaskAuditViewer::new(),
            resource_mon: ResourceMonitor::default(),
            tur: Tur::default(),
            close_modal: None,
            theme_config,
            theme,
            modify_theme: false,
            show_tasks_viewport: HashMap::new(),
            refresh: false,
            timer: None,
            filesystem,
            web_console_layout,
            room_id: String::new(),
            user_chat: UserChat::default(),
            pending_store: None,
        }
    }

    pub fn account_mut(&mut self) -> Option<&mut UserPreferences> {
        match self.state {
            AppState::Authenticated(MainPages::UserPreferences) => Some(&mut self.account_mod),
            _ => None,
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

    pub fn init_layout_configs(&mut self) {
        if self.layout_configs.is_none() && !self.store_users.is_empty() {
            let mut layout_configs = HashMap::new();

            // Collect all unique statuses from user, store_users, and tasks
            let mut statuses = self
                .current_user
                .as_ref()
                .map(|user| {
                    let user_statuses = user.get_statuses();
                    log::warn!("Current user statuses: {:?}", user_statuses);
                    user_statuses
                })
                .unwrap_or_else(|| {
                    log::warn!("No current user; using default statuses");
                    Status::VALUES.to_vec()
                });
            // Add statuses from store_users
            let store_user_statuses = self.store_users
                .iter()
                .flat_map(|u| u.get_statuses())
                .collect::<std::collections::HashSet<Status>>();

            log::warn!("Store users statuses: {:?}", store_user_statuses);
            
            statuses.extend(store_user_statuses.into_iter());
            // Add statuses from tasks
            let task_statuses = self.task_index
                .values()
                .map(|task| task.status.clone())
                .collect::<std::collections::HashSet<Status>>();
            log::warn!("Task statuses: {:?}", task_statuses);
            statuses.extend(task_statuses.into_iter());

            // MyTasks: Current user's tasks, non-Complete and not completed, keyed by all statuses
            let valid_statuses = {
                log::warn!("Raw statuses: {:?}", statuses);
                let filtered_statuses = statuses
                    .into_iter()
                    .filter(|s| *s != Status::Complete)
                    .map(|s| match s {
                        Status::CustomStatus(name) => {
                            let trimmed = name.trim();
                            if trimmed.is_empty() {
                                log::warn!("Empty custom status in valid_keys; using 'Invalid'");
                                "Invalid".to_string()
                            } else {
                                trimmed.to_string()
                            }
                        }
                        _ => s.as_str().to_string(),
                    })
                    .collect::<std::collections::HashSet<String>>()
                    .into_iter()
                    .collect::<Vec<String>>();
                log::warn!("MyTasks valid_statuses: {:?}", filtered_statuses);
                // If user has a saved order for My Tasks, apply it here
                if let Some(user) = self.current_user.as_ref() {
                    if let Some(saved) = user.get_page_task_columns("My Tasks") {
                        let mut applied: Vec<String> = Vec::new();
                        for k in saved.iter() {
                            if filtered_statuses.contains(k) && !applied.contains(k) {
                                applied.push(k.clone());
                            }
                        }
                        for k in filtered_statuses.iter() {
                            if !applied.contains(k) { applied.push(k.clone()); }
                        }
                        applied
                    } else { filtered_statuses }
                } else { filtered_statuses }
            };

            // MyTasks: Current user's tasks, non-Complete, keyed by status
            layout_configs.insert(
                "My Tasks".to_string(),
                LayoutConfig {
                    valid_keys: valid_statuses.clone(),
                    key_provider: Box::new(move |_| valid_statuses.clone()),
                    filter: Box::new(|task, current_user, _store_users, _store| {
                        current_user
                            .as_ref()
                            .map(|user|{
                                task.assignee == user.get_id() &&
                                task.status != Status::Complete &&
                                !task.completed
                            })
                            .unwrap_or(false)
                    }),
                    update_assignees: false,
                },
            );

            // StoreTasks: Incomplete tasks for store users, keyed by username
            // Build default order of usernames for store tasks
            let mut store_users_default: Vec<String> = self
                .store_users
                .iter()
                .map(|u| u.get_username().to_string())
                .collect();
            // Apply saved order if present
            if let Some(user) = self.current_user.as_ref() {
                if let Some(saved) = user.get_page_task_columns("Store Tasks") {
                    let mut applied: Vec<String> = Vec::new();
                    for k in saved.iter() {
                        if store_users_default.contains(k) && !applied.contains(k) { applied.push(k.clone()); }
                    }
                    for k in store_users_default.iter() {
                        if !applied.contains(k) { applied.push(k.clone()); }
                    }
                    store_users_default = applied;
                }
            }

            layout_configs.insert(
                "Store Tasks".to_string(),
                LayoutConfig {
                    valid_keys: store_users_default.clone(),
                    key_provider: Box::new(move |users| {
                        // Rebuild current usernames but order by saved template (store_users_default)
                        let current: Vec<String> = users.iter().map(|u| u.get_username().to_string()).collect();
                        let mut applied: Vec<String> = Vec::new();
                        for k in store_users_default.iter() {
                            if current.contains(k) && !applied.contains(k) { applied.push(k.clone()); }
                        }
                        for k in current.iter() {
                            if !applied.contains(k) { applied.push(k.clone()); }
                        }
                        applied
                    }),
                    filter: Box::new(|task, current_user, store_users, store| {
                        current_user
                            .as_ref()
                            .map(|current| {
                                store_users.iter().any(|u| {
                                    u.get_store() == *store
                                        && u.get_id() != current.get_id()
                                        && task.assignee != current.get_id()
                                        && task.assignee == u.get_id()
                                        && task.status != Status::Complete
                                        && !task.completed
                                })
                            })
                            .unwrap_or(false)
                    }),
                    update_assignees: true,
                },
            );

            // CompletedTasks: Completed tasks for store users, keyed by username
            // Build default order for Completed Tasks usernames
            let mut completed_users_default: Vec<String> = self
                .store_users
                .iter()
                .map(|u| u.get_username().to_string())
                .collect();
            if let Some(user) = self.current_user.as_ref() {
                if let Some(saved) = user.get_page_task_columns("Completed Tasks") {
                    let mut applied: Vec<String> = Vec::new();
                    for k in saved.iter() {
                        if completed_users_default.contains(k) && !applied.contains(k) { applied.push(k.clone()); }
                    }
                    for k in completed_users_default.iter() {
                        if !applied.contains(k) { applied.push(k.clone()); }
                    }
                    completed_users_default = applied;
                }
            }

            layout_configs.insert(
                "Completed Tasks".to_string(),
                LayoutConfig {
                    valid_keys: completed_users_default.clone(),
                    key_provider: Box::new(move |users| {
                        let current: Vec<String> = users.iter().map(|u| u.get_username().to_string()).collect();
                        let mut applied: Vec<String> = Vec::new();
                        for k in completed_users_default.iter() {
                            if current.contains(k) && !applied.contains(k) { applied.push(k.clone()); }
                        }
                        for k in current.iter() {
                            if !applied.contains(k) { applied.push(k.clone()); }
                        }
                        applied
                    }),
                    filter: Box::new(|task, _current_user, store_users, store| {
                        store_users.iter().any(|u| {
                            u.get_store() == *store && task.assignee == u.get_id() && task.completed && task.status == Status::Complete
                        })
                    }),
                    update_assignees: true,
                },
            );
            self.layout_configs = Some(layout_configs);
        }
    }

    pub fn handle_modals(&mut self, ctx: &Context) {
        for (title, modal_type) in self.opened_modals.iter_mut() {
            let action = modal_type.ui(ctx, title.clone(), 750., 850.);
            if let Some(action) = action {
                if let ModalAction::Close = action {
                    self.close_modal = Some(title.clone());
                }
            }
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
        std::sync::Arc::new(
            FontData::from_static(include_bytes!("../../MtechServer2.0/assets/fonts/MonaspaceNeon-Regular.otf"))
        ),
    );

    fonts.font_data.insert(
        "UbuntuSansMono".to_owned(),
        std::sync::Arc::new(
            FontData::from_static(include_bytes!("../../MtechServer2.0/assets/fonts/UbuntuSansMono-Regular.otf"))
        ),
    );

    fonts.font_data.insert(
        "UbuntuMonoNerdFont".to_owned(),
        std::sync::Arc::new(
            FontData::from_static(include_bytes!("../../MtechServer2.0/assets/fonts/UbuntuMonoNerdFont-Regular.ttf"))
        ),
    ); 
    
    // Put my font first (highest priority):
    fonts
        .families
        .get_mut(&FontFamily::Monospace)
        .unwrap()
        .insert(0, "UbuntuMonoNerdFont".to_owned()); // "Monaspace"

    fonts.font_data.insert(
        "Regular".to_owned(),
        std::sync::Arc::new(
            FontData::from_static(include_bytes!("../../MtechServer2.0/assets/fonts/MonaspaceNeon-Regular.otf"))
        ),
    );
    fonts.families.insert(
        FontFamily::Name("Regular".into()),
        vec!["Regular".to_owned()],
    );
    fonts.font_data.insert(
        "Bold".to_owned(),
        std::sync::Arc::new(
            FontData::from_static(include_bytes!("../../MtechServer2.0/assets/fonts/MonaspaceNeon-Bold.otf"))
        ),
    );
    fonts
        .families
        .insert(FontFamily::Name("Bold".into()), vec!["Bold".to_owned()]);
    // Tell egui to use these fonts:
    ctx.set_fonts(fonts);
}
