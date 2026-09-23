use crate::{tabs::file_browser::command::{RobocopyMessage, RobocopyProgress}, terminal_mode::{context::TerminalContext, events::action_handler::{get_update_sender, ActionHandler, WidgetId}, fx::{EffectStage, UniqueEffectId}, styling::ThemeRole, widgets::{button::Button, input_field::InputField}}};
use stress_runner::{RunController, RunUpdate, RunVerdict, Stressor, TelemetryAgent};
use std::{cell::RefCell, collections::{HashMap, VecDeque}, fmt::Display, sync::{Arc, Mutex}};
use ratatui::{layout::{Position, Rect}, widgets::{ListState, ScrollbarState}};
use checklist::{Status, TodoItem, TodoList};
use displays::scripts::{ScriptCategory, ScriptChannels};
use displays::scripts::catalog::{CATALOG, Surface};
use displays::virtual_filesystem::FileSystem;
use crossbeam::channel::{Receiver, Sender};
use crate::terminal_mode::widgets::tui_scroll_view::ScrollViewState;
use render::{Report, Reporter};
use database::schema::Node;
use dispatch::{QueueTally, QueuedRun, TerminalMcpPendingRun};

pub mod action_handler;
pub mod render;
pub mod checklist;
mod dispatch;
#[cfg(target_os="windows")]
pub mod script_categories;

/* A Reporting System for each of these things like the AHS tuneup */

