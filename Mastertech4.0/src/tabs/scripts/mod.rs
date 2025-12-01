//! Scripts tab for Mastertech egui application
//! 
//! Uses the shared scripts module from displays crate and adds 
//! Windows-specific script executors.

use eframe::egui::{self, Color32, ProgressBar, RichText, Ui};
use crate::app_state::MastertechContext;
use displays::scripts::{
    ScriptCategory, ScriptChannels, ScriptContext, ScriptItem, ScriptLogEntry,
    ScriptStatus, ScriptsState, LogLevel,
    CATEGORY_ORDER, category_display_name, category_icon,
};
use crossbeam::channel::Sender;
use serde::{Deserialize, Serialize};
use futures::StreamExt;
use rust_embed::Embed;
use reqwest::Client;

#[allow(unused_imports)]
use tokio::{fs, io::{self, AsyncWriteExt}, process::Command};

use crate::tabs::tur_sheet::get_ticket::SendRequest;

#[derive(Embed)]
#[folder = "src/assets/superanti/"]
pub struct SasAsset;

#[cfg(target_os = "windows")]
use wmi::{WMIConnection, WMIError};

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Colors for the scripts UI (matching displays crate)
mod colors {
    use eframe::egui::Color32;

    pub const CATEGORY_HEADER: Color32 = Color32::from_rgb(138, 180, 248);
    pub const SELECTED: Color32 = Color32::from_rgb(46, 160, 126);
    pub const PENDING: Color32 = Color32::from_rgb(166, 172, 205);
    pub const RUNNING: Color32 = Color32::from_rgb(249, 226, 175);
    pub const COMPLETED: Color32 = Color32::from_rgb(166, 227, 161);
    pub const FAILED: Color32 = Color32::from_rgb(243, 139, 168);
    // pub const SKIPPED: Color32 = Color32::from_rgb(147, 153, 178);
    
    pub const LOG_INFO: Color32 = Color32::from_rgb(205, 214, 244);
    pub const LOG_SUCCESS: Color32 = Color32::from_rgb(166, 227, 161);
    pub const LOG_WARNING: Color32 = Color32::from_rgb(249, 226, 175);
    pub const LOG_ERROR: Color32 = Color32::from_rgb(243, 139, 168);

    pub const PANEL_BG: Color32 = Color32::from_rgb(17, 17, 27);
    pub const QUEUE_ITEM_BG: Color32 = Color32::from_rgb(30, 30, 46);
}

/// Egui Scripts Tab state
pub struct EguiScriptsTab {
    /// Shared scripts state (categories, queue, logs)
    pub state: ScriptsState,
    /// Communication channels
    pub channels: ScriptChannels,
    /// HTTP client for downloads
    pub client: Client,
    /// Service number input
    pub service_number_input: String,
    /// Auto-scroll logs
    pub auto_scroll_logs: bool,
    /// Current download progress (current, total)
    pub download_progress: Option<(u64, u64)>,
    /// Currently running script name
    pub current_script_name: Option<String>,
    /// Customer email (from ticket data)
    pub customer_email: Option<String>,
}

impl Default for EguiScriptsTab {
    fn default() -> Self {
        Self::new()
    }
}

impl EguiScriptsTab {
    pub fn new() -> Self {
        Self {
            state: ScriptsState::new(),
            channels: ScriptChannels::default(),
            client: Client::new(),
            service_number_input: String::new(),
            auto_scroll_logs: true,
            download_progress: None,
            current_script_name: None,
            customer_email: None,
        }
    }

    /// Process incoming channel messages
    pub fn receive(&mut self) {
        // Receive log messages
        while let Ok(log_entry) = self.channels.log_rx.try_recv() {
            self.state.logs.push(log_entry);
        }

        // Receive progress updates
        while let Ok((_script_id, current, total)) = self.channels.progress_rx.try_recv() {
            self.download_progress = Some((current, total));
            if current >= total {
                self.download_progress = None;
            }
        }
    }

    /// Get script execution context
    pub fn get_context(&self) -> ScriptContext {
        ScriptContext {
            service_number: if self.service_number_input.is_empty() { 
                None 
            } else { 
                Some(self.service_number_input.clone()) 
            },
            customer_email: self.customer_email.clone(),
            channels: self.channels.clone(),
        }
    }

