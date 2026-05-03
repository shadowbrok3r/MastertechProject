use crate::{tabs::file_browser::command::{RobocopyMessage, RobocopyProgress}, utilities::scripts::ScheduledTask, terminal_mode::{context::TerminalContext, events::action_handler::{get_update_sender, ActionHandler, WidgetId}, fx::{EffectStage, UniqueEffectId}, styling::{CATPPUCCINTHEME, CYAN, DEEPPINK}, widgets::{button::Button, input_field::InputField}}};
use std::{cell::RefCell, collections::HashMap, fmt::Display, sync::{Arc, Mutex}};
use ratatui::{layout::{Position, Rect}, widgets::{ListState, ScrollbarState}};
use checklist::{Category, Status, TodoItem, TodoList};
use displays::virtual_filesystem::FileSystem;
use crossbeam::channel::{Receiver, Sender};
use crate::terminal_mode::widgets::tui_scroll_view::ScrollViewState;
use render::{Report, Reporter};
use database::schema::Node;
use reqwest::Client;

#[cfg(target_os="windows")]
use crate::utilities::{scripts::{AntiVirusProduct, InstalledProgram}, windows::windows_update::{WindowsUpdateEvent, WindowsUpdates}};

pub mod action_handler;
pub mod render;
pub mod checklist;
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
    run_btn: Button<'a>,
    data_path_buttons: Vec<Button<'a>>,

    reports: RefCell<Vec<Report>>, 
    current_reporter: RefCell<Reporter>,
    #[cfg(target_os="windows")]
    update_log_tx: Sender<WindowsUpdateEvent>,
    #[cfg(target_os="windows")]
    update_log_rx: Receiver<WindowsUpdateEvent>,
    path_size_tx: Sender<Vec<(String, String)>>,
    path_size_rx: Receiver<Vec<(String, String)>>,
    progress_tx: Sender<(u64, u64)>,
    progress_rx: Receiver<(u64, u64)>,
    progress: RefCell<Option<(u64, u64)>>,
    pub script_log_tx: Sender<String>,
    script_log_rx: Receiver<String>,
    checklist_completion_tx: Sender<(Category, String, bool)>,
    checklist_completion_rx: Receiver<(Category, String, bool)>,
    update_progress: RefCell<Option<u64>>,
    windows_installation: RefCell<bool>,
    
    service_number: String,
    /// Antivirus tab
    #[cfg(target_os="windows")]
    antivirus_products: Vec<AntiVirusProduct>,
    /// Installed Programs tab
    #[cfg(target_os="windows")]
    installed_programs: Vec<InstalledProgram>,
    // /// Startup Items tab
    // startup_programs: Vec<StartupProgram>,
    /// Scheduled Tasks tab
    scheduled_tasks: Vec<ScheduledTask>,
    // /// Taskbar Items tab
    // taskbar_items: Vec<TaskbarItem>,
    /// Multipurpose checklist
    checklists: HashMap<String, TodoList>,
    /// Stores the latest retrieved Windows Updates info
    #[cfg(target_os="windows")]
    windows_updates: WindowsUpdates,

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
    current_script: RefCell<Option<(Category, String)>>, 
    is_popup_open: RefCell<bool>,
    // destination_directory: String,
    data_transfer_progress_tx: Sender<RobocopyMessage>,
    data_transfer_progress_rx: Receiver<RobocopyMessage>,
    /// Tracks active robocopy processes by PID
    active_robocopy_processes: RefCell<HashMap<u32, RobocopyProgress>>,
    source_directories: Vec<(String, String)>,
    
    has_scrolled_manually: RefCell<bool>,
    init: RefCell<bool>,
    check_for_scripts: bool,
    client: Client,
    customer_email: String,
    ctx: Arc<Mutex<TerminalContext>>,
    filesystem: FileSystem,
    user_scripts_to_run: Vec<String>,
    scripts_waiting_for_data: Vec<TodoItem>,
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
    /// In-flight MCP-initiated script runs. Each entry is registered when the
    /// terminal tab receives a `ScriptRunRequest` over the global crossbeam
    /// channel and is resolved either by a matching `checklist_completion_rx`
    /// signal or by timeout. See `process_mcp_requests` / `process_mcp_completions`.
    pending_mcp_runs: Vec<TerminalMcpPendingRun>,
}

/// Tracks one in-flight MCP-initiated script run inside the terminal `ScriptsTab`.
/// Logs collected during execution (drained off `script_log_rx` each frame) are
/// returned to the MCP caller in the `ScriptRunResult.logs` field.
#[derive(Debug, Clone)]
pub struct TerminalMcpPendingRun {
    pub request_id: String,
    pub script_name: String,
    pub category: Category,
    pub dispatched_at: std::time::Instant,
    pub timeout: std::time::Duration,
    pub log_lines: Vec<String>,
}

