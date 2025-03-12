use crate::{tabs::scripts::{AntiVirusProduct, InstalledProgram, ScheduledTask, StartupProgram, TaskbarItem}, terminal_mode::{events::action_handler::WidgetId, styling::{CATPPUCCINTHEME, CYAN, DEEPPINK}, widgets::button::Button}, utilities::windows::windows_update::{WindowsUpdateEvent, WindowsUpdates}};
use egui::output;
use ratatui::{layout::Rect, widgets::{ListState, ScrollbarState}};
use std::{cell::RefCell, collections::HashMap, fmt::Display};
use checklist::{Category, Status, TodoItem, TodoList};
use crossbeam::channel::{Receiver, Sender};
use tui_scrollview::ScrollViewState;
use render::{Report, Reporter};

pub mod action_handler;
pub mod render;
pub mod checklist;
pub mod script_checks;

/* A Reporting System for each of these things like the AHS tuneup */

////////////////////////////////
// SCRIPTS TAB with Buttons
////////////////////////////////
/// Let's say we have a subcomponent called ScriptsTab
pub struct ScriptsTab<'a> {
    tuneup_btn: Button<'a>,
    qc_btn: Button<'a>,
    updates_btn: Button<'a>,
    prechecks_btn: Button<'a>,
    informational_btn: Button<'a>,
    run_btn: Button<'a>,

    reports: RefCell<Vec<Report>>, 
    current_reporter: RefCell<Reporter>,
    update_log_tx: Sender<WindowsUpdateEvent>,
    update_log_rx: Receiver<WindowsUpdateEvent>,
    path_size_tx: Sender<Vec<(String, String)>>,
    path_size_rx: Receiver<Vec<(String, String)>>,
    data_path_buttons: Vec<Button<'a>>,
    
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
    list_state: RefCell<ListState>,
    visible_height: RefCell<usize>,
    total_items: RefCell<usize>,
    /// For the scrollbar area
    scroll_area: RefCell<Option<Rect>>,
    /// (button ID, popup position)
    active_popup: RefCell<Option<(WidgetId, Rect)>>,
    frame_area: RefCell<Option<Rect>>,
    /// Tracks popup selection
    popup_list_state: RefCell<ListState>,
    popup_items: RefCell<HashMap<String, Vec<TodoItem>>>,
    /// (category, text) of the running script
    current_script: RefCell<Option<(Category, String)>>, 
    is_popup_open: RefCell<bool>,
    destination_directory: String,
    data_transfer_progress_tx: Sender<Vec<u8>>,
    data_transfer_progress_rx: Receiver<Vec<u8>>,
    source_directories: Vec<(String, String)>,
    progress: RefCell<Option<(f64, f64)>>
}