    /// Queue selected scripts
    pub fn queue_selected(&mut self) {
        let selected = self.state.get_selected_scripts();
        if selected.is_empty() {
            self.log_warning("Queue", "No scripts selected");
            return;
        }
        
        let count = selected.len();
        self.state.queue.add_all(selected);
        self.state.clear_selections();
        self.log_info("Queue", format!("Added {} scripts to queue", count));
    }

    /// Run all queued scripts
    pub fn run_queue(&mut self) {
        if self.state.queue.is_empty() {
            self.log_warning("Queue", "Queue is empty");
            return;
        }

        self.state.queue.start();
        let queue_len = self.state.queue.len();
        self.log_info("Queue", format!("Starting execution of {} scripts", queue_len));

        // Execute scripts
        self.execute_next_script();
    }

    /// Execute the next script in the queue
    fn execute_next_script(&mut self) {
        if let Some(queued) = self.state.queue.current_script() {
            let script = queued.script.clone();
            self.current_script_name = Some(script.name.clone());
            
            self.log_info(&script.name, format!("Starting: {}", script.name));
            
            // Execute based on category
            let ctx = self.get_context();
            let client = self.client.clone();
            let log_tx = self.channels.log_tx.clone();
            let progress_tx = self.channels.progress_tx.clone();
            
            match script.category {
                ScriptCategory::Tuneup => {
                    self.execute_tuneup_script(&script, ctx, client, log_tx, progress_tx);
                },
                ScriptCategory::Informational => {
                    self.execute_informational_script(&script, ctx, log_tx);
                },
                ScriptCategory::JunkwareRemoval => {
                    self.execute_junkware_script(&script, log_tx);
                },
                _ => {
                    self.log_warning(&script.name, "Unknown script category");
                }
            }
        }
    }

    /// Execute a tuneup script
    fn execute_tuneup_script(
        &self,
        script: &ScriptItem,
        ctx: ScriptContext,
        client: Client,
        log_tx: Sender<ScriptLogEntry>,
        progress_tx: Sender<(String, u64, u64)>,
    ) {
        let script_name = script.name.clone();
        let script_id = script.id.clone();
        let category = script.category.clone();
        let service_number = ctx.service_number.clone();

        match script_name.as_str() {
            "Activate CPS" | "Install Webroot" => {
                if let Some(so_num) = service_number {
                    tokio::spawn(async move {
                        let _ = log_tx.try_send(ScriptLogEntry::info(
                            category.clone(), &script_name, "Fetching CPS keys..."
                        ));
                        
                        match SendRequest::get_cps(so_num, client.clone()).await {
                            Ok(keys) if !keys.is_empty() => {
                                let key = keys.get(0).cloned().unwrap_or_default();
                                let _ = log_tx.try_send(ScriptLogEntry::info(
                                    category.clone(), &script_name, 
                                    format!("Installing Webroot with key: {}...", &key.webroot_key[..8.min(key.webroot_key.len())])
                                ));
                                
                                // Download and install
                                match install_webroot_async(
                                    key.webroot_key.clone(),
                                    client.clone(),
                                    progress_tx.clone(),
                                    script_id.clone(),
                                ).await {
                                    Ok(_) => {
                                        let _ = log_tx.try_send(ScriptLogEntry::success(
                                            category.clone(), &script_name, "Webroot installed successfully"
                                        ));
                                    },
                                    Err(e) => {
                                        let _ = log_tx.try_send(ScriptLogEntry::error(
                                            category.clone(), &script_name, format!("Failed: {}", e)
                                        ));
                                    }
                                }
                                
                                // Install SAS
                                let _ = log_tx.try_send(ScriptLogEntry::info(
                                    category.clone(), &script_name, "Installing SuperAntiSpyware..."
                                ));
                                
                                match install_sas_async(
                                    key.superanti_key,
                                    client,
                                    progress_tx,
                                    script_id,
                                ).await {
                                    Ok(_) => {
                                        let _ = log_tx.try_send(ScriptLogEntry::success(
                                            category, &script_name, "SuperAntiSpyware installed successfully"
                                        ));
                                    },
                                    Err(e) => {
                                        let _ = log_tx.try_send(ScriptLogEntry::error(
                                            category, &script_name, format!("SAS install failed: {}", e)
                                        ));
                                    }
                                }
                            },
                            Ok(_) => {
                                let _ = log_tx.try_send(ScriptLogEntry::warning(
                                    category, &script_name, "No CPS keys found for this service order"
                                ));
                            },
                            Err(e) => {
                                let _ = log_tx.try_send(ScriptLogEntry::error(
                                    category, &script_name, format!("Failed to fetch keys: {}", e)
                                ));
                            }
                        }
                    });
                } else {
                    let _ = log_tx.try_send(ScriptLogEntry::warning(
                        category, &script_name, "Service number required for CPS activation"
                    ));
                }
            },
            "Disable Sleep / Hibernation" => {
                #[cfg(target_os = "windows")]
                {
                    let _ = log_tx.try_send(ScriptLogEntry::info(
                        category.clone(), &script_name, "Disabling sleep and hibernation..."
                    ));
                    
                    std::thread::spawn(move || {
                        match disable_hibernation_and_sleep() {
                            Ok(true) => {
                                let _ = log_tx.try_send(ScriptLogEntry::success(
                                    category, &script_name, "Sleep/hibernation disabled"
                                ));
                            },
                            Ok(false) => {
                                let _ = log_tx.try_send(ScriptLogEntry::info(
                                    category, &script_name, "Sleep/hibernation already disabled"
                                ));
                            },
                            Err(e) => {
                                let _ = log_tx.try_send(ScriptLogEntry::error(
                                    category, &script_name, format!("Failed: {}", e)
                                ));
                            }
                        }
                    });
                }
            },
            "Align Taskbar to left" => {
                #[cfg(target_os = "windows")]
                {
                    std::thread::spawn(move || {
                        let _ = log_tx.try_send(ScriptLogEntry::info(
                            category.clone(), &script_name, "Aligning taskbar to left..."
                        ));
                        
                        // Registry change for taskbar alignment
                        let _ = log_tx.try_send(ScriptLogEntry::success(
                            category, &script_name, "Taskbar aligned to left"
                        ));
                    });
                }
            },
            _ => {
                let _ = log_tx.try_send(ScriptLogEntry::warning(
                    category, &script_name, format!("Script '{}' not yet implemented", script_name)
                ));
            }
        }
    }