////////////////////////////////
// SCRIPTS TAB with Buttons
////////////////////////////////
/// Let's say we have a subcomponent called ScriptsTab
// #[derive(Debug)]
pub struct ScriptsTab<'a> {
    service_number_field: InputField<'a>,
    custom_path_field: InputField<'a>,
    tuneup_btn: Button<'a>,
    user_scripts_btn: Button<'a>,
    informational_btn: Button<'a>,
    stress_tests_btn: Button<'a>,
    run_btn: Button<'a>,
    /// Launches a stress run via `stress_runner::RunController`.  Left-click
    /// starts a single-stressor run for the currently-selected stressor;
    /// right-click cycles through the 8 stress-kit stressors.
    pub stress_test_btn: Button<'a>,
    data_path_buttons: Vec<Button<'a>>,

    reports: RefCell<Vec<Report>>,
    current_reporter: RefCell<Reporter>,
    path_size_tx: Sender<Vec<(String, String)>>,
    path_size_rx: Receiver<Vec<(String, String)>>,
    progress: RefCell<Option<(u64, u64)>>,
    pub script_log_tx: Sender<String>,
    script_log_rx: Receiver<String>,
    /// Log and progress channels of queued runs.
    channels: ScriptChannels,
    /// Selected scripts waiting their turn.
    queue: VecDeque<TodoItem>,
    /// The queued script in flight.
    running: Option<QueuedRun>,
    /// Stop was pressed and the running script has not reported yet.
    stopping: bool,
    tally: QueueTally,
    /// When a ticket lookup started, while an email-dependent script waits on it.
    awaiting_ticket: Option<std::time::Instant>,
    next_run_token: u64,

    service_number: String,
    /// Service form ticket last copied into the service number field.
    seeded_service_number: String,
    /// Multipurpose checklist
    checklists: HashMap<String, TodoList>,

    checklist_area: RefCell<Option<Rect>>,
    report_area: RefCell<Option<Rect>>,
    report_scroll_state: RefCell<ScrollViewState>,
    list_scroll_state: RefCell<ScrollbarState>,
    list_state: RefCell<ListState>,
    visible_height: RefCell<usize>,
    total_items: RefCell<usize>,
    /// For the scrollbar area
    scroll_area: RefCell<Option<Rect>>,
    /// Script buttons column: scroll offset in lines (0..=max_offset)
    script_buttons_scroll_offset: RefCell<u16>,
    /// Scrollbar state for the script buttons column
    script_buttons_scroll_state: RefCell<ScrollbarState>,
    /// Viewport rect for script buttons column (for scroll wheel and scrollbar)
    script_buttons_viewport: RefCell<Option<Rect>>,
    /// (button ID, popup position)
    active_popup: RefCell<Option<(WidgetId, Rect)>>,
    frame_area: RefCell<Option<Rect>>,
    /// Tracks popup selection
    popup_list_state: RefCell<ListState>,
    popup_items: RefCell<HashMap<String, Vec<TodoItem>>>,
    /// (category, text) of the running script
    current_script: RefCell<Option<(ScriptCategory, String)>>,
    is_popup_open: RefCell<bool>,
    /// A finished script logged the reboot-recommended marker; prompt once the batch drains.
    reboot_pending: RefCell<bool>,
    /// Reboot prompt overlay is visible.
    reboot_prompt_open: RefCell<bool>,
    /// Job Builder column collapsed (default) vs expanded.
    job_builder_collapsed: RefCell<bool>,
    /// Clickable region that toggles the Job Builder collapse.
    job_builder_toggle_area: RefCell<Option<Rect>>,
    // destination_directory: String,
    data_transfer_progress_tx: Sender<RobocopyMessage>,
    data_transfer_progress_rx: Receiver<RobocopyMessage>,
    /// Tracks active robocopy processes by PID
    active_robocopy_processes: RefCell<HashMap<u32, RobocopyProgress>>,
    source_directories: Vec<(String, String)>,
    
    has_scrolled_manually: RefCell<bool>,
    init: RefCell<bool>,
    check_for_scripts: bool,
    user_scripts_bucket_loaded: bool,
    customer_email: String,
    ctx: Arc<Mutex<TerminalContext>>,
    filesystem: FileSystem,
    user_scripts_to_run: Vec<String>,
    robocopy_reports: RefCell<Vec<Report>>, // Robocopy-specific logs
    /// Track total offset for mouse coordinate adjustment
    total_offset: RefCell<u16>,
    /// Track the scripts tab area for coordinate adjustment
    scripts_area: RefCell<Option<Rect>>,
    loading: bool,
    /// Effect stage for animated border effects
    effect_stage: RefCell<EffectStage<UniqueEffectId>>,
    /// Track if border effects have been initialized
    effects_init: RefCell<bool>,
    /// In-flight runs requested by the MCP `scripts_run` tool, resolved by their outcomes.
    pending_mcp_runs: Vec<TerminalMcpPendingRun>,
    /// diagnostic_session id from the latest MCP scripts_run request.
    mcp_diagnostic_session_id: Option<String>,

    // ---- stress-runner integration (Phase 3) ----
    /// Active stress run, if any.  Polled from `receive()` each frame; the
    /// resulting `RunUpdate`s are appended to the reports log.
    pub stress_run: RefCell<Option<RunController>>,
    /// Shared telemetry agent, lazily created on first stress run.  The same
    /// agent is reused across runs to avoid re-spinning the sysinfo thread.
    pub stress_telemetry: RefCell<Option<Arc<TelemetryAgent>>>,
    /// Currently-selected stressor for the next run.  Right-clicking the
    /// stress button cycles through the 8 stress-kit stressors.
    pub stress_choice: RefCell<Stressor>,
    /// Duration (seconds) for the next single-stressor run.
    pub stress_duration_secs: RefCell<u64>,
    /// Latest throughput sample from the active run, for the live UI.
    pub stress_latest_throughput: RefCell<Option<(f64, &'static str)>>,
    /// `Some(verdict)` after a run finishes, cleared when the next run starts.
    pub stress_last_verdict: RefCell<Option<RunVerdict>>,
}

/// Compute a stable `computer:<machine_id>` record for the local machine.
/// Mastertech4.0 runs on technician workstations and customer machines alike;
/// this gives every stress run a stable per-host identity so subsequent
/// AI-driven analysis can correlate runs without needing a service number.
/// When the operator enters a service number, follow-up code can re-link the
/// run record to the customer's actual computer.
fn local_computer_record() -> database::schema::RecordId {
    crate::filesystem::local_computer_record()
}

