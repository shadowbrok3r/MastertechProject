use crate::{channel_manager::ChannelManager, modals::{create_task_modal::Tur, task_modal::ModalAction, ModalType, ModalWindow}, pages::{account_settings::UserPreferences, login_page::Login, signup_page::Signup}, tabs::{admin_console::AdminConsole, database_viewer::DatabaseEditor, dock_session::{default_dock_session_native, default_dock_session_wasm, DockSession}, github::{GithubIssue, GithubRelease}, koth::Koth, presta_order::PrestashopOrderForm, raw_queries::QueryEditor, resource_monitor::ResourceMonitor, sales_tracker::SalesTracker, server_console::ServerConsole, stock::StockTable, stress_lab::StressLab, task_audit::TaskAuditViewer, tasks::task_layout::{LayoutConfig, TaskLayout}, user_chat::UserChat, web_console::WebConsole, TabId}, ui_tools::{notification_center::NotificationCenter, theme_config::{bootstrap_startup_theme, set_custom_style, ThemeConfig}, toasts::Toasts}, viewports::ViewportData, virtual_filesystem::FileSystem, TaskUiActions, Spawner};
use database::{schema::{get_data::NewTicketChannel, prestashop_schema::PrestashopPayload, AiTask, AiTaskItem, CarboniteResponse, ConnectedClient, LiveTaskPayload, Notification, Status, Store, TaskNotePayload, TaskNoteRead, User, UserSettings}, Database};
use eframe::{egui::{Align2, Context, FontData, FontDefinitions, FontFamily, Style}, CreationContext};
use std::{collections::{BTreeMap, HashMap, HashSet}, sync::Arc};
use crossbeam::channel::{self, Receiver, Sender};
use database::{live_data::Action, schema::RecordId};
use egui_dock::NodeIndex;
use serde::{Deserialize, Serialize};
use anyhow::Error;
// `Spawner` (the trait carrying `spawn`) is already pulled in via the
// top-level `use crate::{... Spawner};` above; we just need
// `PlatformSpawner` (the struct) in scope so the Stage-5 apply drain at
// the bottom of `handle_modals` can call `PlatformSpawner::spawn(...)`.
use crate::PlatformSpawner;

/// Lightweight fleet-agent summary fetched from the orchestrator API.
/// Displayed in the Fleet Dashboard tab for warehouse employees.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FleetAgentSummary {
    pub machine_id: String,
    pub agent_version: String,
    pub last_heartbeat: String,
    pub last_report_at: Option<String>,
    pub cpu_avg_pct: f32,
}


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

/// Result of a background reconnect attempt.
#[derive(Debug, Clone)]
pub enum ReconnectOutcome {
    /// Socket usable and `$auth` matches the logged-in user. `socket_was_down`
    /// distinguishes a genuine connection outage (streams died with the
    /// socket; snapshot must be refetched) from a live-but-single-stream
    /// failure (socket was fine; a poisoned/errored stream). `rebuilt` marks
    /// a tier-2 recovery that replaced the whole SurrealDB client.
    Ok { socket_was_down: bool, rebuilt: bool },
    /// Socket usable but `$auth` can't be restored (expired token, no cached
    /// password) — the operator must sign in again.
    AuthLost,
    Failed(String),
}

impl Default for AppState {
    fn default() -> Self {
        Self::NoAuth("Not Authenticated".to_string())
    }
}

#[derive(Serialize)]
pub struct SharedContext {
    pub state: AppState,
    pub dock: DockSession,
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
    /// {Resolved (connection_string, customer) from a client's linked computer when connected_client.customer is null}
    #[serde(skip)]
    pub client_customer_resolved_tx: Sender<Vec<(String, RecordId)>>,
    #[serde(skip)]
    pub client_customer_resolved_rx: Receiver<Vec<(String, RecordId)>>,
    /// {connection_strings with an in-flight customer-resolution query}
    #[serde(skip)]
    pub client_customer_resolving: std::collections::HashSet<String>,
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
    pub seb_channel: (Sender<Vec<CarboniteResponse>>, Receiver<Vec<CarboniteResponse>>),
    #[serde(skip)]
    pub specs_channel: (Sender<database::schema::prestashop::order::ExtractedOrderSpecs>, Receiver<database::schema::prestashop::order::ExtractedOrderSpecs>),
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
    /// {AI task snapshot + live streams}
    #[serde(skip)]
    pub initial_ai_tasks_tx: Sender<(Vec<AiTask>, Vec<AiTaskItem>)>,
    #[serde(skip)]
    pub initial_ai_tasks_rx: Receiver<(Vec<AiTask>, Vec<AiTaskItem>)>,
    #[serde(skip)]
    pub live_ai_tasks_tx: Sender<(Action, AiTask)>,
    #[serde(skip)]
    pub live_ai_tasks_rx: Receiver<(Action, AiTask)>,
    #[serde(skip)]
    pub live_ai_task_items_tx: Sender<(Action, AiTaskItem)>,
    #[serde(skip)]
    pub live_ai_task_items_rx: Receiver<(Action, AiTaskItem)>,