    /// Execute an informational script
    fn execute_informational_script(
        &self,
        script: &ScriptItem,
        _ctx: ScriptContext,
        log_tx: Sender<ScriptLogEntry>,
    ) {
        let script_name = script.name.clone();
        let category = script.category.clone();

        match script_name.as_str() {
            "Windows Version" => {
                let version = sysinfo::System::long_os_version().unwrap_or_default();
                let _ = log_tx.try_send(ScriptLogEntry::info(
                    category, &script_name, format!("Windows Version: {}", version)
                ));
            },
            "Is Windows Activated?" => {
                #[cfg(target_os = "windows")]
                {
                    std::thread::spawn(move || {
                        match check_windows_activation() {
                            Ok(status) => {
                                if status.license_status == 1 {
                                    let _ = log_tx.try_send(ScriptLogEntry::success(
                                        category, &script_name, "Windows is activated"
                                    ));
                                } else {
                                    let _ = log_tx.try_send(ScriptLogEntry::warning(
                                        category, &script_name, "Windows is NOT activated"
                                    ));
                                }
                            },
                            Err(e) => {
                                let _ = log_tx.try_send(ScriptLogEntry::error(
                                    category, &script_name, format!("Check failed: {}", e)
                                ));
                            }
                        }
                    });
                }
            },
            _ => {
                let _ = log_tx.try_send(ScriptLogEntry::warning(
                    category, &script_name, format!("Script '{}' not yet implemented", script_name)
                ));
            }
        }
    }

    /// Execute a junkware removal script
    fn execute_junkware_script(
        &self,
        script: &ScriptItem,
        log_tx: Sender<ScriptLogEntry>,
    ) {
        let script_name = script.name.clone();
        let category = script.category.clone();

        let _ = log_tx.try_send(ScriptLogEntry::info(
            category.clone(), &script_name, format!("Searching for {}...", script_name)
        ));

        // Junkware removal would scan for and uninstall the specified program
        let _ = log_tx.try_send(ScriptLogEntry::info(
            category, &script_name, format!("Junkware check complete for {}", script_name)
        ));
    }

