use crate::{tabs::scripts::{AntiVirusProduct, InstalledProgram, ScheduledTask, StartupProgram, TaskbarItem}, terminal_mode::{events::action_handler::WidgetId, styling::{CATPPUCCINTHEME, DEEPPINK}, widgets::{button::Button, ButtonType}}, utilities::windows::WindowsUpdates};
use std::{cell::RefCell, collections::HashMap, fmt::Display};
use checklist::{Status, TodoItem, TodoList};
use crossbeam::channel::{Receiver, Sender};
use action_handler::WindowsUpdateEvent;
use ratatui::widgets::ListState;
use render::{Report, Reporter};
use tui_scrollview::ScrollViewState;

pub mod action_handler;
pub mod render;
pub mod checklist;
pub mod script_checks;

// macro_rules! log_message {
//     ($self:expr, $msg:literal, $($args:expr),*) => {
//         $self.log_message(format!($msg, $($args),*))
//     };
// }

/*
    A Reporting System for each of these things
    like the AHS tuneup 

    Checks:
    - Is SuperEasyBackup installed?
        - Is it Active?
    - Is Webroot installed?
        - Is it Active?
    - Is SuperAntiSpyware installed?
        - Is it Active?
        - Are there scheduled tasks for it?
    - If Webroot and/or SuperAntiSpyware is not installed:
        - What other (if any) antivirus is installed, and is it active?
    - Are there any windows updates left?
    - Is Windows Activated?
    - is Sleep enabled?
    - is Hibernation enabled?

    QC ONLY CHECKS:
    - Do we need to do a data transfer?
    - Do we need to install LibreOffice?

    Actionable:
    - Disable Sleep / Hibernation
    - Disable proxy settings
    - Disable Notifications
    - Change Superantispyware settings
    - Disabling Startup Apps
    - Unpin copilot
    - Align Taskbar to left 
    - 

    - Uninstall onelaunch / driver updater / etc
    - if i exec the uninstall string for onelaunch, it works using the /silent flag
    Plans for UI
    - 
*/


#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScriptsTabView {
    #[default]
    Main,
    Antivirus,
    StartupItems,
    InstalledPrograms,
    ScheduledTasks,
    TaskbarItems,
}

// #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
// pub enum ChecklistPageState {
//     #[default]

// }

////////////////////////////////
// SCRIPTS TAB with Buttons
////////////////////////////////
/// Let's say we have a subcomponent called ScriptsTab
pub struct ScriptsTab<'a> {
    tuneup_btn: Button<'a>,
    qc_btn: Button<'a>,
    updates_btn: Button<'a>,
    prechecks_btn: Button<'a>,

    // Buttons for retrieving tab-specific data
    get_antivirus_btn: Button<'a>,
    get_installed_programs_btn: Button<'a>,
    get_startup_items_btn: Button<'a>,
    get_scheduled_tasks_btn: Button<'a>,
    get_taskbar_items_btn: Button<'a>,

    reports: RefCell<Vec<Report>>, 
    current_reporter: RefCell<Reporter>,
    update_log_tx: Sender<WindowsUpdateEvent>,
    update_log_rx: Receiver<WindowsUpdateEvent>,
    path_size_tx: Sender<Vec<(String, String)>>,
    path_size_rx: Receiver<Vec<(String, String)>>,
    current_tab: RefCell<ScriptsTabView>,
    tab_buttons: Vec<(WidgetId, Button<'a>)>,

    // Data for each tab
    /// Antivirus tab
    antivirus_products: Vec<AntiVirusProduct>,
    /// Installed Programs tab
    installed_programs: Vec<InstalledProgram>,
    /// Startup Items tab
    startup_programs: Vec<StartupProgram>,
    /// Scheduled Tasks tab
    scheduled_tasks: Vec<ScheduledTask>,
    /// Taskbar Items tab
    taskbar_items: Vec<TaskbarItem>,

    /// Multipurpose checklist
    checklists: HashMap<String, TodoList>,
    /// Stores the latest retrieved Windows Updates info
    windows_updates: WindowsUpdates,
    // Scroll states for each tab
    // antivirus_scroll: ScrollViewState,
    // installed_programs_scroll: ScrollViewState,
    // startup_items_scroll: ScrollViewState,
    // scheduled_tasks_scroll: ScrollViewState,
    // taskbar_items_scroll: ScrollViewState,
    // report_scroll: ScrollViewState,
    // checklist_
    report_scroll_state: RefCell<ScrollViewState>,
    // script_page_state: RefCell<ChecklistPageState>,
    list_state: RefCell<ListState>,
}