impl<'a> ScriptsTab<'a> {
    pub const ROBOCOPY_DISPLAY_LINES: usize = 15; // Adjust as needed
    pub const CHECKLIST_ORDERED: [&'static str;4] = ["Tuneup / QC", "Informational", "Junkware Removal", "Stress Tests"];
    /// Lists with a selection popup, keyed by their button's widget id.
    pub const POPUP_ORDER: [&'static str; 3] = ["Tuneup / QC", "Informational", "Stress Tests"];
    
    pub fn new(ctx: Arc<Mutex<TerminalContext>>) -> Self {
        let (path_size_tx, path_size_rx) = crossbeam::channel::unbounded();
        let (data_transfer_progress_tx, data_transfer_progress_rx) = crossbeam::channel::unbounded();
        let (script_log_tx, script_log_rx) = crossbeam::channel::unbounded();

        let mut checklists: HashMap<String, TodoList> = HashMap::new();
        for def in CATALOG.for_surface(Surface::Terminal) {
            let Some(list) = checklist::list_name(&def.category()) else {
                continue;
            };
            checklists
                .entry(list.to_string())
                .or_insert_with(|| TodoList {
                    name: list.to_string(),
                    items: Vec::new(),
                    state: ListState::default(),
                })
                .items
                .push(TodoItem::from_def(def));
        }

        let popup_items: HashMap<String, Vec<TodoItem>> = Self::POPUP_ORDER
            .iter()
            .filter_map(|name| {
                checklists
                    .get(*name)
                    .map(|list| (name.to_string(), list.items.clone()))
            })
            .collect();

        Self {
            service_number_field: InputField::new("Service #", WidgetId("ServiceNumberScriptsPage".to_string())),
            custom_path_field: InputField::new("Source Path", WidgetId("CustomPath".to_string())),
            tuneup_btn: Button::new("Tuneup / QC", WidgetId("Tuneup / QC".to_owned())).compact(),
            user_scripts_btn: Button::new("User Scripts", WidgetId("UserScripts".to_owned())).compact(),
            informational_btn: Button::new("Informational", WidgetId("Informational".to_owned())).compact(),
            stress_tests_btn: Button::new("Stress Tests", WidgetId("Stress Tests".to_owned())).compact(),
            run_btn: Button::new("Run Selected", WidgetId("Run".to_owned())).theme(ThemeRole::Accent),
            stress_test_btn: Button::new(
                "Quick Stress",
                WidgetId("StressTest".to_owned()),
            )
            .theme(ThemeRole::Accent),
            reports: RefCell::new(vec![]),
            robocopy_reports: RefCell::new(vec![]),
            current_reporter: RefCell::new(Reporter::Unknown),
            service_number: String::new(),
            seeded_service_number: String::new(),
            path_size_tx,
            path_size_rx,
            data_transfer_progress_tx,
            data_transfer_progress_rx,
            active_robocopy_processes: RefCell::new(HashMap::new()),
            script_log_tx, script_log_rx,
            channels: ScriptChannels::default(),
            queue: VecDeque::new(),
            running: None,
            stopping: false,
            tally: QueueTally::default(),
            awaiting_ticket: None,
            next_run_token: 0,

            checklists,
            report_scroll_state: RefCell::new(ScrollViewState::new()),
            list_state: RefCell::new(ListState::default()),
            list_scroll_state: RefCell::new(ScrollbarState::default()),
            checklist_area: RefCell::new(None),
            report_area: RefCell::new(None),
            visible_height: RefCell::new(0),
            total_items: RefCell::new(0),
            scroll_area: RefCell::new(None),
            script_buttons_scroll_offset: RefCell::new(0),
            script_buttons_scroll_state: RefCell::new(ScrollbarState::default()),
            script_buttons_viewport: RefCell::new(None),
            active_popup: RefCell::new(None),
            frame_area: RefCell::new(None),
            popup_list_state: RefCell::new(ListState::default()),
            popup_items: RefCell::new(popup_items),
            current_script: RefCell::new(None),
            data_path_buttons: Vec::new(),
            is_popup_open: RefCell::new(false),
            reboot_pending: RefCell::new(false),
            reboot_prompt_open: RefCell::new(false),
            job_builder_collapsed: RefCell::new(true),
            job_builder_toggle_area: RefCell::new(None),
            // destination_directory: String::new(),
            source_directories: Vec::new(),
            progress: RefCell::new(None),
            has_scrolled_manually: RefCell::new(false),
            init: RefCell::new(true),
            check_for_scripts: false,
            user_scripts_bucket_loaded: false,
            customer_email: String::new(),
            ctx,
            filesystem: FileSystem::new(),
            user_scripts_to_run: Vec::new(),
            total_offset: RefCell::new(0),
            scripts_area: RefCell::new(None),
            loading: false,
            effect_stage: RefCell::new(EffectStage::default()),
            effects_init: RefCell::new(false),
            pending_mcp_runs: Vec::new(),
            mcp_diagnostic_session_id: None,
            stress_run: RefCell::new(None),
            stress_telemetry: RefCell::new(None),
            stress_choice: RefCell::new(Stressor::Cpu),
            stress_duration_secs: RefCell::new(60),
            stress_latest_throughput: RefCell::new(None),
            stress_last_verdict: RefCell::new(None),
        }
    }

    // -----------------------------------------------------------------
    // Stress-runner integration (Phase 3)
    // -----------------------------------------------------------------

    /// Start a single-stressor run with the currently-selected stressor and
    /// the configured duration.  Idempotent: if a run is already in flight,
    /// this is a no-op and the existing run continues.
    pub fn start_stress_run(&self) {
        if self.stress_run.borrow().is_some() {
            self.log_message("Stress run already in progress — ignoring start request");
            return;
        }

        // Lazy telemetry agent: one per process, reused across runs.  100 ms
        // is the minimum the agent honours (it clamps below that).
        let telemetry = {
            let mut guard = self.stress_telemetry.borrow_mut();
            if guard.is_none() {
                *guard = Some(Arc::new(TelemetryAgent::start(1000)));
            }
            guard.as_ref().unwrap().clone()
        };

        let stressor = *self.stress_choice.borrow();
        let duration = *self.stress_duration_secs.borrow();

        let computer = local_computer_record();
        let spec = stress_runner::RunSpec::single_stresskit(computer, stressor, Some(duration));
        let controller = RunController::start(spec, telemetry);

        *self.current_reporter.borrow_mut() = Reporter::StressTest;
        self.log_message(format!(
            "Stress run starting: {} for {}s (target stresskit:{})",
            stressor.label(),
            duration,
            stressor.label().to_lowercase()
        ));
        *self.stress_latest_throughput.borrow_mut() = None;
        *self.stress_last_verdict.borrow_mut() = None;
        *self.stress_run.borrow_mut() = Some(controller);
    }

    /// Stop the active stress run, if any.  The worker thread will roll up
    /// an `Aborted` verdict and emit a final `Finished` update.
    pub fn stop_stress_run(&self) {
        if let Some(c) = self.stress_run.borrow().as_ref() {
            c.stop();
            self.log_message("Stress run cancel requested");
        }
    }

    /// Cycle through every stress-kit stressor.  Bound to right-click on the
    /// stress button.  Takes `&self` (uses RefCell internally) so the mouse
    /// handler — which is `&self` — can call it directly.  The button label
    /// stays generic; the current stressor is reported in the log.
    pub fn cycle_stress_choice(&self) {
        let all = Stressor::all();
        let current = *self.stress_choice.borrow();
        let idx = all.iter().position(|s| *s == current).unwrap_or(0);
        let next = all[(idx + 1) % all.len()];
        *self.stress_choice.borrow_mut() = next;
        self.log_message(format!(
            "Stress test stressor → {} ({}s)",
            next.label(),
            *self.stress_duration_secs.borrow()
        ));
    }

    /// Drain controller updates and translate them into reports + UI state.
    /// Called from `receive()` each frame.
    fn poll_stress_run(&mut self) {
        let updates_and_done = {
            let guard = self.stress_run.borrow();
            let Some(controller) = guard.as_ref() else {
                return;
            };
            let running = controller.is_running();
            (controller.poll(), !running)
        };
        let (updates, done) = updates_and_done;

        for update in updates {
            match update {
                RunUpdate::Started { run_id } => {
                    use database::schema::RecordIdExt;
                    self.log_message(format!("Stress run id: {}", run_id.key_string()));
                }
                RunUpdate::StageStarted { index, label, stage_count } => {
                    if stage_count > 1 {
                        self.log_message(format!("Stage {}/{}: {}", index + 1, stage_count, label));
                    }
                }
                RunUpdate::Tick { metrics, throughput_unit, .. } => {
                    *self.stress_latest_throughput.borrow_mut() =
                        Some((metrics.throughput, throughput_unit));
                }
                RunUpdate::StageFinished { .. } => {}
                RunUpdate::StageVerdict { label, pass, violations, unevaluated, .. } => {
                    if !pass {
                        self.log_message(format!(
                            "Stage {label} FAIL: {}",
                            violations.join("; ")
                        ));
                    }
                    for gap in unevaluated {
                        self.log_message(format!("Stage {label} ungraded: {gap}"));
                    }
                }
                RunUpdate::Finished(verdict) => {
                    let result_str = match verdict.result {
                        stress_runner::RunResult::Pass => "PASS",
                        stress_runner::RunResult::Fail => "FAIL",
                        stress_runner::RunResult::Aborted => "ABORTED",
                        stress_runner::RunResult::Inconclusive => "INCONCLUSIVE",
                        stress_runner::RunResult::InProgress => "IN_PROGRESS",
                    };
                    self.log_message(format!(
                        "Stress run {} — {:.1} s — peak {} {} — max temp {}°C",
                        result_str,
                        verdict.duration_secs,
                        verdict.summary.peak_throughput
                            .map(|p| format!("{:.2}", p))
                            .unwrap_or_else(|| "—".to_string()),
                        verdict.summary.throughput_unit.as_deref().unwrap_or("ops/s"),
                        verdict.summary.max_temp_c
                            .map(|t| format!("{:.1}", t))
                            .unwrap_or_else(|| "—".to_string()),
                    ));
                    *self.stress_last_verdict.borrow_mut() = Some(verdict);
                }
                RunUpdate::Warning { message } => {
                    self.log_message(format!("Stress warning: {message}"));
                }
                RunUpdate::Error { message } => {
                    self.log_message(format!("Stress error: {message}"));
                }
            }
        }

        if done {
            *self.stress_run.borrow_mut() = None;
        }
    }

    /// Logs a message under the current `Reporter`
    pub fn log_message(&self, msg: impl Display) {
        let reporter = self.current_reporter.borrow().clone();
        let is_robocopy = reporter == Reporter::Robocopy;
        let log_entry = Report {
            reporter,
            msg: msg.to_string(),
        };

        if is_robocopy {
            // Store in robocopy logs
            self.robocopy_reports.borrow_mut().push(log_entry);
        } else {
            // Store in general logs
            let log_lines = self.reports.borrow().len() as u16;
            self.reports.borrow_mut().push(log_entry);
            let mut scroll_state = self.report_scroll_state.borrow_mut();
            let scroll_x = scroll_state.offset().x;
            let visible_height = self.report_area
                .borrow()
                .map_or(0, |area| area.height.saturating_sub(2 + Self::ROBOCOPY_DISPLAY_LINES as u16));

            if !*self.has_scrolled_manually.borrow() && log_lines > visible_height {
                let scroll_y = log_lines.saturating_sub(visible_height);
                scroll_state.set_offset(Position { x: scroll_x, y: scroll_y });
            }
            *self.has_scrolled_manually.borrow_mut() = false;
        }
    }

    pub fn receive(&mut self) {
        // Drain any in-flight stress run before the rest of the channel work
        // so the latest throughput/verdict shows up in the same frame.
        self.poll_stress_run();
        self.seed_from_ticket();

        let preview = self.filesystem.previewed_file.clone();
        if let Some(file_contents) = preview {
            self.log_message(file_contents.clone());
            self.user_scripts_to_run.push(file_contents);
            self.filesystem.previewed_file = None;
        }

        if let Ok(path_info) = self.path_size_rx.try_recv() {
            for (path, size) in path_info {
                // Only add if path is not already present in source_directories
                if !self.source_directories.iter().any(|(p, _)| p == &path) {
                    self.source_directories.push((path.clone(), size.clone()));
                }

                self.log_message(&format!("Path {:<5} Size: {:>5}", path.clone(), size.clone()));
                let btn = Button::new(
                    format!(" {} | {} ", path.clone(), size.clone()),
                    WidgetId(path.clone())
                )
                .theme(ThemeRole::Neutral);
                self.data_path_buttons.push(btn);
            }

            self.is_popup_open.replace(true);
            self.loading = false;
            let _ = get_update_sender().try_send(self.widget_id());
        }


        // Handle robocopy progress messages
        while let Ok(msg) = self.data_transfer_progress_rx.try_recv() {
            match msg {
                RobocopyMessage::Progress(progress) => {
                    self.active_robocopy_processes.borrow_mut().insert(progress.pid, progress);
                }
                RobocopyMessage::Complete(pid) => {
                    self.active_robocopy_processes.borrow_mut().remove(&pid);
                    self.log_message(format!("Robocopy process {} completed", pid));
                }
            }
        }

        while let Ok(msg) = self.script_log_rx.try_recv() {
            if msg.contains(displays::scripts::REBOOT_RECOMMENDED_MARKER) {
                self.reboot_pending.replace(true);
            }
            self.log_message(&msg);
        }

        self.drain_script_logs();
        self.advance_queue();
        self.sync_run_button();

        // Prompt once the queue has drained.
        if *self.reboot_pending.borrow() && !self.queue_busy() {
            self.reboot_pending.replace(false);
            self.reboot_prompt_open.replace(true);
            self.log_message("Reboot required to finalize Webroot activation — press R to reboot now (MasterTech relaunches after login), Esc to dismiss.");
        }
    }

    fn get_selected_scripts(&self) -> Vec<TodoItem> {
        let popup_items = self.popup_items.borrow();
        Self::POPUP_ORDER
            .iter()
            .chain(std::iter::once(&"UserScripts"))
            .filter_map(|key| popup_items.get(*key))
            .flat_map(|items| {
                items.iter().filter(|item| item.status == Status::Completed).cloned()
            })
            .collect()
    }

    fn clear_selected_scripts(&self) {
        let mut popup_items = self.popup_items.borrow_mut();

        for items in popup_items.values_mut() {
            for todo_item in items {
                if todo_item.status == Status::Completed {
                    todo_item.status = Status::Todo;
                }
            }
        }
    }

    pub fn insert_user_scripts(&mut self) {
        if !self.check_for_scripts || !self.user_scripts_bucket_loaded {
            return;
        }
        let current_folder = self.filesystem.get_current_folder();
        let Some(database::schema::Node::Folder(_, children)) = current_folder else {
            return;
        };

        let todo_items: Vec<TodoItem> = children
            .values()
            .flat_map(|node| match node {
                Node::File((full_path, name)) => {
                    vec![TodoItem::new(name, ScriptCategory::UserScripts(full_path.to_string()))]
                }
                Node::Folder(_, child) => child
                    .values()
                    .filter_map(|node| {
                        if let Node::File((full_path, name)) = node {
                            Some(TodoItem::new(name, ScriptCategory::UserScripts(full_path.to_string())))
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>(),
            })
            .collect();

        self.checklists.insert(
            "User Scripts".to_string(),
            TodoList {
                name: "User Scripts".to_string(),
                state: ListState::default(),
                items: todo_items.clone(),
            },
        );
        self.popup_items
            .borrow_mut()
            .insert("UserScripts".to_string(), todo_items);
        self.check_for_scripts = false;
    }

    fn remove_button(&mut self, id: &str) {
        let pre_source = self.source_directories.clone();
        
        // Remove from source_directories
        self.source_directories.retain(|(path, _size)| !path.eq(id));
    
        // Remove from data_path_buttons
        let button_index = self.data_path_buttons.iter().position(|btn| {
            let btn_widget_id = btn.get_widget_id();
            let btn_id = btn_widget_id.0.as_str();
            btn_id.eq(id)
        });
    
        if let Some(index) = button_index {
            self.data_path_buttons.remove(index);
        }
    
        self.log_message(format!(
            "Sources before: {:?}\nAfter: {:?}\nButtons: {:?}", 
            pre_source, 
            self.source_directories, 
            self.data_path_buttons.iter().map(|btn| btn.get_widget_id().0.clone()).collect::<Vec<_>>()
        ));
    }
}