impl<'a> ScriptsTab<'a> {
    pub const ROBOCOPY_DISPLAY_LINES: usize = 15; // Adjust as needed
    pub const CHECKLIST_ORDERED: [&'static str;3] = ["Tuneup / QC", "Informational", "Junkware Removal"];
    
    pub fn new(client: Client, ctx: Arc<Mutex<TerminalContext>>) -> Self {
        #[cfg(target_os="windows")]
        let (update_log_tx, update_log_rx) = crossbeam::channel::unbounded();
        let (path_size_tx, path_size_rx) = crossbeam::channel::unbounded();
        let (data_transfer_progress_tx, data_transfer_progress_rx) = crossbeam::channel::unbounded();
        let (progress_tx, progress_rx) = crossbeam::channel::unbounded();
        let (script_log_tx, script_log_rx) = crossbeam::channel::unbounded();
        let (checklist_completion_tx, checklist_completion_rx) = crossbeam::channel::unbounded();

        let mut checklists = HashMap::new();
        
        // Define checklists with categories
        checklists.insert(
            "Tuneup / QC".to_string(),
            TodoList {
                name: "Tuneup / QC".to_string(),
                state: ListState::default(),
                items: vec![
                    TodoItem::new("Data Transfer", Category::Tuneup),
                    TodoItem::new("Activate Webroot", Category::Tuneup),
                    TodoItem::new("Activate SuperAnti", Category::Tuneup),
                    TodoItem::new("Activate SEB", Category::Tuneup),
                    TodoItem::new("Install Windows Updates", Category::Tuneup),
                    TodoItem::new("Disable Sleep / Hibernation", Category::Tuneup), // Works
                    TodoItem::new("Run SuperAntiSpyware Scan", Category::Tuneup),
                    TodoItem::new("Run Webroot Scan", Category::Tuneup),
                    TodoItem::new("Run Junkware Category", Category::JunkwareRemoval),
                    TodoItem::new("Run Tron", Category::Tuneup), 
                    // TodoItem::new("--------------------------------", Category::Tuneup), 
                    TodoItem::new("Install LibreOffice", Category::Tuneup),
                    TodoItem::new("Disable proxy settings", Category::Tuneup), // Works
                    TodoItem::new("Disable Notifications", Category::Tuneup),
                    TodoItem::new("Change SuperAntiSpyware settings", Category::Tuneup),
                    TodoItem::new("Disable Startup Apps", Category::Tuneup),
                    TodoItem::new("Unpin Copilot", Category::Tuneup),
                    TodoItem::new("Align Taskbar to left", Category::Tuneup), // Works
                    TodoItem::new("Change Timezone to Mountain", Category::Tuneup),
                    TodoItem::new("Disable BitLocker", Category::Tuneup),
                ],
            },
        );

        checklists.insert(
            "Junkware Removal".to_string(),
            TodoList {
                name: "Junkware Removal".to_string(),
                state: ListState::default(),
                items: vec![
                    TodoItem::new("Webroot TEST", Category::JunkwareRemoval),
                    TodoItem::new("SuperAnti TEST", Category::JunkwareRemoval),
                    TodoItem::new("OneLaunch", Category::JunkwareRemoval),
                    TodoItem::new("WebNavigator Browser", Category::JunkwareRemoval),
                    // TodoItem::new("ESET Security", Category::JunkwareRemoval),
                    TodoItem::new("Wave Browser", Category::JunkwareRemoval),
                    TodoItem::new("Clear Browser", Category::JunkwareRemoval),
                    TodoItem::new("Shift Browser", Category::JunkwareRemoval),
                    TodoItem::new("Avast Browser", Category::JunkwareRemoval),
                    TodoItem::new("Mcaffee Safe", Category::JunkwareRemoval),
                    TodoItem::new("Driver Support", Category::JunkwareRemoval),
                    TodoItem::new("Winzip", Category::JunkwareRemoval),
                    TodoItem::new("Uninstall Microsoft 365", Category::JunkwareRemoval),
                    TodoItem::new("Uninstall OneDrive", Category::JunkwareRemoval),
                    TodoItem::new("Disable OneDrive Startup", Category::JunkwareRemoval),
                    TodoItem::new("Disable Edge Startup Boost", Category::JunkwareRemoval),
                ],
            },
        );
        
        checklists.insert(
            "Informational".to_string(),
            TodoList {
                name: "Informational".to_string(),
                state: ListState::default(),
                items: vec![
                    TodoItem::new("Is SuperEasyBackup installed?", Category::Informational)
                        .set_pass_criteria("Installed and active")
                        .set_warning_criteria("Not installed OR its not active")
                        .set_error_criteria("Script Failed To Run"),
                    TodoItem::new("Is Webroot installed?", Category::Informational)
                        .set_pass_criteria("Installed and active")
                        .set_warning_criteria("Not installed OR its not active")
                        .set_error_criteria("Script Failed To Run"),
                    TodoItem::new("Is SuperAntiSpyware installed?", Category::Informational)
                        .set_pass_criteria("Installed and active")
                        .set_warning_criteria("Not installed OR its not active")
                        .set_error_criteria("Script Failed To Run"),
                    TodoItem::new("Are there scheduled tasks for it?", Category::Informational),
                    TodoItem::new("Is Windows Activated?", Category::Informational),
                    TodoItem::new("Is Hibernation/Sleep enabled?", Category::Informational),
                    TodoItem::new("Any Recent Blue Screens?", Category::Informational),
                    TodoItem::new("When Was The Last Service Date?", Category::Informational),
                    TodoItem::new("Windows Version", Category::Informational)
                        .set_pass_criteria("Windows 11")
                        .set_warning_criteria("Windows 10")
                        .set_error_criteria("Script Failed To Run"),
                    TodoItem::new("Check Updates", Category::Tuneup),
                    TodoItem::new("Run Prechecks", Category::Informational)
                ],
            },
        );
        // Sync popup_items with checklists
        let mut popup_items = HashMap::new();
        popup_items.insert(
            "Tuneup / QC".to_string(),
            checklists.get("Tuneup / QC").unwrap().items.clone(),
        );
        popup_items.insert(
            "Informational".to_string(),
            checklists.get("Informational").unwrap().items.clone(),
        );

        Self {
            service_number_field: InputField::new("Service #", WidgetId("ServiceNumberScriptsPage".to_string())),
            custom_path_field: InputField::new("Source Path", WidgetId("CustomPath".to_string())),
            tuneup_btn: Button::new("Tuneup / QC =>", WidgetId("Tuneup / QC".to_owned())).theme(CATPPUCCINTHEME),
            user_scripts_btn: Button::new("User Scripts =>", WidgetId("UserScripts".to_owned())).theme(CATPPUCCINTHEME),
            informational_btn: Button::new("Informational =>", WidgetId("Informational".to_owned())).theme(CATPPUCCINTHEME),
            run_btn: Button::new("Run Selected", WidgetId("Run".to_owned())).theme(DEEPPINK),
            #[cfg(target_os="windows")]
            antivirus_products: Vec::new(),
            #[cfg(target_os="windows")]
            installed_programs: Vec::new(),
            // startup_programs: Vec::new(),
            scheduled_tasks: Vec::new(),
            // taskbar_items: Vec::new(),

            reports: RefCell::new(vec![]),
            robocopy_reports: RefCell::new(vec![]),
            current_reporter: RefCell::new(Reporter::Unknown),
            service_number: String::new(),
            #[cfg(target_os="windows")]
            update_log_tx, 
            #[cfg(target_os="windows")]
            update_log_rx,
            path_size_tx, 
            path_size_rx,
            data_transfer_progress_tx, 
            data_transfer_progress_rx, 
            active_robocopy_processes: RefCell::new(HashMap::new()),
            progress_tx, progress_rx,
            script_log_tx, script_log_rx,
            checklist_completion_tx, checklist_completion_rx,

            checklists,
            #[cfg(target_os="windows")]
            windows_updates: WindowsUpdates::default(),
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
            // destination_directory: String::new(),
            source_directories: Vec::new(),
            progress: RefCell::new(None),
            update_progress: RefCell::new(None),
            has_scrolled_manually: RefCell::new(false),
            init: RefCell::new(true),
            check_for_scripts: false,
            client,
            customer_email: String::new(),
            ctx,
            filesystem: FileSystem::new(),
            user_scripts_to_run: Vec::new(),
            scripts_waiting_for_data: Vec::new(),
            windows_installation: RefCell::new(false),
            total_offset: RefCell::new(0),
            scripts_area: RefCell::new(None),
            loading: false,
            effect_stage: RefCell::new(EffectStage::default()),
            effects_init: RefCell::new(false),
            pending_mcp_runs: Vec::new(),
        }
    }

    /// Drain MCP `scripts_run` requests off the global crossbeam channel and
    /// dispatch each through the terminal's existing `handle_*` methods.
    /// Tracks each request in `pending_mcp_runs` so the next `receive()` call
    /// can match completion signals (from `checklist_completion_rx`) and
    /// report the result + collected logs back to the MCP caller.
    pub fn process_mcp_requests(&mut self) {
        while let Ok(req) = displays::scripts::script_run_request_receiver().try_recv() {
            self.dispatch_mcp_request(req);
        }
    }

    fn dispatch_mcp_request(&mut self, req: displays::scripts::ScriptRunRequest) {
        let local_cat = match req.category {
            displays::scripts::ScriptCategory::Tuneup => Category::Tuneup,
            displays::scripts::ScriptCategory::Informational => Category::Informational,
            displays::scripts::ScriptCategory::JunkwareRemoval => Category::JunkwareRemoval,
            other => {
                let _ = displays::scripts::script_run_result_sender().send(
                    displays::scripts::ScriptRunResult {
                        request_id: req.request_id,
                        success: false,
                        message: format!("Unsupported category: {:?}", other),
                        logs: Vec::new(),
                    },
                );
                return;
            }
        };

        if let Some(sn) = req.service_number.as_deref() {
            if !sn.is_empty() {
                self.service_number = sn.to_string();
            }
        }
        if let Some(em) = req.customer_email.as_deref() {
            if !em.is_empty() {
                self.customer_email = em.to_string();
            }
        }

        let request_id = req.request_id.clone();
        let script_name = req.script_name.clone();
        let category = local_cat.clone();

        self.pending_mcp_runs.push(TerminalMcpPendingRun {
            request_id: request_id.clone(),
            script_name: script_name.clone(),
            category: category.clone(),
            dispatched_at: std::time::Instant::now(),
            timeout: std::time::Duration::from_secs(600),
            log_lines: vec![format!(
                "MCP requested: {} (category {:?}, request_id {})",
                script_name, category, request_id
            )],
        });

        #[cfg(target_os = "windows")]
        {
            match category {
                Category::Tuneup => self.handle_tuneup(&script_name, &Category::Tuneup),
                Category::Informational => {
                    self.handle_informational(&script_name, &Category::Informational)
                }
                Category::JunkwareRemoval => {
                    self.handle_junkware_removal(&script_name, &Category::JunkwareRemoval)
                }
                Category::UserScripts(_) => {
                    let _ = displays::scripts::script_run_result_sender().send(
                        displays::scripts::ScriptRunResult {
                            request_id,
                            success: false,
                            message: "User scripts cannot be invoked via MCP".into(),
                            logs: Vec::new(),
                        },
                    );
                    if let Some(idx) = self
                        .pending_mcp_runs
                        .iter()
                        .position(|p| p.script_name == script_name)
                    {
                        self.pending_mcp_runs.remove(idx);
                    }
                }
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            let _ = displays::scripts::script_run_result_sender().send(
                displays::scripts::ScriptRunResult {
                    request_id,
                    success: false,
                    message: format!(
                        "Script '{}' cannot run: terminal-mode script handlers are Windows-only",
                        script_name
                    ),
                    logs: Vec::new(),
                },
            );
            if let Some(idx) = self
                .pending_mcp_runs
                .iter()
                .position(|p| p.script_name == script_name)
            {
                self.pending_mcp_runs.remove(idx);
            }
            let _ = category;
        }
    }

    /// Time out any pending MCP runs whose `timeout` has elapsed without a
    /// matching `checklist_completion_rx` signal. Successful / failed
    /// completions are sent back inside `receive()` itself, where the
    /// completion channel is drained.
    pub fn process_mcp_completions(&mut self) {
        if self.pending_mcp_runs.is_empty() {
            return;
        }
        let now = std::time::Instant::now();
        let mut to_remove: Vec<usize> = Vec::new();
        for (idx, pending) in self.pending_mcp_runs.iter().enumerate() {
            if now.duration_since(pending.dispatched_at) > pending.timeout {
                let _ = displays::scripts::script_run_result_sender().send(
                    displays::scripts::ScriptRunResult {
                        request_id: pending.request_id.clone(),
                        success: false,
                        message: format!(
                            "Script '{}' did not complete within {}s",
                            pending.script_name,
                            pending.timeout.as_secs()
                        ),
                        logs: pending.log_lines.clone(),
                    },
                );
                to_remove.push(idx);
            }
        }
        for idx in to_remove.iter().rev() {
            self.pending_mcp_runs.remove(*idx);
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
        let preview = self.filesystem.previewed_file.clone();
        if let Some(file_contents) = preview {
            self.log_message(file_contents.clone());
            self.user_scripts_to_run.push(file_contents);
            self.filesystem.previewed_file = None;
        }

        if let Ok(progress) = self.progress_rx.try_recv() {
            self.progress.replace(Some(progress));
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
                .theme(CYAN);
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
            for pending in self.pending_mcp_runs.iter_mut() {
                pending.log_lines.push(msg.clone());
            }
            self.log_message(&msg);
        }

        while let Ok((category, item_text, success)) = self.checklist_completion_rx.try_recv() {
            if let Some(idx) = self.pending_mcp_runs.iter().position(|p| {
                p.category == category && p.script_name == item_text
            }) {
                let pending = self.pending_mcp_runs.remove(idx);
                let _ = displays::scripts::script_run_result_sender().send(
                    displays::scripts::ScriptRunResult {
                        request_id: pending.request_id,
                        success,
                        message: if success {
                            format!("Script '{}' completed successfully", item_text)
                        } else {
                            format!("Script '{}' reported failure", item_text)
                        },
                        logs: pending.log_lines,
                    },
                );
            }
            self.update_checklist(category, &item_text, success);
        }

        #[cfg(target_os="windows")]
        {
            if let Ok(event) = self.update_log_rx.try_recv() {
                match event {
                    WindowsUpdateEvent::UpdateLogs(log) => self.log_message(&log),
                    WindowsUpdateEvent::ReturnedUpdates(windows_updates) => {
                        self.log_message(&format!("{windows_updates:#?}"));
                        self.windows_updates = windows_updates;
                    },
                    WindowsUpdateEvent::DownloadPercentage(percent) => {
                        self.update_progress.replace(Some(percent as u64));
                    },
                    WindowsUpdateEvent::InstallPercentage(percent) => {
                        self.windows_installation.replace(true);
                        self.update_progress.replace(Some(percent as u64));
                    },
                }
            }
        }
    }

    fn get_selected_scripts(&self) -> Vec<TodoItem> {
        let popup_items = self.popup_items.borrow();
        popup_items
            .values()
            .flat_map(|items| {
                items.iter().filter(|item| item.status == Status::Completed).cloned()
            })
            .collect()
    }

    /// Scripts that require a service number to run (activation scripts).
    const SCRIPTS_REQUIRING_SERVICE_NUMBER: &'static [&'static str] = &[
        "Activate Webroot",
        "Activate SuperAnti",
        "Activate SEB",
    ];

    /// True when "Run Selected" should be disabled: any selected script requires a service number
    /// but none is provided (neither in the field nor in `service_number`).
    pub fn run_button_should_be_disabled(&self) -> bool {
        let selected = self.get_selected_scripts();
        let any_requires_sn = selected
            .iter()
            .any(|s| Self::SCRIPTS_REQUIRING_SERVICE_NUMBER.contains(&s.text.as_str()));
        if !any_requires_sn {
            return false;
        }
        let has_sn = !self.service_number.trim().is_empty()
            || !self
                .service_number_field
                .get_text()
                .first()
                .map(|s| s.trim())
                .unwrap_or("")
                .is_empty();
        !has_sn
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
        if self.check_for_scripts {
            let current_folder = self.filesystem.get_current_folder();
            if let Some(node) = current_folder {
                match node {
                    database::schema::Node::Folder(_, children) => {
                        if !children.is_empty() {
                            // log::info!("Children of {path}: {:?}", children);
                            let todo_items: Vec<TodoItem> = children
                            .values()
                            .flat_map(|node| {
                                match node {
                                    Node::Folder(name, child) => {
                                        log::info!("Folder => NAME: {name} CHILD: {child:?}");
                                        child
                                            .iter()
                                            .filter_map(|(_, node)| {
                                                if let Node::File((full_path, name)) = node {
                                                    Some(TodoItem::new(name, Category::UserScripts(full_path.to_string())))
                                                } else {
                                                    None
                                                }
                                            })
                                            .collect::<Vec<_>>()
                                    }
                                    _ => vec![TodoItem::default()],
                                }
                            })
                            .collect();

                            self.checklists.insert("User Scripts".to_string(), 
                                TodoList {
                                    name: "User Scripts".to_string(),
                                    state: ListState::default(),
                                    items: todo_items.clone()
                                }
                            );
                            self.popup_items.borrow_mut().insert(
                                "UserScripts".to_string(),
                                todo_items
                            );
                            self.check_for_scripts = false;
                        }
                        // todo_items.push(value);
                    },
                    database::schema::Node::File(file) => {
                        log::info!("file: {:?}", file);
                    },
                }
            }
        }
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