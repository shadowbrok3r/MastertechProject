use crate::{tabs::scripts::{AntiVirusProduct, InstalledProgram, ScheduledTask, StartupProgram, TaskbarItem}, terminal_mode::{events::action_handler::WidgetId, styling::CATPPUCCINTHEME, widgets::button::Button}, utilities::windows::WindowsUpdates};
use ratatui::{layout::Rect, widgets::{ListState, ScrollbarState}};
use std::{cell::RefCell, collections::HashMap, fmt::Display};
use checklist::{Status, TodoItem, TodoList};
use crossbeam::channel::{Receiver, Sender};
use action_handler::WindowsUpdateEvent;
use tui_scrollview::ScrollViewState;
use render::{Report, Reporter};

pub mod action_handler;
pub mod render;
pub mod checklist;
pub mod script_checks;

/*
    A Reporting System for each of these things
    like the AHS tuneup 

*/

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

    checklist_area: RefCell<Option<Rect>>,
    report_area: RefCell<Option<Rect>>,
    report_scroll_state: RefCell<ScrollViewState>,
    list_scroll_state: RefCell<ScrollbarState>,
    // script_page_state: RefCell<ChecklistPageState>,
    list_state: RefCell<ListState>,
    visible_height: RefCell<usize>,
    total_items: RefCell<usize>,
    scroll_area: RefCell<Option<Rect>>, // For the scrollbar area
    // New fields for popup
    active_popup: RefCell<Option<(WidgetId, Rect)>>, // (button ID, popup position)
    frame_area: RefCell<Option<Rect>>,
    popup_highlighted_idx: RefCell<Option<usize>>, // Tracks highlighted Span index
}