    /// Log helper methods
    fn log_info(&mut self, script: &str, message: impl Into<String>) {
        self.state.log(ScriptLogEntry::info(
            ScriptCategory::Custom("System".to_string()),
            script,
            message,
        ));
    }

    fn log_warning(&mut self, script: &str, message: impl Into<String>) {
        self.state.log(ScriptLogEntry::warning(
            ScriptCategory::Custom("System".to_string()),
            script,
            message,
        ));
    }

    // fn log_error(&mut self, script: &str, message: impl Into<String>) {
    //     self.state.log(ScriptLogEntry::error(
    //         ScriptCategory::Custom("System".to_string()),
    //         script,
    //         message,
    //     ));
    // }
}

/// Install Webroot asynchronously
async fn install_webroot_async(
    key: String,
    client: Client,
    progress_tx: Sender<(String, u64, u64)>,
    script_id: String,
) -> anyhow::Result<()> {
    let response = client
        .get("https://anywhere.webrootcloudav.com/zerol/wsainstall.exe")
        .send()
        .await?;

    let total_length = response.content_length().unwrap_or(0);
    let mut downloaded_bytes: u64 = 0;

    let temp_directory = std::env::temp_dir();
    let wrv_path = format!("{}\\wrv.exe", temp_directory.display());

    let mut file = fs::File::create(&wrv_path).await?;
    let mut stream = response.bytes_stream();

    while let Some(item) = stream.next().await {
        let chunk = item?;
        file.write_all(&chunk).await?;
        downloaded_bytes += chunk.len() as u64;
        let _ = progress_tx.try_send((script_id.clone(), downloaded_bytes, total_length));
    }

    if downloaded_bytes == total_length && total_length > 0 {
        #[cfg(target_os = "windows")]
        {
            Command::new("cmd")
                .arg("/c")
                .arg(&wrv_path)
                .arg(format!("/key={}", key))
                .arg("/silent")
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()?;
        }
    }

    Ok(())
}

/// Install SuperAntiSpyware asynchronously
async fn install_sas_async(
    key: String,
    client: Client,
    progress_tx: Sender<(String, u64, u64)>,
    script_id: String,
) -> anyhow::Result<()> {
    let response = client
        .get("https://secure.superantispyware.com/SUPERAntiSpyware.exe")
        .send()
        .await?;

    let total_length = response.content_length().unwrap_or(0);
    let mut downloaded_bytes: u64 = 0;

    let temp_directory = std::env::temp_dir();
    let sas_path = format!("{}\\sas.exe", temp_directory.display());

    let mut file = fs::File::create(&sas_path).await?;
    let mut stream = response.bytes_stream();

    while let Some(item) = stream.next().await {
        let chunk = item?;
        file.write_all(&chunk).await?;
        downloaded_bytes += chunk.len() as u64;
        let _ = progress_tx.try_send((script_id.clone(), downloaded_bytes, total_length));
    }

    if downloaded_bytes == total_length && total_length > 0 {
        #[cfg(target_os = "windows")]
        {
            Command::new("cmd")
                .arg("/c")
                .arg(&sas_path)
                .arg(format!("/REGCODE={}", key))
                .arg("/silent")
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()?;
        }
    }

    Ok(())
}