    /// {Live-query stream errors, tagged with the epoch that produced them}
    #[serde(skip)]
    pub live_query_error_tx: Sender<(u64, String)>,
    #[serde(skip)]
    pub live_query_error_rx: Receiver<(u64, String)>,
    /// {A reconnect attempt is pending (failed attempt awaiting backoff)}
    pub needs_reconnect: bool,

    /// Background reconnect tasks post `(attempt_token, outcome)` here; the
    /// next frame drains it on the main thread where `&mut self` is
    /// available. Results from abandoned (stalled) attempts are discarded
    /// by comparing tokens.
    #[serde(skip)]
    pub reconnect_result_tx: Sender<(u64, ReconnectOutcome)>,
    #[serde(skip)]
    pub reconnect_result_rx: Receiver<(u64, ReconnectOutcome)>,
    /// Token identifying the most recently spawned reconnect attempt.
    #[serde(skip)]
    pub reconnect_token: u64,
    /// Gate so a transient-error storm doesn't start five overlapping
    /// reconnect tasks. Cleared in the result drain.
    #[serde(skip)]
    pub reconnect_in_progress: bool,
    /// When the in-flight reconnect attempt started (stall watchdog).
    #[serde(skip)]
    pub reconnect_started_at: Option<web_time::Instant>,
    /// Consecutive reconnect failures; drives the retry backoff. Reset to 0
    /// only on a genuine socket-level recovery (never by the canary, so a
    /// permanently-failing single stream can't defeat the backoff). Also the
    /// tier selector: attempts past `TIER2_AFTER_FAILURES` rebuild the client.
    #[serde(skip)]
    pub reconnect_attempts: u32,
    /// The in-flight attempt is a tier-2 client rebuild (longer stall budget).
    #[serde(skip)]
    pub reconnect_rebuilding: bool,
    /// LIVE registrations confirmed for the current stream generation.
    #[serde(skip)]
    pub live_registered: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    /// Streams spawned in the current generation; the first canary waits for
    /// this many confirmed registrations (or the registration grace window).
    #[serde(skip)]
    pub live_streams_expected: usize,
    /// When the current stream generation was spawned.
    #[serde(skip)]
    pub live_spawned_at: Option<web_time::Instant>,
    /// The chat live streams are running in the current generation.
    #[serde(skip)]
    pub chat_streams_active: bool,
    /// Forces the next `load_data` to refetch tasks/users/notifications.
    #[serde(skip)]
    pub force_data_refetch: bool,
    /// Generation counter for live-query streams; stale-epoch errors are ignored.
    #[serde(skip)]
    pub live_epoch: u64,
    /// Abort handles for the currently-running live-query streams.
    #[serde(skip)]
    pub live_stream_aborts: Vec<futures::future::AbortHandle>,
    /// Random per-app-instance id namespacing this session's canary records.
    #[serde(skip)]
    pub live_session_id: String,
    /// Most recent live-query stream error. Keeps the canary from refilling
    /// the reconnect budget while a stream is still actively failing, so the
    /// backoff can hold at its cap instead of tight-looping.
    #[serde(skip)]
    pub last_stream_error_at: Option<web_time::Instant>,
    /// When the last reconnect-driven snapshot refetch ran. Rate-limits the
    /// socket-healthy re-issue path so a permanently-failing stream can't
    /// refetch the whole store every backoff cycle.
    #[serde(skip)]
    pub last_force_refetch_at: Option<web_time::Instant>,

    /// `document.visibilitychange` signal. `true` = visible, `false` =
    /// hidden. Drained in `receive_shared_logic`: on hidden we stamp
    /// `tab_hidden_at`, on visible we measure the hide duration and only
    /// force a `window.location.reload()` when the tab was hidden for
    /// longer than `LONG_HIDE_AUTO_RELOAD`. Short hides (tab-switching)
    /// no longer trip any reconnect — the only authoritative reconnect
    /// signal is `live_query_error_rx`.
    #[serde(skip)]
    pub visibility_signal_tx: Sender<bool>,
    #[serde(skip)]
    pub visibility_signal_rx: Receiver<bool>,
    /// Wall-clock instant the tab last went `visibility=hidden`. None
    /// until the first hide event. Used by `receive_shared_logic` to
    /// decide whether a return-to-foreground qualifies for the
    /// long-hide auto-reload path.
    #[serde(skip)]
    pub tab_hidden_at: Option<web_time::Instant>,
    /// One-shot guard so the `visibilitychange` listener is only
    /// registered with the DOM once across `first_run` invocations.
    #[serde(skip)]
    pub visibility_listener_installed: bool,
    
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
    
    /// store selection for inventory view
    pub store_selection: u64,
    
