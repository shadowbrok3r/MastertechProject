use crate::{tabs::scripts::{AntiVirusProduct, InstalledProgram, ScheduledTask, StartupProgram, TaskbarItem}, terminal_mode::{context::TerminalContext, events::action_handler::WidgetId, styling::{CATPPUCCINTHEME, CYAN, DEEPPINK}, widgets::{button::Button, input_field::InputField}}, utilities::windows::windows_update::{WindowsUpdateEvent, WindowsUpdates}};
use ratatui::{layout::{Position, Rect}, widgets::{ListState, ScrollbarState}};
use reqwest::Client;
use std::{cell::RefCell, collections::HashMap, fmt::Display, sync::{Arc, Mutex}};
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
#[derive(Debug)]
pub struct ScriptsTab<'a> {
    service_number_field: InputField<'a>,
    tuneup_btn: Button<'a>,
    qc_btn: Button<'a>,
    updates_btn: Button<'a>,
    prechecks_btn: Button<'a>,
    informational_btn: Button<'a>,
    run_btn: Button<'a>,
    data_path_buttons: Vec<Button<'a>>,

    reports: RefCell<Vec<Report>>, 
    current_reporter: RefCell<Reporter>,
    update_log_tx: Sender<WindowsUpdateEvent>,
    update_log_rx: Receiver<WindowsUpdateEvent>,
    path_size_tx: Sender<Vec<(String, String)>>,
    path_size_rx: Receiver<Vec<(String, String)>>,
    progress_rx: Receiver<(u64, u64)>, // Receive progress updates
    progress_tx: Sender<(u64, u64)>, // Receive progress updates
    progress: RefCell<Option<(u64, u64)>>,
    
    service_number: String,
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
    
    has_scrolled_manually: RefCell<bool>,
    init: RefCell<bool>,
    client: Client,
    customer_email: String,
    ctx: Arc<Mutex<TerminalContext>>,
}