impl<'a> ScriptsTab<'a> {
    pub fn new() -> Self {
        let (update_log_tx, update_log_rx) = crossbeam::channel::unbounded();
        let (path_size_tx, path_size_rx) = crossbeam::channel::unbounded();

        let mut checklists = HashMap::new();
        
        checklists.insert(
            "Prechecks".to_string(),
            TodoList {
                name: "Prechecks".to_string(),
                state: ListState::default(),
                items: vec![
                    TodoItem::new("Is SuperEasyBackup installed?"),
                    TodoItem::new("Is it Active?"),
                    TodoItem::new("Is Webroot installed?"),
                    TodoItem::new("Is it Active?"),
                    TodoItem::new("Is SuperAntiSpyware installed?"),
                    TodoItem::new("Is it Active?"),
                    TodoItem::new("Are there scheduled tasks for it?"),
                    TodoItem::new("If Webroot/SAS not installed, what AV is active?"),
                    TodoItem::new("Are there any pending Windows updates?"),
                    TodoItem::new("Is Windows Activated?"),
                    TodoItem::new("Is Sleep enabled?"),
                    TodoItem::new("Is Hibernation enabled?"),
                ],
            },
        );

        checklists.insert(
            "QC Only Checks".to_string(),
            TodoList {
                name: "QC Only Checks".to_string(),
                state: ListState::default(),
                items: vec![
                    TodoItem::new("Do we need to do a data transfer?"),
                    TodoItem::new("Do we need to install LibreOffice?"),
                ],
            },
        );

        checklists.insert(
            "Actionable".to_string(),
            TodoList {
                name: "Actionable".to_string(),
                state: ListState::default(),
                items: vec![
                    TodoItem::new("Disable Sleep / Hibernation"),
                    TodoItem::new("Disable proxy settings"),
                    TodoItem::new("Disable Notifications"),
                    TodoItem::new("Change SuperAntiSpyware settings"),
                    TodoItem::new("Disable Startup Apps"),
                    TodoItem::new("Unpin Copilot"),
                    TodoItem::new("Align Taskbar to left"),
                ],
            },
        );

        let tab_buttons = vec![
            (WidgetId("Main".to_owned()), Button::new("Main", WidgetId("Main".to_owned())).theme(DEEPPINK)),
            (WidgetId("Antivirus".to_owned()), Button::new("Antivirus", WidgetId("Antivirus".to_owned())).theme(CATPPUCCINTHEME)),
            (WidgetId("StartupItems".to_owned()), Button::new("Startup Items", WidgetId("StartupItems".to_owned())).theme(CATPPUCCINTHEME)),
            (WidgetId("InstalledPrograms".to_owned()), Button::new("Installed Programs", WidgetId("InstalledPrograms".to_owned())).theme(CATPPUCCINTHEME)),
            (WidgetId("ScheduledTasks".to_owned()), Button::new("Scheduled Tasks", WidgetId("ScheduledTasks".to_owned())).theme(CATPPUCCINTHEME)),
            (WidgetId("TaskbarItems".to_owned()), Button::new("Taskbar Items", WidgetId("TaskbarItems".to_owned())).theme(CATPPUCCINTHEME)),
        ];

        Self {
            tuneup_btn: Button::new("Tuneup", WidgetId("Tuneup".to_owned())).theme(CATPPUCCINTHEME),
            qc_btn: Button::new("Quality Check", WidgetId("Qc".to_owned())).theme(CATPPUCCINTHEME),
            updates_btn: Button::new("Windows Updates", WidgetId("WindowsUpdates".to_owned())).theme(CATPPUCCINTHEME),
            prechecks_btn: Button::new("Run Prechecks", WidgetId("RunPrechecks".to_owned())).theme(CATPPUCCINTHEME),

            // Initialize buttons for retrieving data
            get_antivirus_btn: Button::new("Get AV Info", WidgetId("GetAntivirus".to_owned())).theme(CATPPUCCINTHEME),
            get_installed_programs_btn: Button::new("Get Installed Programs", WidgetId("GetInstalledPrograms".to_owned())).theme(CATPPUCCINTHEME),
            get_startup_items_btn: Button::new("Get Startup Items", WidgetId("GetStartupItems".to_owned())).theme(CATPPUCCINTHEME),
            get_scheduled_tasks_btn: Button::new("Get Scheduled Tasks", WidgetId("GetScheduledTasks".to_owned())).theme(CATPPUCCINTHEME),
            get_taskbar_items_btn: Button::new("Get Taskbar Items", WidgetId("GetTaskbarItems".to_owned())).theme(CATPPUCCINTHEME),
            
            tab_buttons,
            
            antivirus_products: Vec::new(),
            installed_programs: Vec::new(),
            startup_programs: Vec::new(),
            scheduled_tasks: Vec::new(),
            taskbar_items: Vec::new(),

            reports: RefCell::new(vec![]),
            current_reporter: RefCell::new(Reporter::Unknown),
            update_log_tx, 
            update_log_rx,
            path_size_tx, 
            path_size_rx,
            current_tab: RefCell::new(ScriptsTabView::default()),

            checklists,
            windows_updates: WindowsUpdates::default(),
            report_scroll_state: RefCell::new(ScrollViewState::new()),
            list_state: RefCell::new(ListState::default()),
            // Initialize scroll states
            // antivirus_scroll: ScrollViewState::default(),
            // installed_programs_scroll: ScrollViewState::default(),
            // startup_items_scroll: ScrollViewState::default(),
            // scheduled_tasks_scroll: ScrollViewState::default(),
            // taskbar_items_scroll: ScrollViewState::default(),
            // report_scroll: ScrollViewState::default(),
        }
    }