    pub new_note: bool,
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
    /// Enhanced AI playground with MCP diagnostic capabilities
    #[cfg(not(target_arch = "wasm32"))]
    #[serde(skip)]
    pub enhanced_ai_playground: crate::tabs::ai_playground::enhanced::EnhancedAiPlayground,
    #[serde(skip)]
    pub show_tasks_viewport: HashMap<RecordId, ViewportData>,
    pub refresh: bool,
    #[serde(skip)]
    pub timer: Option<web_time::Instant>,
    /// Most recent toast text and when it was shown. Used by the toast
    /// consumer in `ui_data::mod` to suppress back-to-back identical
    /// messages (e.g. the admin_transport retry loop firing a "TCP
    /// connect failed" toast every 3 seconds). The window is short — a
    /// genuinely repeating problem will re-toast once the previous
    /// notification has faded.
    #[serde(skip)]
    pub last_toast: Option<(String, web_time::Instant)>,
    /// Admin TCP dial targets whose connect toasts the operator dismissed.
    /// Suppresses re-shows for the rest of the session.
    #[serde(skip)]
    pub dismissed_admin_tcp_targets: HashSet<String>,
    #[serde(skip)]
    pub filesystem: FileSystem,
    #[serde(skip)]
    pub resource_mon: ResourceMonitor,
    #[serde(skip)]
    pub web_console_layout: AdminConsole,
    /// New revamped web console
    #[serde(skip)]
    pub web_console: WebConsole,
    pub room_id: String,
    #[serde(skip)]
    pub associated_notes_tx: Sender<Vec<TaskNotePayload>>,
    #[serde(skip)]
    pub associated_notes_rx: Receiver<Vec<TaskNotePayload>>,
    #[serde(skip)]
    pub layout_configs: Option<HashMap<String, LayoutConfig>>,
    pub user_chat: UserChat,
    pub pending_store: Option<Store>,
    pub task_index: HashMap<String, LiveTaskPayload>, // Index by task ID
    /// AI tasks + checklist items, live-query-fed; the single source of
    /// truth for card, column, and diagnostics-tab checklist rendering.
    #[serde(skip)]
    pub ai_tasks: HashMap<String, AiTask>,
    #[serde(skip)]
    pub ai_task_items: HashMap<String, AiTaskItem>,
    /// Completed AI tasks linger on the tech board for a short grace window.
    #[serde(skip)]
    pub ai_task_done_grace: HashMap<String, web_time::Instant>,
    #[serde(skip)]
    pub ai_popup_queue: std::collections::VecDeque<crate::modals::ai_attention_modal::AiPopup>,
    #[serde(skip)]
    pub ai_popup_modal: Option<crate::modals::ai_attention_modal::AiAttentionModal>,
    pub search_results: Option<Vec<LiveTaskPayload>>, // Store global search results
    pub account_mod: UserPreferences,
    #[serde(skip)]
    login: Login,
    #[serde(skip)]
    signup: Signup,
    pub first_run: bool,
    /// Set after login applies a saved color scheme (or fallback).
    pub user_theme_loaded: bool,
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
    #[serde(skip)]
    pub sales_tracker: SalesTracker,
    #[serde(skip)]
    pub stress_lab: StressLab,
    /// Root-only view of the axum orchestrator's recorded requests.
    #[serde(skip)]
    pub server_console: ServerConsole,
    /// {Widgets / Modals / Ui for portions throughout the app}
    pub search_input: String,
    // Miscellaneous Fields
    pub notification_center: NotificationCenter,
    /// When downloading mastertech from the website
    pub total_download_size: f32,
    /// progress of downloading mastertech
    pub download_progress: f32,
    pub user_settings: UserSettings,
    pub update_settings: bool,
    pub get_settings: bool,
    #[serde(skip)]
    pub added_nodes: Vec<(egui_dock::SurfaceIndex, NodeIndex)>,
    /// Tabs requested to be added from TabViewer::add_popup; applied after DockArea::show
    #[serde(skip)]
    pub pending_tab_adds: Vec<(egui_dock::SurfaceIndex, NodeIndex, TabId)>,
    /// Tabs requested to be removed from TabViewer::add_popup; applied after DockArea::show
    #[serde(skip)]
    pub pending_tab_removes: Vec<TabId>,
    /// Tab to activate after DockArea draws to avoid mutating a stale DockState
    #[serde(skip)]
    pub pending_activate_tab: Option<TabId>,
    /// Tabs to open on the focused leaf after DockArea::show
    #[serde(skip)]
    pub pending_tab_opens: Vec<TabId>,
    /// Tracks when task notes were last read by the current user (task_id -> last_read_at)
    pub last_read_notes: HashMap<RecordId, chrono::DateTime<chrono::Utc>>,
    /// {Read-state rows fetched from SurrealDB on initial load (task_note_read table)}
    #[serde(skip)]
    pub read_state_tx: Sender<Vec<TaskNoteRead>>,
    #[serde(skip)]
    pub read_state_rx: Receiver<Vec<TaskNoteRead>>,
    /// When set, the Admin Console renderer should scroll to / select this
    /// `connection_string`. Cleared by the renderer after acting.
    #[serde(skip)]
    pub pending_admin_console_focus: Option<String>,
    /// Set when the user clicks a suggestion chip on a connected-client
    /// card.  Tuple of `(connection_string, candidate_index)`.  The
    /// Stage-4 confirmation modal reads this to know which suggestion
    /// to render and clears it once the modal is instantiated.
    #[serde(skip)]
    pub pending_open_service_candidate: Option<(String, usize)>,
    /// Live Stage-4 confirmation modal.  At most one open at a time —
    /// keeping it on `SharedContext` rather than inside `opened_modals`
    /// because it doesn't share the `ModalType` enum's lifecycle (it
    /// owns its own egui window via its `.show()` method).
    #[serde(skip)]
    pub open_service_confirm_modal: Option<crate::modals::OpenServiceConfirmModal>,
    /// Staged outcome from the confirmation modal.  When the operator
    /// clicks Confirm, the modal returns
    /// `OpenServiceConfirmOutcome::Confirm(apply)`; we stash the
    /// `apply` payload here so Stage 5's persistence layer can drain
    /// it on the next frame.  Reject outcomes don't populate this — a
    /// confirmed bind is the only thing that needs to persist.
    #[serde(skip)]
    pub pending_open_service_apply:
        Option<crate::modals::OpenServiceConfirmApply>,
    /// Entity-link resolution modal (MCP block or admin manual link).
    /// Desktop + `tokio` only — not built into the wasm bundle.
    #[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
    #[serde(skip)]
    pub entity_link_resolution_modal:
        Option<crate::modals::EntityLinkResolutionModal>,
    /// Connected-client diagnostics popup target (`connection_string`).
    /// `Some` while the popup is open; `None` when closed.
    #[serde(skip)]
    pub client_diagnostics_popup: Option<String>,
    /// Diagnostic sessions fetched for the currently-open client popup.
    /// Cleared and refilled whenever `client_diagnostics_popup` flips to a
    /// new connection_string.
    #[serde(skip)]
    pub client_diagnostics_sessions: Vec<crate::modals::tabs::DiagnosticSessionView>,
    /// Whether the background SurrealDB fetch is still in flight.
    #[serde(skip)]
    pub client_diagnostics_loading: bool,
    /// Which connection_string the current `client_diagnostics_sessions`
    /// belongs to. Used to detect when the popup target changed and we
    /// need to refetch. `None` means "no fetch has been kicked off
    /// yet" — distinct from "fetched but got zero sessions back."
    #[serde(skip)]
    pub client_diagnostics_loaded_for: Option<String>,
    /// Last error from the diagnostics fetch, surfaced in the popup body.
    #[serde(skip)]
    pub client_diagnostics_error: Option<String>,
    /// Async background loaders post `DiagnosticSessionView`s here; the
    /// `receive_shared_ui` pump drains the channel into
    /// `client_diagnostics_sessions` each frame, same pattern the Task
    /// Modal already uses.
    #[serde(skip)]
    pub client_diagnostics_tx: Sender<crate::modals::tabs::DiagnosticSessionView>,
    #[serde(skip)]
    pub client_diagnostics_rx: Receiver<crate::modals::tabs::DiagnosticSessionView>,
    /// Selection state inside the popup (matches the `selected: &mut
    /// Option<RecordId>` parameter that
    /// `display_diagnostics_page` expects).
    #[serde(skip)]
    pub client_diagnostics_selected: Option<RecordId>,
    /// Slice 5 — per-client TCP reachability cache populated by
    /// the background prober (`spawn_reachability_prober` in
    /// `ui_data/reachability.rs`). Keyed by `connection_string`.
    /// Consulted by `should_show_connected_client_in_summaries`
    /// to decide whether to surface a client in the My Tasks and
    /// Admin Console lists.
    ///
    /// Why **local** state instead of a SurrealDB field: TCP
    /// reachability is *per-admin-network* — an admin VPN'd into
    /// the office can reach clients an admin at home can't. Each
    /// admin instance probes from its own network, so the
    /// result is only meaningful for that admin's filter view.
    #[serde(skip)]
    pub reachability_cache: HashMap<String, crate::ui_data::reachability::ReachabilityStatus>,
    /// Which connected clients the live query subscribes to. Non-root is
    /// clamped to `MyClients` when the query is built, so a stored wider scope
    /// never grants visibility.
    pub client_scope: crate::ui_data::ClientScope,
    /// Set when the operator picks a new scope; the next `receive_shared_logic`
    /// re-issues the live queries under the new filter.
    #[serde(skip)]
    pub client_scope_dirty: bool,
    /// `user` record key -> store label, for grouping a fleet-wide client list
    /// by the store that owns each machine.
    #[serde(skip)]
    pub user_store_map: HashMap<String, String>,
    #[serde(skip)]
    pub all_users_tx: Sender<Vec<database::schema::User>>,
    #[serde(skip)]
    pub all_users_rx: Receiver<Vec<database::schema::User>>,
    /// Latest `Cmd::OpenServiceCandidatesResponse` keyed by the
    /// connected client's `connection_string`.  Populated when the
    /// admin's Web Console session for that client returns a response
    /// to `Cmd::RequestOpenServiceCandidates`; consumed by the
    /// connected-client card to render the suggestion chip and by the
    /// Stage-4 confirmation modal.  In-memory only by design — no DB
    /// persistence of transient suggestions.
    #[serde(skip)]
    pub open_service_suggestions:
        HashMap<String, crate::open_service_suggestions::OpenServiceSuggestion>,
    /// Sender / Receiver the prober uses to ship probe results
    /// back to the UI thread. Drained per-frame in
    /// `receive_shared_ui`.
    #[serde(skip)]
    pub reachability_tx: Sender<crate::ui_data::reachability::ReachabilityEvent>,
    #[serde(skip)]
    pub reachability_rx: Receiver<crate::ui_data::reachability::ReachabilityEvent>,
    /// Cached fleet agent list from the orchestrator, displayed in the
    /// Fleet Dashboard tab for warehouse employees.  Updated by a background
    /// HTTP poller; `None` until the orchestrator URL is configured.
    #[serde(skip)]
    pub fleet_agents: Option<Vec<FleetAgentSummary>>,
    /// Sender half of the fleet poller channel. The poller task writes the
    /// latest `/api/v1/qc/agents` snapshot here; `drain_fleet_updates()`
    /// pulls the most-recent one off the receiver each frame.
    #[serde(skip)]
    pub fleet_agents_tx: crossbeam::channel::Sender<Vec<FleetAgentSummary>>,
    /// Receiver paired with `fleet_agents_tx`. Bounded(1) keeps us at the
    /// latest snapshot only — older payloads are dropped if the UI is slow.
    #[serde(skip)]
    pub fleet_agents_rx: crossbeam::channel::Receiver<Vec<FleetAgentSummary>>,
    #[serde(skip)]
    pub fleet_poller_running: bool,