impl<'a> ScriptsTab<'a> {
    pub fn new() -> Self {
        let (update_log_tx, update_log_rx) = crossbeam::channel::unbounded();
        let (path_size_tx, path_size_rx) = crossbeam::channel::unbounded();
        let (data_transfer_progress_tx, data_transfer_progress_rx) = crossbeam::channel::unbounded();
        let mut checklists = HashMap::new();
        
        // Define checklists with categories
        checklists.insert(
            "Tuneup".to_string(),
            TodoList {
                name: "Tuneup".to_string(),
                state: ListState::default(),
                items: vec![
                    TodoItem::new("Disable Sleep / Hibernation", Category::Tuneup),
                    TodoItem::new("Install Windows Updates", Category::Tuneup),
                    TodoItem::new("Activate CPS", Category::Tuneup),
                    TodoItem::new("Activate SEB", Category::Tuneup),
                    TodoItem::new("Run Tron", Category::Tuneup),
                    TodoItem::new("Run SuperAntiSpyware Scan", Category::Tuneup),
                    TodoItem::new("Run Junkware Category", Category::JunkwareRemoval),
                ],
            },
        );

        checklists.insert(
            "QC".to_string(),
            TodoList {
                name: "QC".to_string(),
                state: ListState::default(),
                items: vec![
                    TodoItem::new("Data Transfer", Category::Qc),
                    TodoItem::new("Install LibreOffice", Category::Qc),
                    TodoItem::new("Disable Sleep / Hibernation", Category::Qc),
                    TodoItem::new("Disable proxy settings", Category::Qc),
                    TodoItem::new("Disable Notifications", Category::Qc),
                    TodoItem::new("Change SuperAntiSpyware settings", Category::Qc),
                    TodoItem::new("Disable Startup Apps", Category::Qc),
                    TodoItem::new("Unpin Copilot", Category::Qc),
                    TodoItem::new("Align Taskbar to left", Category::Qc),
                ],
            },
        );

        checklists.insert(
            "Junkware Removal".to_string(),
            TodoList {
                name: "Junkware Removal".to_string(),
                state: ListState::default(),
                items: vec![
                    TodoItem::new("OneLaunch", Category::JunkwareRemoval),
                    TodoItem::new("WebNavigator Browser", Category::JunkwareRemoval),
                    // TodoItem::new("ESET Security", Category::JunkwareRemoval),
                    TodoItem::new("Wavesor", Category::JunkwareRemoval),
                    TodoItem::new("Clear Browser", Category::JunkwareRemoval),
                    TodoItem::new("Shift Browser", Category::JunkwareRemoval),
                    TodoItem::new("Avast Browser", Category::JunkwareRemoval),
                    TodoItem::new("Mcaffee Safe", Category::JunkwareRemoval),
                    TodoItem::new("Driver Support", Category::JunkwareRemoval),
                    TodoItem::new("Winzip", Category::JunkwareRemoval),
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
                    TodoItem::new("If Webroot/SAS not installed, what AV is active?", Category::Informational),
                    TodoItem::new("Are there any pending Windows updates?", Category::Informational),
                    TodoItem::new("Is Windows Activated?", Category::Informational),
                    TodoItem::new("Is Hibernation/Sleep enabled?", Category::Informational),
                    TodoItem::new("Have there been any Blue Screens in the past 30 days?", Category::Informational),
                    TodoItem::new("When Was The Last Service Date?", Category::Informational),
                    TodoItem::new("Check Windows Updates", Category::Informational),
                    TodoItem::new("Windows Version", Category::Informational)
                        .set_pass_criteria("Windows 11")
                        .set_warning_criteria("Windows 10")
                        .set_error_criteria("Script Failed To Run"),
                ],
            },
        );
        // Sync popup_items with checklists
        let mut popup_items = HashMap::new();
        popup_items.insert(
            "Tuneup".to_string(),
            checklists.get("Tuneup").unwrap().items.clone(),
        );
        popup_items.insert(
            "Qc".to_string(),
            checklists.get("QC").unwrap().items.clone(),
        );
        popup_items.insert(
            "WindowsUpdates".to_string(),
            vec![
                TodoItem::new("Check Updates", Category::WindowsUpdates),
                TodoItem::new("Install Windows Updates", Category::WindowsUpdates),
            ],
        );
        popup_items.insert(
            "RunPrechecks".to_string(),
            vec![
                TodoItem::new("Run Prechecks", Category::RunPrechecks),
            ],
        );
        popup_items.insert(
            "Informational".to_string(),
            checklists.get("Informational").unwrap().items.clone(),
        );

        Self {
            tuneup_btn: Button::new("Tuneup =>", WidgetId("Tuneup".to_owned())).theme(CATPPUCCINTHEME),
            qc_btn: Button::new("Quality Check =>", WidgetId("Qc".to_owned())).theme(CATPPUCCINTHEME),
            updates_btn: Button::new("Windows Updates =>", WidgetId("WindowsUpdates".to_owned())).theme(CATPPUCCINTHEME),
            prechecks_btn: Button::new("Run Prechecks =>", WidgetId("RunPrechecks".to_owned())).theme(CATPPUCCINTHEME),
            informational_btn: Button::new("Informational =>", WidgetId("Informational".to_owned())).theme(CATPPUCCINTHEME),
            run_btn: Button::new("Run Selected", WidgetId("Run".to_owned())).theme(DEEPPINK),

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
            data_transfer_progress_tx, 
            data_transfer_progress_rx,

            checklists,
            windows_updates: WindowsUpdates::default(),
            report_scroll_state: RefCell::new(ScrollViewState::new()),
            list_state: RefCell::new(ListState::default()),
            list_scroll_state: RefCell::new(ScrollbarState::default()),
            checklist_area: RefCell::new(None),
            report_area: RefCell::new(None),
            visible_height: RefCell::new(0),
            total_items: RefCell::new(0),
            scroll_area: RefCell::new(None),
            active_popup: RefCell::new(None),
            frame_area: RefCell::new(None),
            popup_list_state: RefCell::new(ListState::default()),
            popup_items: RefCell::new(popup_items),
            current_script: RefCell::new(None),
            data_path_buttons: Vec::new(),
            is_popup_open: RefCell::new(false),
            destination_directory: String::new(),
            source_directories: Vec::new(),
            progress: RefCell::new(None),
            
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
            self.source_directories = path_info.clone();
            self.data_path_buttons.clear();
            for (path, size) in path_info {
                self.log_message(&format!("Path {:<5} Size: {:>5}", path.clone(), size.clone()));
                self.data_path_buttons.push(
                    Button::new(
                        format!(" {} | {} ", path.clone(), size.clone()), 
                        WidgetId(path)
                    )
                    .theme(CYAN)
                );
            }
            self.is_popup_open.replace(true);
        }

        if let Ok(data_transfer_progress) = self.data_transfer_progress_rx.try_recv() {
            let out = String::from_utf8(data_transfer_progress);
            log::info!("Robocopy Output: {out:?}");
            match out {
                Ok(output) => {
                    // Replace tabs with 4 spaces
                    let cleaned_output = output.trim_ascii().replace("\t", "    ");
                    self.log_message(cleaned_output);
                },
                Err(e) => self.log_message(format!("FromUTF8 Err: {e:?}")),
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

    fn get_selected_scripts(&self) -> Vec<TodoItem> {
        let popup_items = self.popup_items.borrow();
        popup_items
            .values()
            .flat_map(|items| {
                items.iter().filter(|item| item.status == Status::Completed).cloned()
            })
            .collect()
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