    /// Logs a message under the current `Reporter`
    pub fn log_message(&self, msg: impl Display) {
        let reporter = self.current_reporter.borrow().clone();
        let log_entry = Report {
            reporter,
            msg: msg.to_string(),
        };
        self.reports.borrow_mut().push(log_entry); // ✅ Store log
    }

    pub fn receive(&mut self) {

        if let Ok(path_info) = self.path_size_rx.try_recv() {
            for (path, size) in path_info {
                self.log_message(&format!("Path {path:<10} Size: {size:>10}"));
            }
        }

        // listen for Windows Update logs & results
        while let Ok(event) = self.update_log_rx.try_recv() {
            match event {
                WindowsUpdateEvent::UpdateLogs(log) => self.log_message(&log),
                WindowsUpdateEvent::ReturnedUpdates(windows_updates) => {
                    self.log_message(&serde_json::to_string_pretty(&windows_updates).unwrap());
                    self.windows_updates = windows_updates;
                },
            }
        }
    }

    /// Sets the currently active tab based on button state
    pub fn update_selected_tab(&self) {
        for (widget_id, button) in self.tab_buttons.iter() {
            if button.is_active() {
                match widget_id.0.as_str() {
                    "Antivirus" => self.current_tab.replace(ScriptsTabView::Antivirus),
                    "StartupItems" => self.current_tab.replace(ScriptsTabView::StartupItems),
                    "InstalledPrograms" => self.current_tab.replace(ScriptsTabView::InstalledPrograms),
                    "ScheduledTasks" => self.current_tab.replace(ScriptsTabView::ScheduledTasks),
                    "TaskbarItems" => self.current_tab.replace(ScriptsTabView::TaskbarItems),
                    "Main" => self.current_tab.replace(ScriptsTabView::Main),
                    _ => self.current_tab.replace(ScriptsTabView::default())
                };
            }
        }
    }

    fn _mark_task_complete(&mut self, checklist_name: &str, task_name: &str) {
        if let Some(list) = self.checklists.get_mut(checklist_name) {
            if let Some(task) = list.items.iter_mut().find(|t| t.text == task_name) {
                task.status = Status::Completed;
            }
        }
    }
    
}