    #[serde(skip)]
    pub live_queries_active: bool,

    #[serde(skip)]
    pub last_live_respawn_at: Option<web_time::Instant>,

    /// Live-query health-probe (canary) state. A periodic `live_query_check`
    /// notification is written to the DB and is expected to round-trip back
    /// through the `notification` live stream within a timeout.
    #[serde(skip)]
    pub canary_seq: u64,
    #[serde(skip)]
    pub canary_nonce: Option<String>,
    #[serde(skip)]
    pub canary_sent_at: Option<web_time::Instant>,
    #[serde(skip)]
    pub last_canary_at: Option<web_time::Instant>,
    /// Wall-clock instant of the last `refresh_client_list()` call.
    /// The admin's connected-client UI is normally driven by the
    /// `LIVE SELECT * FROM connected_client` subscription, but a
    /// missed live event (transient blip, subscription stall between
    /// retries) historically meant the UI got permanently stuck on
    /// stale state until the operator clicked "Clients" or restarted
    /// Mastertech.  `receive_shared_logic` now re-runs the one-shot
    /// fetch every 60 s as a self-heal fallback; this field
    /// rate-limits that.
    #[serde(skip)]
    pub last_client_list_refresh: Option<web_time::Instant>,
    /// In-memory snapshot of the connected-client list shared with the
    /// reachability prober. The prober was previously running its own
    /// `SELECT * FROM connected_client WHERE connected == true LIMIT 200`
    /// each round (every 30 s × N admins → meaningful ambient DB load
    /// per the `SurrealCrashes.md` audit). It now reads this Mutex
    /// instead. The UI thread refreshes the snapshot inside
    /// `receive_client` whenever the live-clients channel pushes an
    /// update, so the prober sees the same fresh data the UI does.
    #[serde(skip)]
    pub clients_for_prober: Arc<std::sync::Mutex<Vec<ConnectedClient>>>,
}