impl<'a> ScriptsTab<'a> {
    pub fn new(client: Client, ctx: Arc<Mutex<TerminalContext>>) -> Self {
        let (update_log_tx, update_log_rx) = crossbeam::channel::unbounded();
        let (path_size_tx, path_size_rx) = crossbeam::channel::unbounded();
        let (data_transfer_progress_tx, data_transfer_progress_rx) = crossbeam::channel::unbounded();
        let (progress_tx, progress_rx) = crossbeam::channel::unbounded();
        let mut checklists = HashMap::new();
        
        // Define checklists with categories
        checklists.insert(
            "Tuneup".to_string(),
            TodoList {
                name: "Tuneup".to_string(),
                state: ListState::default(),
                items: vec![
                    TodoItem::new("Disable Sleep / Hibernation", Category::Tuneup),
                    TodoItem::new("Install Windows Updates", Category::WindowsUpdates),
                    TodoItem::new("Activate CPS", Category::Tuneup),
                    TodoItem::new("Activate SEB", Category::Tuneup),
                    TodoItem::new("Run Tron", Category::Tuneup),
                    TodoItem::new("Run SuperAntiSpyware Scan", Category::Tuneup),
                    TodoItem::new("Run Webroot Scan", Category::Tuneup),
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
            service_number_field: InputField::new("Service #", WidgetId("ServiceNumberScriptsPage".to_string())),
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
            service_number: String::new(),
            update_log_tx, 
            update_log_rx,
            path_size_tx, 
            path_size_rx,
            data_transfer_progress_tx, 
            data_transfer_progress_rx, 
            progress_tx, progress_rx,

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
            has_scrolled_manually: RefCell::new(false),
            init: RefCell::new(true),
            client,
            customer_email: String::new(),
            ctx,
        }
    }

    /// Logs a message under the current `Reporter`
    pub fn log_message(&self, msg: impl Display) {
        let reporter = self.current_reporter.borrow().clone();
        let log_lines = self.reports.borrow().len() as u16;
        let log_entry = Report {
            reporter,
            msg: msg.to_string(),
        };
        self.reports.borrow_mut().push(log_entry); // ✅ Store log
        let mut scroll_state = self.report_scroll_state.borrow_mut();
        let scroll_x = scroll_state.offset().x;
        let visible_height = self.report_area.borrow().map_or(0, |area| area.height.saturating_sub(2));

        if !*self.has_scrolled_manually.borrow() && log_lines > visible_height {
            let scroll_y = log_lines.saturating_sub(visible_height);
            scroll_state.set_offset(Position { x: scroll_x, y: scroll_y });
        }
        *self.has_scrolled_manually.borrow_mut() = false;
    }

    pub fn receive(&mut self) {
        if let Ok(progress) = self.progress_rx.try_recv() {
            self.progress.replace(Some(progress));
        }

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
        if let Ok(event) = self.update_log_rx.try_recv() {
            match event {
                WindowsUpdateEvent::UpdateLogs(log) => self.log_message(&log),
                WindowsUpdateEvent::ReturnedUpdates(windows_updates) => {
                    self.log_message(&format!("{windows_updates:#?}"));
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









pub const TEST_TEXT: [&str; 120] = [
    r#"23:15:09 [INFO] STARTING TERM MODE"#,
    r#"23:15:09 [INFO] Hooking StdOut"#,
    r#"23:15:09 [INFO] Creating Crossterm backend"#,
    r#"23:15:09 [INFO] Creating Terminal"#,
    r#"23:15:09 [INFO] Filesystem -> get_computer_data -> Getting sysinfo"#,
    r#"23:15:09 [INFO] Filesystem -> get_computer_data -> Pulling Drive information"#,
    r#"23:15:09 [INFO] Filesystem -> get_computer_data -> DriveData: "M.2-WDBlue""#,
    r#"23:15:09 [INFO] Filesystem -> get_computer_data -> DriveData: "HDD-Games""#,
    r#"23:15:09 [INFO] Filesystem -> get_computer_data -> DriveData: """#,
    r#"23:15:09 [INFO] Filesystem -> get_computer_data -> DriveData: "darkmage79@gmail.com - Google...""#,
    r#"23:15:09 [INFO] Filesystem -> get_computer_data -> DriveData: "logan@kingsofalchemy.com - Go...""#,
    r#"23:15:09 [ERROR] Error Pulling SEB info: "The system cannot find the path specified. (os error 3)""#,
    r#"23:15:09 [INFO] Filesystem -> get_computer_data -> pulling GPU"#,
    r#"23:15:09 [INFO] Filesystem -> get_computer_data -> Process: Ok(Output { status: ExitStatus(ExitStatus(0)), stdout: "NVIDIA GeForce RTX 3090\r\n", stderr: "" })"#,
    r#"23:15:09 [INFO] Filesystem -> get_computer_data -> x: [78, 86, 73, 68, 73, 65, 32, 71, 101, 70, 111, 114, 99, 101, 32, 82, 84, 88, 32, 51, 48, 57, 48, 13, 10]"#,
    r#"23:15:09 [INFO] Filesystem -> get_computer_data -> GPU: "NVIDIA GeForce RTX 3090\r\n""#,
    r#"23:15:09 [INFO] Filesystem -> get_computer_data -> Pulling CPU"#,
    r#"23:15:09 [INFO] Filesystem -> get_computer_data -> Pulling RAM"#,
    r#"23:15:09 [INFO] Filesystem -> get_computer_data -> Pulling OS"#,
    r#"23:15:09 [INFO] Filesystem -> get_computer_data -> Pulling Hostname"#,
    r#"23:15:09 [INFO] Filesystem -> generate_client_id -> combined: ShadowbrokerPC-AMD Ryzen 9 5950X 16-Core Processor-AMD64 Family 25 Model 33 Stepping 0, AuthenticAMD"#,
    r#"23:15:09 [INFO] Filesystem -> generate_client_id -> hex_string: bb6064d0dce336cb2731c7a708ddf1fcd764f2f66b4832b398197f3cfad76d8b"#,
    r#"23:15:09 [INFO] Filesystem -> get_computer_data -> ID: ShadowbrokerPC:bb6064d0d"#,
    r#"23:15:09 [INFO] Filesystem -> get_computer_data -> RecordID: Thing { tb: "computer", id: String("ShadowbrokerPC:bb6064d0d") }"#,
    r#"23:15:09 [INFO] Computer Data: ComputerData { id: Thing { tb: "computer", id: String("ShadowbrokerPC:bb6064d0d") }, customer: None, seb_info: None, hostname: "ShadowbrokerPC", operating_system: "Windows 11 Pro", cpu: "AMD Ryzen 9 5950X 16-Core Processor", gpu: "NVIDIA GeForce RTX 3090", ram: "64", drives: [DriveData { drive_letter: "D:\\", drive_type: "SSD", total_size: "1,862", space_left: "593" }, DriveData { drive_letter: "E:\\", drive_type: "HDD", total_size: "5,589", space_left: "1,797" }, DriveData { drive_letter: "C:\\", drive_type: "SSD", total_size: "1,862", space_left: "798" }, DriveData { drive_letter: "G:\\", drive_type: "Unknown(-1)", total_size: "1,862", space_left: "132" }, DriveData { drive_letter: "M:\\", drive_type: "Unknown(-1)", total_size: "1,862", space_left: "758" }], device_name: None, device_mfg: None, device_model: None, device_serial: None }"#,
    r#"23:15:09 [INFO] Retrieving sysinfo"#,
    r#"23:15:10 [INFO] Running app"#,
    r#"23:15:10 [INFO] Filesystem -> generate_client_id -> combined: ShadowbrokerPC-AMD Ryzen 9 5950X 16-Core Processor-AMD64 Family 25 Model 33 Stepping 0, AuthenticAMD"#,
    r#"23:15:10 [INFO] Filesystem -> generate_client_id -> hex_string: bb6064d0dce336cb2731c7a708ddf1fcd764f2f66b4832b398197f3cfad76d8b"#,
    r#"23:15:10 [INFO] First Run Results: Ok(())"#,
    r#"23:15:10 [INFO] Running splash"#,
    r#"23:15:10 [INFO] Running splash 2"#,
    r#"23:15:10 [INFO] Entering main loop"#,
    r#"23:15:14 [INFO] DB: Err(There was an error processing a remote WS request: IO error: No connection could be made because the target machine actively refused it. (os error 10061)"#,
    r#"Caused by:"#,
    r#"    There was an error processing a remote WS request: IO error: No connection could be made because the target machine actively refused it. (os error 10061))"#,
    r#"23:15:46 [INFO] Button: Left"#,
    r#"23:15:46 [INFO] Button: Left"#,
    r#"widget: WidgetId("Scripts")"#,
    r#"23:15:46 [INFO] Button: Left"#,
    r#"23:15:09 [INFO] 231123 STARTING TERM MODE"#,
    r#"23:15:09 [INFO] 231123 Hooking StdOut"#,
    r#"23:15:09 [INFO] 231123 Creating Crossterm backend"#,
    r#"23:15:09 [INFO] 231123 Creating Terminal"#,
    r#"23:15:09 [INFO] 231123 Filesystem -> get_computer_data -> Getting sysinfo"#,
    r#"23:15:09 [INFO] 231123 Filesystem -> get_computer_data -> Pulling Drive information"#,
    r#"23:15:09 [INFO] 231123 Filesystem -> get_computer_data -> DriveData: "M.2-WDBlue""#,
    r#"23:15:09 [INFO] 231123 Filesystem -> get_computer_data -> DriveData: "HDD-Games""#,
    r#"23:15:09 [INFO] 231123 Filesystem -> get_computer_data -> DriveData: """#,
    r#"23:15:09 [INFO] 231123 Filesystem -> get_computer_data -> DriveData: "darkmage79@gmail.com - Google...""#,
    r#"23:15:09 [INFO] 231123 Filesystem -> get_computer_data -> DriveData: "logan@kingsofalchemy.com - Go...""#,
    r#"23:15:09 [ERROR]231123  Error Pulling SEB info: "The system cannot find the path specified. (os error 3)""#,
    r#"23:15:09 [INFO] 231123 Filesystem -> get_computer_data -> pulling GPU"#,
    r#"23:15:09 [INFO] 231123 Filesystem -> get_computer_data -> Process: Ok(Output { status: ExitStatus(ExitStatus(0)), stdout: "NVIDIA GeForce RTX 3090\r\n", stderr: "" })"#,
    r#"23:15:09 [INFO] 231123 Filesystem -> get_computer_data -> x: [78, 86, 73, 68, 73, 65, 32, 71, 101, 70, 111, 114, 99, 101, 32, 82, 84, 88, 32, 51, 48, 57, 48, 13, 10]"#,
    r#"23:15:09 [INFO] 231123 Filesystem -> get_computer_data -> GPU: "NVIDIA GeForce RTX 3090\r\n""#,
    r#"23:15:09 [INFO] 231123 Filesystem -> get_computer_data -> Pulling CPU"#,
    r#"23:15:09 [INFO] 231123 Filesystem -> get_computer_data -> Pulling RAM"#,
    r#"23:15:09 [INFO] 231123 Filesystem -> get_computer_data -> Pulling OS"#,
    r#"23:15:09 [INFO] 231123 Filesystem -> get_computer_data -> Pulling Hostname"#,
    r#"23:15:09 [INFO] 231123 Filesystem -> generate_client_id -> combined: ShadowbrokerPC-AMD Ryzen 9 5950X 16-Core Processor-AMD64 Family 25 Model 33 Stepping 0, AuthenticAMD"#,
    r#"23:15:09 [INFO] 231123 Filesystem -> generate_client_id -> hex_string: bb6064d0dce336cb2731c7a708ddf1fcd764f2f66b4832b398197f3cfad76d8b"#,
    r#"23:15:09 [INFO] 231123 Filesystem -> get_computer_data -> ID: ShadowbrokerPC:bb6064d0d"#,
    r#"23:15:09 [INFO] 231123 Filesystem -> get_computer_data -> RecordID: Thing { tb: "computer", id: String("ShadowbrokerPC:bb6064d0d") }"#,
    r#"23:15:09 [INFO] 231123 Computer Data: ComputerData { id: Thing { tb: "computer", id: String("ShadowbrokerPC:bb6064d0d") }, customer: None, seb_info: None, hostname: "ShadowbrokerPC", operating_system: "Windows 11 Pro", cpu: "AMD Ryzen 9 5950X 16-Core Processor", gpu: "NVIDIA GeForce RTX 3090", ram: "64", drives: [DriveData { drive_letter: "D:\\", drive_type: "SSD", total_size: "1,862", space_left: "593" }, DriveData { drive_letter: "E:\\", drive_type: "HDD", total_size: "5,589", space_left: "1,797" }, DriveData { drive_letter: "C:\\", drive_type: "SSD", total_size: "1,862", space_left: "798" }, DriveData { drive_letter: "G:\\", drive_type: "Unknown(-1)", total_size: "1,862", space_left: "132" }, DriveData { drive_letter: "M:\\", drive_type: "Unknown(-1)", total_size: "1,862", space_left: "758" }], device_name: None, device_mfg: None, device_model: None, device_serial: None }"#,
    r#"23:15:09 [INFO] 231123 Retrieving sysinfo"#,
    r#"23:15:10 [INFO] 231123 Running app"#,
    r#"23:15:10 [INFO] 231123 Filesystem -> generate_client_id -> combined: ShadowbrokerPC-AMD Ryzen 9 5950X 16-Core Processor-AMD64 Family 25 Model 33 Stepping 0, AuthenticAMD"#,
    r#"23:15:10 [INFO] 231123 Filesystem -> generate_client_id -> hex_string: bb6064d0dce336cb2731c7a708ddf1fcd764f2f66b4832b398197f3cfad76d8b"#,
    r#"23:15:10 [INFO] 231123 First Run Results: Ok(())"#,
    r#"23:15:10 [INFO] 231123 Running splash"#,
    r#"23:15:10 [INFO] 231123 Running splash 2"#,
    r#"23:15:10 [INFO] 231123 Entering main loop"#,
    r#"23:15:14 [INFO] 231123 DB: Err(There was an error processing a remote WS request: IO error: No connection could be made because the target machine actively refused it. (os error 10061)"#,
    r#"Caused by:"#,
    r#"    There was an error processing a remote WS request: IO error: No connection could be made because the target machine actively refused it. (os error 10061))"#,
    r#"23:15:46 [INFO] ```Button: Left"#,
    r#"23:15:46 [INFO] ```Button: Left"#,
    r#"widget: WidgetId```("Scripts")"#,
    r#"23:15:46 [INFO] ```Button: Left"#,
    r#"23:15:09 [INFO] ```STARTING TERM MODE"#,
    r#"23:15:09 [INFO] ```Hooking StdOut"#,
    r#"23:15:09 [INFO] ```Creating Crossterm backend"#,
    r#"23:15:09 [INFO] ```Creating Terminal"#,
    r#"23:15:09 [INFO] ```Filesystem -> get_computer_data -> Getting sysinfo"#,
    r#"23:15:09 [INFO] ```Filesystem -> get_computer_data -> Pulling Drive information"#,
    r#"23:15:09 [INFO] ```Filesystem -> get_computer_data -> DriveData: "M.2-WDBlue""#,
    r#"23:15:09 [INFO] ```Filesystem -> get_computer_data -> DriveData: "HDD-Games""#,
    r#"23:15:09 [INFO] ```Filesystem -> get_computer_data -> DriveData: """#,
    r#"23:15:09 [INFO] ```Filesystem -> get_computer_data -> DriveData: "darkmage79@gmail.com - Google...""#,
    r#"23:15:09 [INFO] ```Filesystem -> get_computer_data -> DriveData: "logan@kingsofalchemy.com - Go...""#,
    r#"23:15:09 [ERROR]``` Error Pulling SEB info: "The system cannot find the path specified. (os error 3)""#,
    r#"23:15:09 [INFO] ```Filesystem -> get_computer_data -> pulling GPU"#,
    r#"23:15:09 [INFO] ```Filesystem -> get_computer_data -> Process: Ok(Output { status: ExitStatus(ExitStatus(0)), stdout: "NVIDIA GeForce RTX 3090\r\n", stderr: "" })"#,
    r#"23:15:09 [INFO] ```Filesystem -> get_computer_data -> x: [78, 86, 73, 68, 73, 65, 32, 71, 101, 70, 111, 114, 99, 101, 32, 82, 84, 88, 32, 51, 48, 57, 48, 13, 10]"#,
    r#"23:15:09 [INFO] ```Filesystem -> get_computer_data -> GPU: "NVIDIA GeForce RTX 3090\r\n""#,
    r#"23:15:09 [INFO] ```Filesystem -> get_computer_data -> Pulling CPU"#,
    r#"23:15:09 [INFO] ```Filesystem -> get_computer_data -> Pulling RAM"#,
    r#"23:15:09 [INFO] ```Filesystem -> get_computer_data -> Pulling OS"#,
    r#"23:15:09 [INFO] ```Filesystem -> get_computer_data -> Pulling Hostname"#,
    r#"23:15:09 [INFO] ```Filesystem -> generate_client_id -> combined: ShadowbrokerPC-AMD Ryzen 9 5950X 16-Core Processor-AMD64 Family 25 Model 33 Stepping 0, AuthenticAMD"#,
    r#"23:15:09 [INFO] ```Filesystem -> generate_client_id -> hex_string: bb6064d0dce336cb2731c7a708ddf1fcd764f2f66b4832b398197f3cfad76d8b"#,
    r#"23:15:09 [INFO] ```Filesystem -> get_computer_data -> ID: ShadowbrokerPC:bb6064d0d"#,
    r#"23:15:09 [INFO] ```Filesystem -> get_computer_data -> RecordID: Thing { tb: "computer", id: String("ShadowbrokerPC:bb6064d0d") }"#,
    r#"23:15:09 [INFO] ```Computer Data: ComputerData { id: Thing { tb: "computer", id: String("ShadowbrokerPC:bb6064d0d") }, customer: None, seb_info: None, hostname: "ShadowbrokerPC", operating_system: "Windows 11 Pro", cpu: "AMD Ryzen 9 5950X 16-Core Processor", gpu: "NVIDIA GeForce RTX 3090", ram: "64", drives: [DriveData { drive_letter: "D:\\", drive_type: "SSD", total_size: "1,862", space_left: "593" }, DriveData { drive_letter: "E:\\", drive_type: "HDD", total_size: "5,589", space_left: "1,797" }, DriveData { drive_letter: "C:\\", drive_type: "SSD", total_size: "1,862", space_left: "798" }, DriveData { drive_letter: "G:\\", drive_type: "Unknown(-1)", total_size: "1,862", space_left: "132" }, DriveData { drive_letter: "M:\\", drive_type: "Unknown(-1)", total_size: "1,862", space_left: "758" }], device_name: None, device_mfg: None, device_model: None, device_serial: None }"#,
    r#"23:15:09 [INFO] ```Retrieving sysinfo"#,
    r#"23:15:10 [INFO] ```Running app"#,
    r#"23:15:10 [INFO] ```Filesystem -> generate_client_id -> combined: ShadowbrokerPC-AMD Ryzen 9 5950X 16-Core Processor-AMD64 Family 25 Model 33 Stepping 0, AuthenticAMD"#,
    r#"23:15:10 [INFO] ```Filesystem -> generate_client_id -> hex_string: bb6064d0dce336cb2731c7a708ddf1fcd764f2f66b4832b398197f3cfad76d8b"#,
    r#"23:15:10 [INFO] ```First Run Results: Ok(())"#,
    r#"23:15:10 [INFO] ```Running splash"#,
    r#"23:15:10 [INFO] ```Running splash 2"#,
    r#"23:15:10 [INFO] ```Entering main loop"#,
    r#"23:15:14 [INFO] ```DB: Err(There was an error processing a remote WS request: IO error: No connection could be made because the target machine actively refused it. (os error 10061)"#,
    r#"Caused by:"#,
    r#"    There was an error processing a remote WS request: IO error: No connection could be made because the target machine actively refused it. (os error 10061))"#,
    r#"23:15:46 [INFO] Button: Left"#,
    r#"23:15:46 [INFO] Button: Left"#,
    r#"widget: WidgetId("Scripts")"#,
    r#"23:15:46 [INFO] Button: Left"#,
];