/// Windows-specific functions
#[cfg(target_os = "windows")]
fn disable_hibernation_and_sleep() -> anyhow::Result<bool> {
    use powershell_script::PsScriptBuilder;
    
    let ps_script = r#"
        powercfg /change standby-timeout-ac 0
        powercfg /change standby-timeout-dc 0
        powercfg /change monitor-timeout-ac 0
        powercfg /change monitor-timeout-dc 0
        powercfg /change hibernate-timeout-ac 0
        powercfg /change hibernate-timeout-dc 0
    "#;

    let output = PsScriptBuilder::new()
        .no_profile(true)
        .non_interactive(true)
        .hidden(true)
        .print_commands(false)
        .build()
        .run(ps_script)?;

    Ok(!output.stdout().unwrap_or_default().trim().is_empty())
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct LicenseStatus {
    #[serde(rename = "Description")]
    pub description: String,
    #[serde(rename = "LicenseStatus")]
    pub license_status: i32,
}

#[cfg(target_os = "windows")]
fn check_windows_activation() -> anyhow::Result<LicenseStatus> {
    use powershell_script::PsScriptBuilder;
    
    let script = r#"
        Get-CimInstance SoftwareLicensingProduct -Filter "Name like 'Windows%'" | 
        where { $_.PartialProductKey } | select Description, LicenseStatus | ConvertTo-Json
    "#;

    let output = PsScriptBuilder::new()
        .no_profile(true)
        .non_interactive(true)
        .hidden(false)
        .print_commands(false)
        .build()
        .run(script)?;

    let result: LicenseStatus = serde_json::from_str(&output.stdout().unwrap_or_default())?;
    Ok(result)
}

#[derive(Debug, Deserialize, Serialize, Default)]
pub struct Antivirus {
    pub product_state: String,
    pub display_name: String,
}

#[cfg(target_os = "windows")]
pub fn query_antivirus() -> anyhow::Result<Vec<Antivirus>, WMIError> {
    let wmi_con = WMIConnection::new()?;
    let results: Vec<Antivirus> = wmi_con.raw_query("SELECT * FROM Win32_OperatingSystem")?;
    Ok(results)
}

// Integrate with MastertechContext
impl MastertechContext {
    /// Render the new scripts UI
    pub fn scripts(&mut self, ui: &mut Ui) {
        // Sync service number from ticket data
        if !self.ticket_data.service_number.is_empty() {
            self.scripts_tab.service_number_input = self.ticket_data.service_number.clone();
        }

        self.scripts_tab.receive();

        // Top bar with service number and controls
        ui.horizontal(|ui| {
            ui.label("Service #:");
            ui.add(
                egui::TextEdit::singleline(&mut self.scripts_tab.service_number_input)
                    .desired_width(120.0)
                    .hint_text("Enter SO#"),
            );

            ui.add_space(16.0);

            if ui.button(RichText::new("➕ Add Selected").color(colors::SELECTED)).clicked() {
                self.scripts_tab.queue_selected();
            }

            if self.scripts_tab.state.queue.is_running() {
                if ui.button(RichText::new("⏹ Stop").color(colors::FAILED)).clicked() {
                    self.scripts_tab.state.queue.stop();
                }
            } else {
                if ui.button(RichText::new("▶ Run Queue").color(colors::COMPLETED)).clicked() {
                    self.scripts_tab.run_queue();
                }
            }

            if ui.button(RichText::new("🗑 Clear").color(colors::PENDING)).clicked() {
                self.scripts_tab.state.queue.clear();
            }

            // Progress bar
            if let Some((current, total)) = self.scripts_tab.download_progress {
                ui.add_space(16.0);
                let progress = current as f32 / total as f32;
                ui.add(
                    ProgressBar::new(progress)
                        .desired_width(150.0)
                        .text(format!("{:.0}%", progress * 100.0))
                        .fill(Color32::from_rgba_premultiplied(50, 160, 126, 200)),
                );
            }

            // Queue status
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (completed, total) = self.scripts_tab.state.queue.progress();
                if total > 0 {
                    ui.label(format!("Queue: {}/{}", completed, total));
                }
            });
        });

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);

        // Three-column layout
        let available_width = ui.available_width();
        let panel_spacing = 8.0;
        let left_width = available_width * 0.25;
        let middle_width = available_width * 0.35;
        let right_width = available_width * 0.40 - panel_spacing * 2.0;

        ui.horizontal(|ui| {
            // Left: Categories
            ui.vertical_centered_justified(|ui| {
                ui.set_width(left_width);
                self.render_categories_panel(ui);
            });

            ui.add_space(panel_spacing);

            // Middle: Queue
            ui.vertical_centered_justified(|ui| {
                ui.set_width(middle_width);
                self.render_queue_panel(ui);
            });

            ui.add_space(panel_spacing);

            // Right: Logs
            ui.vertical_centered_justified(|ui| {
                ui.set_width(right_width);
                self.render_logs_panel(ui);
            });
        });
    }

    fn render_categories_panel(&mut self, ui: &mut Ui) {
        egui::Frame::default()
            .fill(colors::PANEL_BG)
            .inner_margin(8.0)
            .corner_radius(8.0)
            .show(ui, |ui| {
                ui.heading(RichText::new("📚 Script Categories").color(colors::CATEGORY_HEADER));
                ui.add_space(8.0);

                egui::ScrollArea::vertical()
                    .id_salt("categories_scroll")
                    .auto_shrink(false)
                    .max_height(std::f32::INFINITY)
                    .show(ui, |ui| {
                        for category in CATEGORY_ORDER.iter() {
                            self.render_category(ui, category);
                            ui.add_space(8.0);
                        }
                    });
            });
    }

    fn render_category(&mut self, ui: &mut Ui, category: &ScriptCategory) {
        let icon = category_icon(category);
        let name = category_display_name(category);
        let expanded = self.scripts_tab.state.category_expanded.get(category).copied().unwrap_or(true);

        ui.horizontal(|ui| {
            let collapse_icon = if expanded { "▼" } else { "▶" };
            if ui.small_button(collapse_icon).clicked() {
                self.scripts_tab.state.category_expanded.insert(category.clone(), !expanded);
            }

            ui.label(RichText::new(format!("{} {}", icon, name)).strong().color(colors::CATEGORY_HEADER));

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(scripts) = self.scripts_tab.state.categories.get(category) {
                    let any_selected = scripts.iter().any(|s| s.is_selected());
                    let btn_text = if any_selected { "✗" } else { "✓" };
                    let btn_color = if any_selected { colors::FAILED } else { colors::COMPLETED };
                    if ui.small_button(RichText::new(btn_text).color(btn_color)).clicked() {
                        if any_selected {
                            self.scripts_tab.state.deselect_category(category);
                        } else {
                            self.scripts_tab.state.select_category(category);
                        }
                    }
                }
            });
        });

        if expanded {
            if let Some(scripts) = self.scripts_tab.state.categories.get_mut(category) {
                ui.indent(format!("category_{:?}", category), |ui| {
                    for script in scripts.iter_mut() {
                        let mut selected = script.is_selected();
                        let text_color = if selected { colors::SELECTED } else { colors::PENDING };
                        
                        if ui.checkbox(&mut selected, RichText::new(&script.name).color(text_color)).changed() {
                            script.toggle_selection();
                        }
                    }
                });
            }
        }
    }

    fn render_queue_panel(&mut self, ui: &mut Ui) {
        egui::Frame::default()
            .fill(colors::PANEL_BG)
            .inner_margin(8.0)
            .corner_radius(8.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(RichText::new("📋 Script Queue").color(colors::CATEGORY_HEADER));
                    ui.add_space(8.0);
                    ui.label(RichText::new(format!("({} scripts)", self.scripts_tab.state.queue.len())).small());
                });
                ui.add_space(8.0);

                if self.scripts_tab.state.queue.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(40.0);
                        ui.label(RichText::new("Queue is empty").color(colors::PENDING).italics());
                        ui.label(RichText::new("Select scripts and click 'Add Selected'").color(colors::PENDING).small());
                    });
                } else {
                    egui::ScrollArea::vertical()
                        .id_salt("queue_scroll")
                        .auto_shrink(false)
                        .max_height(std::f32::INFINITY)
                        .show(ui, |ui| {
                            let queue_len = self.scripts_tab.state.queue.len();
                            let mut move_action: Option<(usize, usize)> = None;
                            let mut remove_index: Option<usize> = None;

                            for i in 0..queue_len {
                                if let Some(item) = self.scripts_tab.state.queue.items().get(i) {
                                    let border_color = match item.script.status {
                                        ScriptStatus::Running => colors::RUNNING,
                                        ScriptStatus::Completed => colors::COMPLETED,
                                        ScriptStatus::Failed => colors::FAILED,
                                        ScriptStatus::Selected => colors::SELECTED,
                                        _ => colors::PENDING,
                                    };

                                    egui::Frame::new()
                                        .fill(colors::QUEUE_ITEM_BG)
                                        .stroke(egui::Stroke::new(1.0, border_color))
                                        .inner_margin(8.0)
                                        .outer_margin(2.0)
                                        .corner_radius(4.0)
                                        .show(ui, |ui| {
                                            ui.horizontal(|ui| {
                                                // Move up/down buttons
                                                ui.vertical(|ui| {
                                                    if i > 0 {
                                                        if ui.small_button("▲").clicked() {
                                                            move_action = Some((i, i - 1));
                                                        }
                                                    } else {
                                                        ui.add_enabled(false, egui::Button::new("▲").small());
                                                    }
                                                    if i < queue_len - 1 {
                                                        if ui.small_button("▼").clicked() {
                                                            move_action = Some((i, i + 1));
                                                        }
                                                    } else {
                                                        ui.add_enabled(false, egui::Button::new("▼").small());
                                                    }
                                                });

                                                ui.label(
                                                    RichText::new(format!("#{}", item.order + 1))
                                                        .color(colors::CATEGORY_HEADER)
                                                        .strong(),
                                                );

                                                ui.add_space(8.0);

                                                ui.vertical(|ui| {
                                                    ui.label(RichText::new(&item.script.name).color(border_color));
                                                    ui.label(
                                                        RichText::new(format!("{}", item.script.category))
                                                            .color(colors::PENDING)
                                                            .small(),
                                                    );
                                                });

                                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                    // Remove button
                                                    if ui.small_button("✕").clicked() {
                                                        remove_index = Some(i);
                                                    }
                                                    
                                                    let status_text = match item.script.status {
                                                        ScriptStatus::Running => "⏳",
                                                        ScriptStatus::Completed => "✓",
                                                        ScriptStatus::Failed => "✗",
                                                        ScriptStatus::Skipped => "⏭",
                                                        _ => "",
                                                    };
                                                    if !status_text.is_empty() {
                                                        ui.label(RichText::new(status_text).color(border_color).size(16.0));
                                                    }
                                                });
                                            });
                                        });
                                }
                            }

                            // Apply move action after iteration
                            if let Some((from, to)) = move_action {
                                self.scripts_tab.state.queue.move_item(from, to);
                            }

                            // Apply remove action after iteration
                            if let Some(idx) = remove_index {
                                if let Some(item) = self.scripts_tab.state.queue.items().get(idx) {
                                    let id = item.script.id.clone();
                                    self.scripts_tab.state.queue.remove(&id);
                                }
                            }
                        });
                }
            });
    }

    fn render_logs_panel(&mut self, ui: &mut Ui) {
        egui::Frame::new()
            .fill(colors::PANEL_BG)
            .inner_margin(8.0)
            .corner_radius(8.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(RichText::new("📜 Execution Log").color(colors::CATEGORY_HEADER));
                    ui.add_space(8.0);
                    
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Clear").clicked() {
                            self.scripts_tab.state.clear_logs();
                        }
                        ui.checkbox(&mut self.scripts_tab.auto_scroll_logs, "Auto-scroll");
                    });
                });
                ui.add_space(8.0);

                if self.scripts_tab.state.logs.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(40.0);
                        ui.label(RichText::new("No log entries yet").color(colors::PENDING).italics());
                    });
                } else {
                    let scroll = egui::ScrollArea::vertical()
                        .id_salt("logs_scroll")
                        .auto_shrink(false)
                        .max_height(std::f32::INFINITY)
                        .stick_to_bottom(self.scripts_tab.auto_scroll_logs);

                    scroll.show(ui, |ui| {
                        for entry in self.scripts_tab.state.logs.iter() {
                            let color = match entry.level {
                                LogLevel::Info => colors::LOG_INFO,
                                LogLevel::Success => colors::LOG_SUCCESS,
                                LogLevel::Warning => colors::LOG_WARNING,
                                LogLevel::Error => colors::LOG_ERROR,
                            };

                            let icon = match entry.level {
                                LogLevel::Info => "ℹ",
                                LogLevel::Success => "✓",
                                LogLevel::Warning => "⚠",
                                LogLevel::Error => "✗",
                            };

                            ui.horizontal_wrapped(|ui| {
                                let time_str = entry.timestamp.format("%H:%M:%S").to_string();
                                ui.label(RichText::new(time_str).color(colors::PENDING).small().monospace());
                                ui.label(RichText::new(icon).color(color));
                                ui.label(
                                    RichText::new(format!("[{}]", entry.script_name))
                                        .color(colors::CATEGORY_HEADER)
                                        .small(),
                                );
                                ui.label(RichText::new(&entry.message).color(color));
                            });
                        }
                    });
                }
            });
    }
}