impl SharedContext {
    pub fn new(cc: &CreationContext<'_>) -> Self {
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
        let (all_users_tx, all_users_rx) = channel::unbounded::<Vec<database::schema::User>>();
        let (client_customer_resolved_tx, client_customer_resolved_rx) = channel::unbounded::<Vec<(String, RecordId)>>();
        let (notes_tx, notes_rx) = channel::unbounded::<(Action, TaskNotePayload)>();
        let (read_state_tx, read_state_rx) = channel::unbounded::<Vec<TaskNoteRead>>();
        let (client_diagnostics_tx, client_diagnostics_rx) =
            channel::unbounded::<crate::modals::tabs::DiagnosticSessionView>();
        let (reachability_tx, reachability_rx) =
            channel::unbounded::<crate::ui_data::reachability::ReachabilityEvent>();
        let (new_ticket_tx, new_ticket_rx) = channel::unbounded::<NewTicketChannel>();
        let (new_note_tx, new_note_rx) = channel::unbounded::<TaskNotePayload>();
        let (live_notification_tx, live_notification_rx) = channel::unbounded::<(Action, Notification)>();
        let (live_user_tx, live_user_rx) = channel::unbounded::<(Action, User)>();
        let (initial_ai_tasks_tx, initial_ai_tasks_rx) = channel::unbounded::<(Vec<AiTask>, Vec<AiTaskItem>)>();
        let (live_ai_tasks_tx, live_ai_tasks_rx) = channel::unbounded::<(Action, AiTask)>();
        let (live_ai_task_items_tx, live_ai_task_items_rx) = channel::unbounded::<(Action, AiTaskItem)>();
        // Unbounded so a burst of stream errors (all five streams dying on
        // one WS reset) is never silently dropped; the epoch tag lets the
        // drain discard entries from already-replaced stream generations.
        let (live_query_error_tx, live_query_error_rx) = channel::unbounded::<(u64, String)>();
        let (reconnect_result_tx, reconnect_result_rx) =
            channel::unbounded::<(u64, ReconnectOutcome)>();
        let (visibility_signal_tx, visibility_signal_rx) = channel::unbounded::<bool>();
        let (notification_tx, notification_rx) = channel::unbounded::<Vec<Notification>>();
        let bytes_channel = <(Vec<u8>, u64)>::create_unbounded_channel();
        let tur_channel = PrestashopPayload::create_unbounded_channel();
        let (settings_sender, settings_receiver) = crossbeam::channel::bounded::<Style>(1);
        let seb_channel = <Vec<CarboniteResponse>>::create_unbounded_channel();
        let specs_channel = <database::schema::prestashop::order::ExtractedOrderSpecs>::create_unbounded_channel();
        let (app_state_tx, app_state_rx) = channel::unbounded::<AppState>();
        let github_releases_channel = <Vec<GithubRelease>>::create_unbounded_channel();
        // bounded(1) → only the latest snapshot survives if the UI is slow.
        // Pair the tx/rx here so they share one channel; the poller `try_send`s
        // and drops on overflow.
        let (fleet_agents_tx, fleet_agents_rx) =
            crossbeam::channel::bounded::<Vec<FleetAgentSummary>>(1);
        let theme_config = ThemeConfig::default();
        let theme = set_custom_style(&theme_config);
        bootstrap_startup_theme(&cc.egui_ctx);
        let web_console_layout = AdminConsole::new(BTreeMap::new(), Vec::new());
        let filesystem = FileSystem::new();
        

        let dock = if cfg!(target_arch = "wasm32") {
            default_dock_session_wasm()
        } else {
            default_dock_session_native()
        };

        Self {
            dock,
            prestashop_order_form: PrestashopOrderForm::new(),
            koth: Koth::default(),
            first_run: true,
            user_theme_loaded: false,
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
            toasts: Toasts::new().anchor(Align2::RIGHT_TOP, (5.0, 45.0)),
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
            client_customer_resolved_tx, client_customer_resolved_rx,
            client_customer_resolving: std::collections::HashSet::new(),
            new_ticket_tx, new_ticket_rx,
            notes_tx, notes_rx,
            live_user_tx, live_user_rx,
            live_query_error_tx, live_query_error_rx,
            needs_reconnect: false,
            reconnect_result_tx, reconnect_result_rx,
            reconnect_token: 0,
            reconnect_in_progress: false,
            reconnect_started_at: None,
            reconnect_attempts: 0,
            reconnect_rebuilding: false,
            live_registered: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            live_streams_expected: 0,
            live_spawned_at: None,
            chat_streams_active: false,
            force_data_refetch: false,
            live_epoch: 0,
            live_stream_aborts: Vec::new(),
            live_session_id: database::new_live_session_id(),
            last_stream_error_at: None,
            last_force_refetch_at: None,
            visibility_signal_tx, visibility_signal_rx,
            tab_hidden_at: None,
            visibility_listener_installed: false,
            new_note_tx, new_note_rx,
            notification_tx, notification_rx,
            live_notification_tx, live_notification_rx,
            initial_ai_tasks_tx, initial_ai_tasks_rx,
            live_ai_tasks_tx, live_ai_tasks_rx,
            live_ai_task_items_tx, live_ai_task_items_rx,
            ai_tasks: HashMap::new(),
            ai_task_items: HashMap::new(),
            ai_task_done_grace: HashMap::new(),
            ai_popup_queue: std::collections::VecDeque::new(),
            ai_popup_modal: None,
            settings_sender, settings_receiver,
            bytes_channel,
            tur_channel,
            seb_channel,
            specs_channel,
            clients: Vec::new(),
            opened_modals: HashMap::new(),
            new_note: false,
            search_results: None,
            stock_tables: StockTable::default(),

            // Other Components
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
            last_toast: None,
            dismissed_admin_tcp_targets: HashSet::new(),
            filesystem,
            web_console_layout,
            web_console: WebConsole::new(),
            room_id: String::new(),
            user_chat: UserChat::default(),
            pending_store: None,
            sales_tracker: SalesTracker::default(),
            stress_lab: StressLab::default(),
            server_console: ServerConsole::default(),
            notification_center: NotificationCenter::default(),
            user_settings: UserSettings::default(),
            update_settings: false,
            get_settings: true,
            search_input: String::new(),
            total_download_size: 0.0,
            download_progress: 0.0,
            pending_tab_adds: Vec::new(),
            pending_tab_removes: Vec::new(),
            pending_activate_tab: None,
            pending_tab_opens: Vec::new(),
            added_nodes: Vec::new(),
            last_read_notes: HashMap::new(),
            read_state_tx, read_state_rx,
            pending_admin_console_focus: None,
            pending_open_service_candidate: None,
            open_service_confirm_modal: None,
            pending_open_service_apply: None,
            #[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
            entity_link_resolution_modal: None,
            client_diagnostics_popup: None,
            client_diagnostics_sessions: Vec::new(),
            client_diagnostics_loading: false,
            client_diagnostics_loaded_for: None,
            client_diagnostics_error: None,
            client_diagnostics_tx,
            client_diagnostics_rx,
            client_diagnostics_selected: None,
            reachability_cache: HashMap::new(),
            client_scope: Default::default(),
            client_scope_dirty: false,
            user_store_map: HashMap::new(),
            all_users_tx,
            all_users_rx,
            open_service_suggestions: HashMap::new(),
            reachability_tx,
            reachability_rx,
            fleet_agents: None,
            fleet_agents_tx,
            fleet_agents_rx,
            fleet_poller_running: false,
            live_queries_active: false,
            last_live_respawn_at: None,
            canary_seq: 0,
            canary_nonce: None,
            canary_sent_at: None,
            last_canary_at: None,
            last_client_list_refresh: None,
            clients_for_prober: Arc::new(std::sync::Mutex::new(Vec::new())),
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
            AppState::Authenticated(MainPages::Tasks) => {
                self.login.password = Default::default();
                Some(&mut self.login)
            },
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
        #[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
        {
            use crate::plugins::entity_link_pending::{
                entity_link_request_receiver, set_entity_link_ui_active,
            };
            set_entity_link_ui_active(true);
            while let Ok(req) = entity_link_request_receiver().try_recv() {
                if self.entity_link_resolution_modal.is_none() {
                    self.entity_link_resolution_modal =
                        Some(crate::modals::EntityLinkResolutionModal::new(req));
                }
            }
            if let Some(modal) = self.entity_link_resolution_modal.as_mut() {
                if modal.show(ctx).is_some() {
                    self.entity_link_resolution_modal = None;
                }
            }
        }

        if let Some(apply) = self.pending_open_service_apply.take() {
            let cs = apply.connection_string.clone();
            PlatformSpawner::spawn(async move {
                match crate::ui_data::open_service_apply::apply_open_service_confirm(&apply).await {
                    Ok(()) => log::info!("OpenService Stage-5 apply ok for {cs}"),
                    Err(e) => log::error!("OpenService Stage-5 apply failed for {cs}: {e}"),
                }
            });
        }

        // Sync AI handoff views into open task modals — snapshot cloned
        // before the mutable opened_modals borrow so card + modal render
        // the same SharedContext state.
        if !self.opened_modals.is_empty() && !self.ai_tasks.is_empty() {
            use database::schema::RecordIdExt;
            let mut ai_views_by_task: HashMap<String, Vec<(AiTask, Vec<AiTaskItem>)>> =
                HashMap::new();
            for task in self.ai_tasks.values() {
                let key = task.id.key_string();
                let mut items: Vec<AiTaskItem> = self
                    .ai_task_items
                    .values()
                    .filter(|i| i.ai_task_ref.key_string() == key)
                    .cloned()
                    .collect();
                items.sort_by_key(|i| i.position);
                ai_views_by_task
                    .entry(task.task_ref.key_string())
                    .or_default()
                    .push((task.clone(), items));
            }
            for views in ai_views_by_task.values_mut() {
                views.sort_by(|a, b| b.0.created_at.cmp(&a.0.created_at));
            }
            let ui_tx = self.ui_actions_tx.clone();
            for modal_type in self.opened_modals.values_mut() {
                if let ModalType::TaskModal(m) = modal_type {
                    let views = ai_views_by_task
                        .get(&m.task.id.key_string())
                        .cloned()
                        .unwrap_or_default();
                    m.sync_ai_state(views, ui_tx.clone());
                }
            }
        }

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

        // Pump the AI attention/review queue into the blocking modal slot,
        // one popup at a time; outcomes route through TaskUiActions.
        if self.ai_popup_modal.is_none() {
            if let Some(popup) = self.ai_popup_queue.pop_front() {
                self.ai_popup_modal =
                    Some(crate::modals::ai_attention_modal::AiAttentionModal {
                        popup,
                        queue_remaining: self.ai_popup_queue.len(),
                    });
            }
        }
        if let Some(modal) = self.ai_popup_modal.as_ref() {
            if let Some(outcome) = modal.show(ctx, &self.store_users) {
                use crate::modals::ai_attention_modal::{AiAttentionOutcome, AiPopupKind};
                let (popup, view_now) = match outcome {
                    AiAttentionOutcome::ViewNow(p) => (p, true),
                    AiAttentionOutcome::Later(p) => (p, false),
                };
                let review = popup.kind == AiPopupKind::OperatorReview;
                let _ = self.ui_actions_tx.try_send(TaskUiActions::AcknowledgeAiTask {
                    ai_task_id: popup.ai_task.id.clone(),
                    review,
                });
                if view_now {
                    let _ = self.ui_actions_tx.try_send(TaskUiActions::OpenTaskDiagnostics {
                        task_id: popup.ai_task.task_ref.clone(),
                        session: Some(popup.ai_task.session_ref.clone()),
                    });
                }
                // Show only ONE popup per burst: acknowledge and discard the
                // rest of the queue (they stay visible as cards + the column
                // badge) so a batch of new AI tasks never forces click-through.
                while let Some(extra) = self.ai_popup_queue.pop_front() {
                    let review = extra.kind == AiPopupKind::OperatorReview;
                    let _ = self.ui_actions_tx.try_send(TaskUiActions::AcknowledgeAiTask {
                        ai_task_id: extra.ai_task.id.clone(),
                        review,
                    });
                }
                self.ai_popup_modal = None;
            }
        }

        // Stage-4: instantiate the open-service-confirm modal whenever
        // a card click left a (`connection_string`, `candidate_index`)
        // tuple on `pending_open_service_candidate`.  The modal reads
        // the matching suggestion from the global store and renders
        // independently; once the operator confirms or rejects we
        // stash the resulting `apply` on `pending_open_service_apply`
        // for Stage-5 to drain.
        if let Some((cs, idx)) = self.pending_open_service_candidate.take() {
            if self.open_service_confirm_modal.is_none() {
                if let Some(suggestion) = crate::open_service_suggestions::get(&cs) {
                    if let Some(m) = crate::modals::OpenServiceConfirmModal::new(
                        cs, suggestion, idx,
                    ) {
                        self.open_service_confirm_modal = Some(m);
                    } else {
                        log::warn!(
                            "open_service_confirm_modal: candidate_index out of range; \
                             ignoring chip click"
                        );
                    }
                } else {
                    log::warn!(
                        "open_service_confirm_modal: no cached suggestion for \
                         connection_string; refresh from the card and try again"
                    );
                }
            }
        }
        if let Some(modal) = self.open_service_confirm_modal.as_mut() {
            if let Some(outcome) = modal.show(ctx) {
                match outcome {
                    crate::modals::OpenServiceConfirmOutcome::Confirm(apply) => {
                        log::info!(
                            "OpenServiceConfirm: operator confirmed bind for {} \
                             service #{}",
                            apply.connection_string,
                            apply.candidate.service_number
                        );
                        self.pending_open_service_apply = Some(apply);
                    }
                    crate::modals::OpenServiceConfirmOutcome::Reject => {
                        log::info!("OpenServiceConfirm: operator rejected bind");
                    }
                }
                self.open_service_confirm_modal = None;
            }
        }
    }
}

/// Reinstalls the font definitions, forcing a fresh glyph atlas and a full
/// font-texture upload.
pub fn reinstall_custom_fonts(ctx: &Context) {
    setup_custom_fonts(ctx);
}

fn setup_custom_fonts(ctx: &Context) {
    ctx.set_fonts(font_definitions());
}

/// The app's font stack. Exposed so callers can assert glyph coverage without a context.
pub fn font_definitions() -> FontDefinitions {
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

    fonts.font_data.insert(
        "CascadiaMono".to_owned(),
        std::sync::Arc::new(
            FontData::from_static(include_bytes!("../../MtechServer2.0/assets/fonts/CascadiaMono.ttf"))
        ),
    ); 

    // Put my font first (highest priority):
    fonts
        .families
        .get_mut(&FontFamily::Monospace)
        .unwrap()
        .insert(0, "UbuntuMonoNerdFont".to_owned()); // "Monaspace"

    // fonts
    //     .families
    //     .get_mut(&FontFamily::Name("CascadiaMono".into()))
    //     .unwrap()
    //     .insert(0, "CascadiaMono".to_owned()); // "Monaspace"

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

    // Sole face: anything the terminal UI emits that CascadiaMono lacks must
    // surface as tofu during development rather than silently break the grid.
    fonts.families.insert(
        FontFamily::Name("CascadiaMono".into()),
        vec!["CascadiaMono".to_owned()],
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

    crate::ui_tools::icons::install_fonts(&mut fonts);

    fonts
}

pub fn default_tree() -> DockSession {
    default_dock_session_native()
}

pub fn default_tree_wasm() -> DockSession {
    default_dock_session_wasm()
}

pub fn default_dock_session() -> DockSession {
    if cfg!(target_arch = "wasm32") {
        default_dock_session_wasm()
    } else {
        default_dock_session_native()
    }
}