impl<'a> ScriptsTab<'a> {
    pub fn new() -> Self {
        let (update_log_tx, update_log_rx) = crossbeam::channel::unbounded();
        let (path_size_tx, path_size_rx) = crossbeam::channel::unbounded();

        let mut checklists = HashMap::new();
        
        checklists.insert(
            "Informational".to_string(),
            TodoList {
                name: "Informational".to_string(),
                state: ListState::default(),
                items: vec![
                    TodoItem::new("Is SuperEasyBackup installed?", None).set_pass_criteria("Installed and active").set_warning_criteria("Not installed OR its not active").set_error_criteria("Script Failed To Run"),
                    TodoItem::new("Is Webroot installed?", None).set_pass_criteria("Installed and active").set_warning_criteria("Not installed OR its not active").set_error_criteria("Script Failed To Run"),
                    TodoItem::new("Is SuperAntiSpyware installed?", None).set_pass_criteria("Installed and active").set_warning_criteria("Not installed OR its not active").set_error_criteria("Script Failed To Run"),
                    TodoItem::new("Are there scheduled tasks for it?", None).set_pass_criteria("").set_warning_criteria("").set_fail_criteria(""),
                    TodoItem::new("If Webroot/SAS not installed, what AV is active?", None).set_pass_criteria("").set_warning_criteria("").set_fail_criteria(""),
                    TodoItem::new("Are there any pending Windows updates?", None).set_pass_criteria("").set_warning_criteria("").set_fail_criteria(""),
                    TodoItem::new("Is Windows Activated?", None).set_pass_criteria("").set_warning_criteria("").set_fail_criteria(""),
                    TodoItem::new("Is Sleep enabled?", None).set_pass_criteria("").set_warning_criteria("").set_fail_criteria(""),
                    TodoItem::new("Is Hibernation enabled?", None).set_pass_criteria("").set_warning_criteria("").set_fail_criteria(""),
                    TodoItem::new("Have there been any Blue Screens in the past 30 days?", None).set_pass_criteria("").set_warning_criteria("").set_fail_criteria(""),
                    TodoItem::new("When Was The Last Service Date?", None).set_pass_criteria("").set_warning_criteria("").set_fail_criteria(""),
                    TodoItem::new("Windows Version", None).set_pass_criteria("Windows 11").set_warning_criteria("Windows 10").set_error_criteria("Script Failed To Run").set_fail_criteria(""), 
                ],
            },
        );

        checklists.insert(
            "Tuneup".to_string(),
            TodoList {
                name: "Tuneup".to_string(),
                state: ListState::default(),
                items: vec![
                    TodoItem::new("Disable Sleep / Hibernation", None),
                    TodoItem::new("Run Windows Updates", None),
                    TodoItem::new("Activate CPS", None),
                    TodoItem::new("Activate SEB", None),
                    TodoItem::new("Run Tron", None),
                    TodoItem::new("Run SuperAntiSpyware Scan", None),
                    TodoItem::new("Run Junkware Category", None),
                ],
            },
        );

        checklists.insert(
            "Junkware Removal".to_string(),
            TodoList {
                name: "Junkware Removal".to_string(),
                state: ListState::default(),
                items: vec![
                    TodoItem::new("OneLaunch", None),
                    TodoItem::new("WebNavigatorBrowser", None),
                    TodoItem::new("ESET Security", None),
                    TodoItem::new("Wavesor", None),
                    TodoItem::new("ClearBrowser", None),
                    TodoItem::new("ShiftBrowser", None),
                    TodoItem::new("AvastBrowser", None),
                    TodoItem::new("McaffeeSafe", None),
                    TodoItem::new("DriverSupport", None),
                    TodoItem::new("Winzip", None),
                ],
            },
        );

        checklists.insert(
            "QC".to_string(),
            TodoList {
                name: "QC".to_string(),
                state: ListState::default(),
                items: vec![
                    TodoItem::new("Data Transfer", None),
                    TodoItem::new("Install LibreOffice", None),
                    TodoItem::new("Disable Sleep / Hibernation", None).set_pass_criteria("".to_string()).set_warning_criteria("".to_string()).set_fail_criteria("".to_string()),
                    TodoItem::new("Disable proxy settings", None).set_pass_criteria("".to_string()).set_warning_criteria("".to_string()).set_fail_criteria("".to_string()),
                    TodoItem::new("Disable Notifications", None).set_pass_criteria("".to_string()).set_warning_criteria("".to_string()).set_fail_criteria("".to_string()),
                    TodoItem::new("Change SuperAntiSpyware settings", None).set_pass_criteria("".to_string()).set_warning_criteria("".to_string()).set_fail_criteria("".to_string()),
                    TodoItem::new("Disable Startup Apps", None).set_pass_criteria("".to_string()).set_warning_criteria("".to_string()).set_fail_criteria("".to_string()),
                    TodoItem::new("Unpin Copilot", None).set_pass_criteria("".to_string()).set_warning_criteria("".to_string()).set_fail_criteria("".to_string()),
                    TodoItem::new("Align Taskbar to left", None).set_pass_criteria("".to_string()).set_warning_criteria("".to_string()).set_fail_criteria("".to_string()),
                ],
            },
        );

        Self {
            tuneup_btn: Button::new("Tuneup", WidgetId("Tuneup".to_owned())).theme(CATPPUCCINTHEME),
            qc_btn: Button::new("Quality Check", WidgetId("Qc".to_owned())).theme(CATPPUCCINTHEME),
            updates_btn: Button::new("Windows Updates", WidgetId("WindowsUpdates".to_owned())).theme(CATPPUCCINTHEME),
            prechecks_btn: Button::new("Run Prechecks", WidgetId("RunPrechecks".to_owned())).theme(CATPPUCCINTHEME),
            get_antivirus_btn: Button::new("Get AV Info", WidgetId("GetAntivirus".to_owned())).theme(CATPPUCCINTHEME),
            get_installed_programs_btn: Button::new("Get Installed Programs", WidgetId("GetInstalledPrograms".to_owned())).theme(CATPPUCCINTHEME),
            get_startup_items_btn: Button::new("Get Startup Items", WidgetId("GetStartupItems".to_owned())).theme(CATPPUCCINTHEME),
            get_scheduled_tasks_btn: Button::new("Get Scheduled Tasks", WidgetId("GetScheduledTasks".to_owned())).theme(CATPPUCCINTHEME),
            get_taskbar_items_btn: Button::new("Get Taskbar Items", WidgetId("GetTaskbarItems".to_owned())).theme(CATPPUCCINTHEME),
            
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

            checklists,
            windows_updates: WindowsUpdates::default(),
            report_scroll_state: RefCell::new(ScrollViewState::new()),
            list_state: RefCell::new(ListState::default()),
            list_scroll_state: RefCell::new(ScrollbarState::default()), // Renamed
            checklist_area: RefCell::new(None),
            report_area: RefCell::new(None),
            visible_height: RefCell::new(0),
            total_items: RefCell::new(0),
            scroll_area: RefCell::new(None),
            active_popup: RefCell::new(None),
            frame_area: RefCell::new(None),
            popup_highlighted_idx: RefCell::new(None),
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

    // /// Sets the currently active tab based on button state
    // pub fn update_selected_tab(&self) {
    //     for (widget_id, button) in self.tab_buttons.iter() {
    //         if button.is_active() {
    //             match widget_id.0.as_str() {
    //                 "Antivirus" => self.current_tab.replace(ScriptsTabView::Antivirus),
    //                 "StartupItems" => self.current_tab.replace(ScriptsTabView::StartupItems),
    //                 "InstalledPrograms" => self.current_tab.replace(ScriptsTabView::InstalledPrograms),
    //                 "ScheduledTasks" => self.current_tab.replace(ScriptsTabView::ScheduledTasks),
    //                 "TaskbarItems" => self.current_tab.replace(ScriptsTabView::TaskbarItems),
    //                 "Main" => self.current_tab.replace(ScriptsTabView::Main),
    //                 _ => self.current_tab.replace(ScriptsTabView::default())
    //             };
    //         }
    //     }
    // }

    fn _mark_task_complete(&mut self, checklist_name: &str, task_name: &str) {
        if let Some(list) = self.checklists.get_mut(checklist_name) {
            if let Some(task) = list.items.iter_mut().find(|t| t.text == task_name) {
                task.status = Status::Completed;
            }
        }
    }
    
}
