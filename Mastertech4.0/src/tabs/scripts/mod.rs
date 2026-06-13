//! Scripts tab for Mastertech egui application
//! 
//! Uses the shared scripts module from displays crate and adds 
//! Windows-specific script executors.

use eframe::egui::{self, vec2, Color32, ProgressBar, RichText, Ui};
use egui::{Button, Widget};
use crate::app_state::MastertechContext;
use crate::tabs::tur_sheet::get_ticket::SendRequest;
use crate::tabs::file_browser::command::{run_robocopy, RobocopyMessage};
use displays::scripts::{
    ScriptCategory, ScriptChannels, ScriptContext, ScriptItem, ScriptLogEntry,
    ScriptStatus, ScriptsState, LogLevel,
    CATEGORY_ORDER, category_display_name, category_icon,
    script_run_request_receiver, script_run_result_sender,
    ScriptRunRequest, ScriptRunResult,
};
use crossbeam::channel::{Receiver, Sender};
use serde::{Deserialize, Serialize};
#[allow(unused_imports)]
use futures::StreamExt;
use rust_embed::Embed;
use reqwest::Client;
use std::path::PathBuf;

#[allow(unused_imports)]
use tokio::{fs, io::{self, AsyncWriteExt}, process::Command};

#[cfg(target_os = "windows")]
use crate::utilities::scripts::{
    install_webroot, install_sas, install_supereasybackup, install_program,
    InstalledProgram, AntiVirusProduct, ScheduledTask, StartupProgram, StartupState,
    get_running_processes, check_power_options,
};

#[cfg(target_os = "windows")]
use crate::utilities::windows::registry::{
    align_taskbar_left, disable_notifications, disable_copilot,
    disable_lockscreen_notifications, disable_content_delivery_allowed,
    disable_silent_installed_apps_enabled, disable_subscribed_content_enabled,
    disable_system_pane_suggestions_enabled, disable_account_notifications,
    enable_more_pins_layout, disable_start_account_notifications,
    disable_recent_items_tracking, remove_chat_from_taskbar,
};

#[cfg(target_os = "windows")]
use crate::utilities::windows::windows_update::install_windows_updates;

#[cfg(target_os = "windows")]
#[allow(unused_imports)]
use crate::utilities::windows::antivirus::check_antivirus;

#[derive(Embed)]
#[folder = "src/assets/superanti/"]
pub struct SasAsset;

#[cfg(target_os = "windows")]
#[allow(unused_imports)]
use wmi::{WMIConnection, WMIError};

#[cfg(target_os = "windows")]
#[allow(dead_code)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Colors for the scripts UI
mod colors {
    use eframe::egui::Color32;

    pub const CATEGORY_HEADER: Color32 = Color32::from_rgb(138, 180, 248);
    pub const SELECTED: Color32 = Color32::from_rgb(46, 160, 126);
    pub const PENDING: Color32 = Color32::from_rgb(166, 172, 205);
    pub const RUNNING: Color32 = Color32::from_rgb(249, 226, 175);
    pub const COMPLETED: Color32 = Color32::from_rgb(166, 227, 161);
    pub const FAILED: Color32 = Color32::from_rgb(243, 139, 168);
    
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
    /// Data transfer candidates (path, size)
    pub data_transfer_candidates: Vec<(String, String)>,
    /// Channel for receiving data transfer candidates
    pub data_transfer_rx: Receiver<Vec<(String, String)>>,
    pub data_transfer_tx: Sender<Vec<(String, String)>>,
    /// Channel for robocopy messages
    pub robocopy_rx: Receiver<RobocopyMessage>,
    pub robocopy_tx: Sender<RobocopyMessage>,
    /// Channel for install progress (used by install_webroot, install_sas, etc.)
    pub install_progress_rx: Receiver<(u64, u64)>,
    pub install_progress_tx: Sender<(u64, u64)>,
    /// Windows update channel
    #[cfg(target_os = "windows")]
    pub windows_update_rx: Receiver<crate::utilities::windows::windows_update::WindowsUpdateEvent>,
    #[cfg(target_os = "windows")]
    pub windows_update_tx: Sender<crate::utilities::windows::windows_update::WindowsUpdateEvent>,
    /// Is data transfer UI showing
    pub show_data_transfer_ui: bool,
    /// Selected source paths for data transfer
    pub selected_sources: Vec<String>,
    /// Selected destination for data transfer
    pub selected_destination: Option<String>,
    /// In-flight script runs requested by the MCP `scripts_run` tool. Each
    /// entry tracks the request_id, the script the AI asked us to run, the
    /// `state.logs` index at dispatch time, and the dispatch timestamp so we
    /// can time it out. Resolved when a Success / Error / Warning log entry
    /// for the matching script_name lands in `state.logs`.
    pub pending_mcp_runs: Vec<McpPendingRun>,
    /// diagnostic_session id from the latest MCP scripts_run request.
    pub mcp_diagnostic_session_id: Option<String>,
}

/// Tracks one in-flight MCP-initiated script run inside `EguiScriptsTab`.
#[derive(Debug, Clone)]
pub struct McpPendingRun {
    pub request_id: String,
    pub script_name: String,
    pub category: ScriptCategory,
    /// Index into `state.logs` at the moment the script was dispatched.
    /// All log entries with index >= this value are candidates for completion
    /// detection and inclusion in the returned `ScriptRunResult.logs`.
    pub log_start_index: usize,
    pub dispatched_at: std::time::Instant,
    /// Hard ceiling: if this elapses without a Success/Error/Warning log for
    /// `script_name`, we send back a timeout result and drop the entry.
    pub timeout: std::time::Duration,
}

impl Default for EguiScriptsTab {
    fn default() -> Self {
        Self::new()
    }
}

impl EguiScriptsTab {
    pub fn new() -> Self {
        let (data_transfer_tx, data_transfer_rx) = crossbeam::channel::unbounded();
        let (robocopy_tx, robocopy_rx) = crossbeam::channel::unbounded();
        let (install_progress_tx, install_progress_rx) = crossbeam::channel::unbounded();
        #[cfg(target_os = "windows")]
        let (windows_update_tx, windows_update_rx) = crossbeam::channel::unbounded();
        
        Self {
            state: ScriptsState::new(),
            channels: ScriptChannels::default(),
            client: Client::new(),
            service_number_input: String::new(),
            auto_scroll_logs: true,
            download_progress: None,
            current_script_name: None,
            customer_email: None,
            data_transfer_candidates: Vec::new(),
            data_transfer_rx,
            data_transfer_tx,
            robocopy_rx,
            robocopy_tx,
            install_progress_rx,
            install_progress_tx,
            #[cfg(target_os = "windows")]
            windows_update_rx,
            #[cfg(target_os = "windows")]
            windows_update_tx,
            show_data_transfer_ui: false,
            selected_sources: Vec::new(),
            selected_destination: None,
            pending_mcp_runs: Vec::new(),
            mcp_diagnostic_session_id: None,
        }
    }

    /// Drain MCP `scripts_run` requests off the global crossbeam channel and
    /// dispatch each through the existing `execute_*_script` paths. Tracks
    /// each request in `pending_mcp_runs` so `process_mcp_completions` can
    /// later report success/failure + collected logs back to the MCP caller.
    ///
    /// Call this once per frame, BEFORE `receive()`, so any logs the script
    /// emits synchronously inside dispatch land in `state.logs` after the
    /// `log_start_index` we capture here.
    pub fn process_mcp_requests(&mut self) {
        while let Ok(req) = script_run_request_receiver().try_recv() {
            self.dispatch_mcp_request(req);
        }
    }

    fn dispatch_mcp_request(&mut self, req: ScriptRunRequest) {
        if let Some(sn) = req.service_number.as_deref() {
            if !sn.is_empty() {
                self.service_number_input = sn.to_string();
            }
        }
        if let Some(em) = req.customer_email.as_deref() {
            if !em.is_empty() {
                self.customer_email = Some(em.to_string());
            }
        }
        self.mcp_diagnostic_session_id = req
            .diagnostic_session_id
            .clone()
            .filter(|s| !s.trim().is_empty());

        let log_start_index = self.state.logs.len();
        self.log_info(
            "MCP",
            format!(
                "MCP requested: run '{}' (category {:?}, request_id {})",
                req.script_name, req.category, req.request_id
            ),
        );

        let script = ScriptItem::new(req.script_name.clone(), req.category.clone());
        let ctx = self.get_context();
        let client = self.client.clone();
        let log_tx = self.channels.log_tx.clone();
        let progress_tx = self.channels.progress_tx.clone();

        match req.category {
            ScriptCategory::Tuneup => {
                self.execute_tuneup_script(&script, ctx, client, log_tx, progress_tx);
            }
            ScriptCategory::Informational => {
                self.execute_informational_script(&script, ctx, log_tx);
            }
            ScriptCategory::JunkwareRemoval => {
                self.execute_junkware_script(&script, log_tx);
            }
            other => {
                let _ = script_run_result_sender().send(ScriptRunResult {
                    request_id: req.request_id,
                    success: false,
                    message: format!("Unsupported category: {:?}", other),
                    logs: Vec::new(),
                });
                return;
            }
        }

        self.pending_mcp_runs.push(McpPendingRun {
            request_id: req.request_id,
            script_name: req.script_name,
            category: req.category,
            log_start_index,
            dispatched_at: std::time::Instant::now(),
            timeout: std::time::Duration::from_secs(600),
        });
    }

    /// Walks `pending_mcp_runs` and reports completion to MCP for any run
    /// whose script has emitted a final-state (Success / Error / Warning) log
    /// entry, or whose `timeout` has elapsed. Call once per frame, AFTER
    /// `receive()` has drained the latest log entries into `state.logs`.
    pub fn process_mcp_completions(&mut self) {
        if self.pending_mcp_runs.is_empty() {
            return;
        }

        let now = std::time::Instant::now();
        let logs_snapshot_len = self.state.logs.len();
        let mut to_remove: Vec<usize> = Vec::new();

        for (idx, pending) in self.pending_mcp_runs.iter().enumerate() {
            let final_entry = self
                .state
                .logs
                .get(pending.log_start_index..logs_snapshot_len)
                .unwrap_or(&[])
                .iter()
                .rev()
                .find(|e| {
                    e.script_name == pending.script_name
                        && matches!(e.level, LogLevel::Success | LogLevel::Error | LogLevel::Warning)
                });

            let timed_out = now.duration_since(pending.dispatched_at) > pending.timeout;

            if let Some(final_e) = final_entry {
                let success = matches!(final_e.level, LogLevel::Success);
                let logs = self.collect_pending_logs(pending, logs_snapshot_len);
                let _ = script_run_result_sender().send(ScriptRunResult {
                    request_id: pending.request_id.clone(),
                    success,
                    message: final_e.message.clone(),
                    logs,
                });
                to_remove.push(idx);
            } else if timed_out {
                let logs = self.collect_pending_logs(pending, logs_snapshot_len);
                let _ = script_run_result_sender().send(ScriptRunResult {
                    request_id: pending.request_id.clone(),
                    success: false,
                    message: format!(
                        "Script '{}' did not emit a Success/Error log within {}s. It may still be running on the host.",
                        pending.script_name,
                        pending.timeout.as_secs()
                    ),
                    logs,
                });
                to_remove.push(idx);
            }
        }

        for idx in to_remove.iter().rev() {
            self.pending_mcp_runs.remove(*idx);
        }
    }

    fn collect_pending_logs(&self, pending: &McpPendingRun, end_index: usize) -> Vec<String> {
        self.state
            .logs
            .get(pending.log_start_index..end_index)
            .unwrap_or(&[])
            .iter()
            .filter(|e| e.script_name == pending.script_name || e.script_name == "MCP")
            .map(|e| {
                let level = match e.level {
                    LogLevel::Info => "INFO",
                    LogLevel::Success => "OK",
                    LogLevel::Warning => "WARN",
                    LogLevel::Error => "ERR",
                };
                format!(
                    "{} [{}] {}",
                    e.timestamp.format("%H:%M:%S"),
                    level,
                    e.message
                )
            })
            .collect()
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

        // Receive install progress updates (from install_webroot, install_sas, etc.)
        while let Ok((current, total)) = self.install_progress_rx.try_recv() {
            self.download_progress = Some((current, total));
            if current >= total {
                self.download_progress = None;
            }
        }

        // Receive data transfer candidates
        while let Ok(candidates) = self.data_transfer_rx.try_recv() {
            self.data_transfer_candidates = candidates;
            self.show_data_transfer_ui = true;
            self.log_info("Data Transfer", format!("Found {} user profiles", self.data_transfer_candidates.len()));
        }

        // Receive robocopy messages
        while let Ok(msg) = self.robocopy_rx.try_recv() {
            match msg {
                RobocopyMessage::Progress(progress) => {
                    self.log_info("Data Transfer", format!(
                        "Copying: {} -> {} (R: {:.1} MB/s, W: {:.1} MB/s)",
                        progress.source, progress.destination,
                        progress.bytes_read, progress.bytes_written
                    ));
                },
                RobocopyMessage::Complete(pid) => {
                    self.log_info("Data Transfer", format!("Transfer complete (PID: {})", pid));
                }
            }
        }

        // Receive Windows update events
        #[cfg(target_os = "windows")]
        while let Ok(event) = self.windows_update_rx.try_recv() {
            use crate::utilities::windows::windows_update::WindowsUpdateEvent;
            match event {
                WindowsUpdateEvent::UpdateLogs(log) => {
                    self.log_info("Windows Updates", log);
                },
                WindowsUpdateEvent::ReturnedUpdates(updates) => {
                    self.log_info("Windows Updates", format!("Found {} updates", updates.updates.len()));
                },
                WindowsUpdateEvent::DownloadPercentage(pct) => {
                    self.download_progress = Some((pct as u64, 100));
                },
                WindowsUpdateEvent::InstallPercentage(pct) => {
                    self.download_progress = Some((pct as u64, 100));
                    if pct >= 100 {
                        self.download_progress = None;
                    }
                },
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
        &mut self,
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
        let customer_email = ctx.customer_email.clone();

        match script_name.as_str() {
            "Data Transfer" => {
                self.execute_data_transfer(log_tx);
            },
            "Activate CPS" => {
                self.execute_activate_cps(service_number, client, log_tx, progress_tx, script_id, category, script_name);
            },
            "Activate SEB" => {
                self.execute_activate_seb(customer_email, client, log_tx, progress_tx, script_id, category, script_name);
            },
            "Install Windows Updates" => {
                self.execute_install_windows_updates(log_tx, category, script_name);
            },
            "Disable Sleep / Hibernation" => {
                self.execute_disable_sleep(log_tx, category, script_name);
            },
            "Run SuperAntiSpyware Scan" => {
                self.execute_sas_scan(log_tx, category, script_name);
            },
            "Run Webroot Scan" => {
                self.execute_webroot_scan(log_tx, category, script_name);
            },
            "Run Junkware Category" => {
                self.execute_all_junkware(log_tx);
            },
            "Install LibreOffice" => {
                self.execute_install_libreoffice(client, log_tx, progress_tx, script_id, category, script_name);
            },
            "Disable proxy settings" => {
                let _ = log_tx.try_send(ScriptLogEntry::warning(
                    category, &script_name, "Proxy settings disable not yet implemented"
                ));
            },
            "Disable Notifications" => {
                self.execute_disable_notifications(log_tx, category, script_name);
            },
            "Change SuperAntiSpyware settings" => {
                let _ = log_tx.try_send(ScriptLogEntry::warning(
                    category, &script_name, "SuperAntiSpyware settings change not yet implemented"
                ));
            },
            "Disable Startup Apps" => {
                self.execute_disable_startup_apps(log_tx, category, script_name);
            },
            "Unpin Copilot" => {
                self.execute_unpin_copilot(log_tx, category, script_name);
            },
            "Align Taskbar to left" => {
                self.execute_align_taskbar(log_tx, category, script_name);
            },
            "Change Timezone to Mountain" => {
                self.execute_change_timezone(log_tx, category, script_name);
            },
            "Disable BitLocker" => {
                self.execute_disable_bitlocker(log_tx, category, script_name);
            },
            "GPU Stress Test" => {
                self.execute_gpu_probe(log_tx, category, script_name);
            },
            _ => {
                let _ = log_tx.try_send(ScriptLogEntry::warning(
                    category, &script_name, format!("Script '{}' not yet implemented", script_name)
                ));
            }
        }
    }

    fn execute_gpu_probe(
        &mut self,
        log_tx: Sender<ScriptLogEntry>,
        category: ScriptCategory,
        script_name: String,
    ) {
        use std::sync::Arc;
        use stress_kit::telemetry::TelemetryAgent;
        use stress_runner::{drive_blocking, gpu_probe_spec, RunResult, RunUpdate};

        let _ = log_tx.try_send(ScriptLogEntry::info(
            category.clone(),
            &script_name,
            "Starting 4-stage GPU probe (compute → matmul → VRAM → PCIe)",
        ));

        let client = crate::filesystem::get_client_hash();
        let service_number = self.service_number_input.clone();
        let diagnostic_session_id = self.mcp_diagnostic_session_id.clone().unwrap_or_default();
        let log_tx2 = log_tx.clone();
        let category2 = category.clone();
        let script_name2 = script_name.clone();

        std::thread::spawn(move || {
            let telemetry = Arc::new(TelemetryAgent::start(1000));
            let mut spec = gpu_probe_spec(
                client.computer.clone().expect("get_client_hash sets computer"),
                1.0,
            );
            spec.tags.push("origin:scripts".into());
            spec.hostname = std::env::var("COMPUTERNAME")
                .or_else(|_| std::env::var("HOSTNAME"))
                .ok();
            spec.machine_id = Some(client.client_hash.clone());
            if !service_number.is_empty() {
                spec.service_order = Some(database::schema::RecordId::new(
                    database::schema::TICKET_TABLE,
                    service_number,
                ));
            }
            if !diagnostic_session_id.is_empty() {
                spec.session_ref = Some(database::schema::entity_link::parse_record_id(
                    &diagnostic_session_id,
                    database::schema::DIAGNOSTIC_SESSION_TABLE,
                ));
            }

            let verdict = drive_blocking(spec, telemetry, |update| match update {
                RunUpdate::Started { run_id } => {
                    use database::schema::RecordIdExt;
                    let _ = log_tx2.try_send(ScriptLogEntry::info(
                        category2.clone(),
                        &script_name2,
                        format!("stress_test_run id: {}", run_id.key_string()),
                    ));
                }
                RunUpdate::StageStarted { index, label, stage_count } => {
                    let _ = log_tx2.try_send(ScriptLogEntry::info(
                        category2.clone(),
                        &script_name2,
                        format!("Stage {}/{}: {label}", index + 1, stage_count),
                    ));
                }
                RunUpdate::Tick { metrics, stage_label, .. } => {
                    if let Some(err) = metrics.last_error.as_ref() {
                        let stage = stage_label.unwrap_or_else(|| "gpu".into());
                        let _ = log_tx2.try_send(ScriptLogEntry::warning(
                            category2.clone(),
                            &script_name2,
                            format!("{stage}: {err}"),
                        ));
                    }
                }
                RunUpdate::StageFinished { .. } => {}
                RunUpdate::StageVerdict { label, pass, violations, .. } => {
                    if !pass {
                        let _ = log_tx2.try_send(ScriptLogEntry::warning(
                            category2.clone(),
                            &script_name2,
                            format!("Stage {label} FAIL: {}", violations.join("; ")),
                        ));
                    }
                }
                RunUpdate::Finished(v) => {
                    let msg = format!(
                        "GPU probe {} in {:.1}s (run persisted)",
                        match v.result {
                            RunResult::Pass => "passed",
                            RunResult::Fail => "failed",
                            RunResult::Aborted => "aborted",
                            RunResult::Inconclusive => "inconclusive",
                            RunResult::InProgress => "in progress",
                        },
                        v.duration_secs
                    );
                    let entry = if v.result == RunResult::Pass {
                        ScriptLogEntry::success(category2.clone(), &script_name2, msg)
                    } else if v.result == RunResult::Aborted {
                        ScriptLogEntry::warning(category2.clone(), &script_name2, msg)
                    } else {
                        ScriptLogEntry::error(category2.clone(), &script_name2, msg)
                    };
                    let _ = log_tx2.try_send(entry);
                }
                RunUpdate::Warning { message } => {
                    let _ = log_tx2.try_send(ScriptLogEntry::warning(
                        category2.clone(),
                        &script_name2,
                        message,
                    ));
                }
                RunUpdate::Error { message } => {
                    let _ = log_tx2.try_send(ScriptLogEntry::error(
                        category2.clone(),
                        &script_name2,
                        message,
                    ));
                }
            });

            if verdict.is_none() {
                let _ = log_tx2.try_send(ScriptLogEntry::error(
                    category2,
                    &script_name2,
                    "GPU probe exited without a verdict",
                ));
            }
        });
    }

    /// Execute Data Transfer script
    fn execute_data_transfer(&mut self, log_tx: Sender<ScriptLogEntry>) {
        let _ = log_tx.try_send(ScriptLogEntry::info(
            ScriptCategory::Tuneup, "Data Transfer", "Scanning for user profiles..."
        ));
        
        let tx = self.data_transfer_tx.clone();
        std::thread::spawn(move || {
            match get_data_transfer_candidates() {
                Ok(paths) => { let _ = tx.try_send(paths); },
                Err(e) => log::error!("Error getting data transfer candidates: {e:?}"),
            }
        });
    }

    /// Start robocopy for data transfer
    pub fn start_data_transfer(&mut self, sources: Vec<String>, destination: String) {
        let robocopy_tx = self.robocopy_tx.clone();
        let log_tx = self.channels.log_tx.clone();
        
        for source in sources {
            let source_path = PathBuf::from(&source);
            let dest_path = PathBuf::from(&destination);
            let tx = robocopy_tx.clone();
            let log = log_tx.clone();
            
            let _ = log.try_send(ScriptLogEntry::info(
                ScriptCategory::Tuneup, "Data Transfer",
                format!("Starting transfer: {} -> {}", source, destination)
            ));
            
            tokio::spawn(async move {
                if let Err(e) = run_robocopy(&source_path, &dest_path, tx).await {
                    let _ = log.try_send(ScriptLogEntry::error(
                        ScriptCategory::Tuneup, "Data Transfer",
                        format!("Robocopy failed: {}", e)
                    ));
                }
            });
        }
        
        self.show_data_transfer_ui = false;
        self.selected_sources.clear();
        self.selected_destination = None;
    }

    /// Execute Activate CPS script
    fn execute_activate_cps(
        &self,
        service_number: Option<String>,
        client: Client,
        log_tx: Sender<ScriptLogEntry>,
        _progress_tx: Sender<(String, u64, u64)>,
        _script_id: String,
        category: ScriptCategory,
        script_name: String,
    ) {
        if let Some(so_num) = service_number {
            #[cfg(target_os = "windows")]
            {
                
                // Kill any running SAS processes first
                if let Ok(processes) = get_running_processes() {
                    for process in processes {
                        let name = process.process_name.to_lowercase();
                        let exe_path = process.exe_path.clone().unwrap_or_default().to_lowercase();
                        if name.contains("sascore") || exe_path.contains("superanti") || name.contains("superanti") {
                            let _ = log_tx.try_send(ScriptLogEntry::info(
                                category.clone(), &script_name,
                                format!("Killing SAS process (PID: {})", process.id)
                            ));
                            let _ = std::process::Command::new("taskkill")
                                .args(&["/PID", &format!("{}", process.id), "/F"])
                                .output();
                        }
                    }
                }
            }
            
            // Use the persistent install progress channel from self
            let install_progress_tx = self.install_progress_tx.clone();
            let install_progress_tx2 = self.install_progress_tx.clone();
            
            tokio::spawn(async move {
                let _ = log_tx.try_send(ScriptLogEntry::info(
                    category.clone(), &script_name, "Fetching CPS keys..."
                ));
                
                match SendRequest::get_cps(so_num, client.clone()).await {
                    Ok(keys) if !keys.is_empty() => {
                        let key = keys.get(0).cloned().unwrap_or_default();
                        
                        // Install Webroot
                        let _ = log_tx.try_send(ScriptLogEntry::info(
                            category.clone(), &script_name, "Installing Webroot..."
                        ));
                        
                        #[cfg(target_os = "windows")]
                        match install_webroot(key.webroot_key.clone(), client.clone(), install_progress_tx).await {
                            Ok(_) => {
                                let _ = log_tx.try_send(ScriptLogEntry::success(
                                    category.clone(), &script_name, "Webroot installed successfully"
                                ));
                            },
                            Err(e) => {
                                let _ = log_tx.try_send(ScriptLogEntry::error(
                                    category.clone(), &script_name, format!("Webroot install failed: {}", e)
                                ));
                            }
                        }
                        
                        // Install SAS
                        let _ = log_tx.try_send(ScriptLogEntry::info(
                            category.clone(), &script_name, "Installing SuperAntiSpyware..."
                        ));
                        
                        #[cfg(target_os = "windows")]
                        match install_sas(key.superanti_key, client, install_progress_tx2).await {
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
    }

    /// Execute Activate SEB script
    fn execute_activate_seb(
        &self,
        customer_email: Option<String>,
        client: Client,
        log_tx: Sender<ScriptLogEntry>,
        _progress_tx: Sender<(String, u64, u64)>,
        _script_id: String,
        category: ScriptCategory,
        script_name: String,
    ) {
        if let Some(email) = customer_email {
            #[cfg(target_os = "windows")]
            {
                // Use the persistent install progress channel from self
                let install_progress_tx = self.install_progress_tx.clone();
                tokio::spawn(async move {
                    let _ = log_tx.try_send(ScriptLogEntry::info(
                        category.clone(), &script_name, format!("Installing SuperEasyBackup for {}...", email)
                    ));
                    
                    match install_supereasybackup(email, client, install_progress_tx).await {
                        Ok(_) => {
                            let _ = log_tx.try_send(ScriptLogEntry::success(
                                category, &script_name, "SuperEasyBackup installed successfully"
                            ));
                        },
                        Err(e) => {
                            let _ = log_tx.try_send(ScriptLogEntry::error(
                                category, &script_name, format!("SEB install failed: {}", e)
                            ));
                        }
                    }
                });
            }
            
            #[cfg(not(target_os = "windows"))]
            let _ = log_tx.try_send(ScriptLogEntry::warning(
                category, &script_name, "SEB installation only available on Windows"
            ));
        } else {
            let _ = log_tx.try_send(ScriptLogEntry::warning(
                category, &script_name, "Customer email required for SEB activation"
            ));
        }
    }

    /// Execute Install Windows Updates script
    fn execute_install_windows_updates(
        &self,
        log_tx: Sender<ScriptLogEntry>,
        category: ScriptCategory,
        script_name: String,
    ) {
        let _ = log_tx.try_send(ScriptLogEntry::info(
            category.clone(), &script_name, "Starting Windows Updates..."
        ));
        
        #[cfg(target_os = "windows")]
        {
            let tx = self.windows_update_tx.clone();
            std::thread::spawn(move || {
                let _ = install_windows_updates(tx, true, true);
            });
        }
        
        #[cfg(not(target_os = "windows"))]
        let _ = log_tx.try_send(ScriptLogEntry::warning(
            category, &script_name, "Windows Updates only available on Windows"
        ));
    }

    /// Execute Disable Sleep script
    fn execute_disable_sleep(
        &self,
        log_tx: Sender<ScriptLogEntry>,
        category: ScriptCategory,
        script_name: String,
    ) {
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
        
        #[cfg(not(target_os = "windows"))]
        let _ = log_tx.try_send(ScriptLogEntry::warning(
            category, &script_name, "Sleep/hibernation control only available on Windows"
        ));
    }

    /// Execute SuperAntiSpyware Scan
    fn execute_sas_scan(
        &self,
        log_tx: Sender<ScriptLogEntry>,
        category: ScriptCategory,
        script_name: String,
    ) {
        #[cfg(target_os = "windows")]
        {
            std::thread::spawn(move || {
                let _ = log_tx.try_send(ScriptLogEntry::info(
                    category.clone(), &script_name, "Starting SuperAntiSpyware quick scan..."
                ));
                match crate::utilities::scripts::antivirus::run_sas_quick_scan() {
                    Ok(messages) => {
                        for message in messages {
                            let _ = log_tx.try_send(ScriptLogEntry::info(category.clone(), &script_name, message));
                        }
                        let _ = log_tx.try_send(ScriptLogEntry::success(
                            category, &script_name, "SAS quick scan started"
                        ));
                    }
                    Err(e) => {
                        let _ = log_tx.try_send(ScriptLogEntry::error(
                            category, &script_name, format!("SAS scan failed: {e}")
                        ));
                    }
                }
            });
        }

        #[cfg(not(target_os = "windows"))]
        let _ = log_tx.try_send(ScriptLogEntry::warning(
            category, &script_name, "SAS scan only available on Windows"
        ));
    }

    /// Execute Webroot Scan
    fn execute_webroot_scan(
        &self,
        log_tx: Sender<ScriptLogEntry>,
        category: ScriptCategory,
        script_name: String,
    ) {
        #[cfg(target_os = "windows")]
        {
            std::thread::spawn(move || {
                match crate::utilities::scripts::antivirus::start_webroot_scan() {
                    Ok(message) => {
                        let _ = log_tx.try_send(ScriptLogEntry::success(category, &script_name, message));
                    }
                    Err(e) => {
                        let _ = log_tx.try_send(ScriptLogEntry::error(
                            category, &script_name, format!("Webroot scan failed: {e}")
                        ));
                    }
                }
            });
        }

        #[cfg(not(target_os = "windows"))]
        let _ = log_tx.try_send(ScriptLogEntry::warning(
            category, &script_name, "Webroot scan only available on Windows"
        ));
    }

    /// Execute Install LibreOffice
    fn execute_install_libreoffice(
        &self,
        client: Client,
        log_tx: Sender<ScriptLogEntry>,
        _progress_tx: Sender<(String, u64, u64)>,
        _script_id: String,
        category: ScriptCategory,
        script_name: String,
    ) {
        let _ = log_tx.try_send(ScriptLogEntry::info(
            category.clone(), &script_name, "Downloading LibreOffice via Ninite..."
        ));
        
        #[cfg(target_os = "windows")]
        {
            // Use the persistent install progress channel from self
            let install_progress_tx = self.install_progress_tx.clone();
            tokio::spawn(async move {
                let download_url = "https://ninite.com/libreoffice/ninite.exe";
                match install_program(download_url.to_string(), client, install_progress_tx).await {
                    Ok(_) => {
                        let _ = log_tx.try_send(ScriptLogEntry::success(
                            category, &script_name, "LibreOffice installed successfully"
                        ));
                    },
                    Err(e) => {
                        let _ = log_tx.try_send(ScriptLogEntry::error(
                            category, &script_name, format!("LibreOffice install failed: {}", e)
                        ));
                    }
                }
            });
        }
        
        #[cfg(not(target_os = "windows"))]
        let _ = log_tx.try_send(ScriptLogEntry::warning(
            category, &script_name, "LibreOffice install only available on Windows"
        ));
    }

    /// Execute Disable Notifications
    fn execute_disable_notifications(
        &self,
        log_tx: Sender<ScriptLogEntry>,
        category: ScriptCategory,
        script_name: String,
    ) {
        #[cfg(target_os = "windows")]
        {
            std::thread::spawn(move || {
                let _ = log_tx.try_send(ScriptLogEntry::info(
                    category.clone(), &script_name, "Disabling Windows notifications..."
                ));
                
                let mut success_count = 0;
                let mut error_count = 0;
                
                macro_rules! run_reg_fn {
                    ($fn:expr, $name:expr) => {
                        match $fn() {
                            Ok(_) => {
                                success_count += 1;
                                let _ = log_tx.try_send(ScriptLogEntry::info(
                                    category.clone(), &script_name, format!("✓ {}", $name)
                                ));
                            },
                            Err(e) => {
                                error_count += 1;
                                let _ = log_tx.try_send(ScriptLogEntry::warning(
                                    category.clone(), &script_name, format!("✗ {}: {}", $name, e)
                                ));
                            }
                        }
                    };
                }
                
                run_reg_fn!(disable_notifications, "Push Notifications");
                run_reg_fn!(disable_lockscreen_notifications, "Lockscreen Notifications");
                run_reg_fn!(disable_content_delivery_allowed, "Content Delivery");
                run_reg_fn!(disable_silent_installed_apps_enabled, "Silent App Installs");
                run_reg_fn!(disable_subscribed_content_enabled, "Subscribed Content");
                run_reg_fn!(disable_system_pane_suggestions_enabled, "System Pane Suggestions");
                run_reg_fn!(disable_account_notifications, "Account Notifications");
                run_reg_fn!(enable_more_pins_layout, "More Pins Layout");
                run_reg_fn!(disable_start_account_notifications, "Start Account Notifications");
                run_reg_fn!(disable_recent_items_tracking, "Recent Items Tracking");
                run_reg_fn!(remove_chat_from_taskbar, "Remove Chat from Taskbar");
                
                let _ = log_tx.try_send(ScriptLogEntry::success(
                    category, &script_name,
                    format!("Completed: {} succeeded, {} failed", success_count, error_count)
                ));
            });
        }
        
        #[cfg(not(target_os = "windows"))]
        let _ = log_tx.try_send(ScriptLogEntry::warning(
            category, &script_name, "Notification control only available on Windows"
        ));
    }

    /// Execute Disable Startup Apps
    fn execute_disable_startup_apps(
        &self,
        log_tx: Sender<ScriptLogEntry>,
        category: ScriptCategory,
        script_name: String,
    ) {
                #[cfg(target_os = "windows")]
                {
            std::thread::spawn(move || {
                use crate::utilities::scripts::{disable_hkcu_startup_entries, onedrive_in_use};

                match disable_hkcu_startup_entries("msedge") {
                    Ok(messages) => for message in messages {
                        let _ = log_tx.try_send(ScriptLogEntry::info(
                            category.clone(), &script_name, format!("Edge: {message}")
                        ));
                    },
                    Err(e) => {
                        let _ = log_tx.try_send(ScriptLogEntry::error(
                            category.clone(), &script_name, format!("Edge startup: {e}")
                        ));
                    }
                }

                if onedrive_in_use() {
                    let _ = log_tx.try_send(ScriptLogEntry::info(
                        category.clone(), &script_name,
                        "OneDrive has a signed-in account; leaving its startup entry enabled."
                    ));
                } else {
                    match disable_hkcu_startup_entries("onedrive") {
                        Ok(messages) => for message in messages {
                            let _ = log_tx.try_send(ScriptLogEntry::info(
                                category.clone(), &script_name, format!("OneDrive: {message}")
                            ));
                        },
                        Err(e) => {
                            let _ = log_tx.try_send(ScriptLogEntry::error(
                                category.clone(), &script_name, format!("OneDrive startup: {e}")
                            ));
                        }
                    }
                    // Stop the running instance so sign-in prompts end immediately.
                    use std::os::windows::process::CommandExt;
                    let _ = std::process::Command::new("taskkill")
                        .args(["/F", "/IM", "OneDrive.exe"])
                        .creation_flags(0x08000000)
                        .output();
                }

                let _ = log_tx.try_send(ScriptLogEntry::success(
                    category, &script_name, "Startup apps processed"
                ));
            });
        }
        
        #[cfg(not(target_os = "windows"))]
        let _ = log_tx.try_send(ScriptLogEntry::warning(
            category, &script_name, "Startup apps control only available on Windows"
        ));
    }

    /// Execute Unpin Copilot
    fn execute_unpin_copilot(
        &self,
        log_tx: Sender<ScriptLogEntry>,
        category: ScriptCategory,
        script_name: String,
    ) {
        #[cfg(target_os = "windows")]
        {
            std::thread::spawn(move || {
                let _ = log_tx.try_send(ScriptLogEntry::info(
                    category.clone(), &script_name, "Unpinning Copilot from taskbar..."
                ));
                
                match disable_copilot() {
                    Ok(results) => {
                        for result in results {
                            let _ = log_tx.try_send(ScriptLogEntry::info(
                                category.clone(), &script_name, result
                            ));
                        }
                        let _ = log_tx.try_send(ScriptLogEntry::success(
                            category, &script_name, "Copilot unpinned successfully"
                        ));
                    },
                    Err(e) => {
                        let _ = log_tx.try_send(ScriptLogEntry::error(
                            category, &script_name, format!("Failed to unpin Copilot: {}", e)
                        ));
                    }
                }
            });
        }
        
        #[cfg(not(target_os = "windows"))]
        let _ = log_tx.try_send(ScriptLogEntry::warning(
            category, &script_name, "Copilot control only available on Windows"
        ));
    }

    /// Execute Align Taskbar to Left
    fn execute_align_taskbar(
        &self,
        log_tx: Sender<ScriptLogEntry>,
        category: ScriptCategory,
        script_name: String,
    ) {
        #[cfg(target_os = "windows")]
        {
            std::thread::spawn(move || {
                let _ = log_tx.try_send(ScriptLogEntry::info(
                    category.clone(), &script_name, "Aligning taskbar to left..."
                ));
                
                match align_taskbar_left() {
                    Ok(messages) => {
                        for message in messages {
                            let _ = log_tx.try_send(ScriptLogEntry::info(
                                category.clone(), &script_name, message.trim().to_string()
                            ));
                        }
                        let _ = log_tx.try_send(ScriptLogEntry::success(
                            category, &script_name, "Taskbar aligned to left"
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
        
        #[cfg(not(target_os = "windows"))]
        let _ = log_tx.try_send(ScriptLogEntry::warning(
            category, &script_name, "Taskbar alignment only available on Windows"
        ));
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
                self.execute_check_windows_activation(log_tx, category, script_name);
            },
            "Is SuperEasyBackup installed?" => {
                self.execute_check_program_installed(log_tx, category, script_name, "supereasybackup");
            },
            "Is Webroot installed?" => {
                self.execute_check_program_installed(log_tx, category, script_name, "webroot");
            },
            "Is SuperAntiSpyware installed?" => {
                self.execute_check_program_installed(log_tx, category, script_name, "superantispyware");
            },
            "Are there scheduled tasks for it?" => {
                self.execute_check_sas_scheduled_tasks(log_tx, category, script_name);
            },
            "Is Hibernation/Sleep enabled?" => {
                self.execute_check_power_settings(log_tx, category, script_name);
            },
            "Any Recent Blue Screens?" => {
                let _ = log_tx.try_send(ScriptLogEntry::warning(
                    category, &script_name, "BSOD check not yet implemented"
                ));
            },
            "When Was The Last Service Date?" => {
                let _ = log_tx.try_send(ScriptLogEntry::warning(
                    category, &script_name, "Service date check not yet implemented"
                ));
            },
            "Check Updates" => {
                self.execute_check_updates(log_tx, category, script_name);
            },
            "Run Prechecks" => {
                self.execute_run_prechecks(log_tx, category, script_name);
            },
            _ => {
                let _ = log_tx.try_send(ScriptLogEntry::warning(
                    category, &script_name, format!("Script '{}' not yet implemented", script_name)
                ));
            }
        }
    }

    /// Check Windows Activation
    fn execute_check_windows_activation(
        &self,
        log_tx: Sender<ScriptLogEntry>,
        category: ScriptCategory,
        script_name: String,
    ) {
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
        
        #[cfg(not(target_os = "windows"))]
        let _ = log_tx.try_send(ScriptLogEntry::warning(
            category, &script_name, "Windows activation check only available on Windows"
        ));
    }

    /// Check if a program is installed
    fn execute_check_program_installed(
        &self,
        log_tx: Sender<ScriptLogEntry>,
        category: ScriptCategory,
        script_name: String,
        search_term: &str,
    ) {
        let search = search_term.to_lowercase();
        
        #[cfg(target_os = "windows")]
        {
            std::thread::spawn(move || {
                let _ = log_tx.try_send(ScriptLogEntry::info(
                    category.clone(), &script_name, format!("Searching for {}...", search)
                ));
                
                if let Ok(programs) = InstalledProgram::get_installed_programs() {
                    for program in &programs {
                        let display_name = program.display_name.clone().unwrap_or_default().to_lowercase();
                        let publisher = program.publisher.clone().unwrap_or_default().to_lowercase();
                        
                        if display_name.contains(&search) || publisher.contains(&search) {
                            let _ = log_tx.try_send(ScriptLogEntry::success(
                                category.clone(), &script_name, format!("{} Found!", search)
                            ));
                            let _ = log_tx.try_send(ScriptLogEntry::info(
                                category.clone(), &script_name,
                                format!("  Display Name: {}", program.display_name.clone().unwrap_or_default())
                            ));
                            let _ = log_tx.try_send(ScriptLogEntry::info(
                                category.clone(), &script_name,
                                format!("  Version: {}", program.display_version.clone().unwrap_or_default())
                            ));
                            return;
                        }
                    }
                }
                
                // Check antivirus products if not found in installed programs
                if let Ok(av_products) = AntiVirusProduct::query_installed() {
                    for product in &av_products {
                        if product.display_name.to_lowercase().contains(&search) {
                            let _ = log_tx.try_send(ScriptLogEntry::success(
                                category.clone(), &script_name,
                                format!("{} Found (AV): {}", search, product.display_name)
                            ));
                            return;
                        }
                    }
                }
                
                let _ = log_tx.try_send(ScriptLogEntry::warning(
                    category, &script_name, format!("{} not installed", search)
                ));
            });
        }
        
        #[cfg(not(target_os = "windows"))]
        let _ = log_tx.try_send(ScriptLogEntry::warning(
            category, &script_name, "Program check only available on Windows"
        ));
    }

    /// Check SAS Scheduled Tasks
    fn execute_check_sas_scheduled_tasks(
        &self,
        log_tx: Sender<ScriptLogEntry>,
        category: ScriptCategory,
        script_name: String,
    ) {
        #[cfg(target_os = "windows")]
        {
            std::thread::spawn(move || {
                let _ = log_tx.try_send(ScriptLogEntry::info(
                    category.clone(), &script_name, "Checking scheduled tasks..."
                ));
                
                match ScheduledTask::list_tasks() {
                    Ok(tasks) => {
                        let sas_tasks: Vec<_> = tasks.iter()
                            .filter(|t| t.task_name.clone().unwrap_or_default().contains("SUPERAntiSpyware"))
                            .collect();
                        
                        if !sas_tasks.is_empty() {
                            let _ = log_tx.try_send(ScriptLogEntry::success(
                                category.clone(), &script_name,
                                format!("Found {} SAS scheduled task(s)", sas_tasks.len())
                            ));
                            for task in sas_tasks {
                                let _ = log_tx.try_send(ScriptLogEntry::info(
                                    category.clone(), &script_name,
                                    format!("  • {}", task.task_name.clone().unwrap_or_default())
                                ));
            }
        } else {
                            let _ = log_tx.try_send(ScriptLogEntry::warning(
                                category, &script_name, "No SAS scheduled tasks found"
                            ));
                        }
                    },
                    Err(e) => {
                        let _ = log_tx.try_send(ScriptLogEntry::error(
                            category, &script_name, format!("Failed to get tasks: {}", e)
                        ));
                    }
                }
            });
        }
        
        #[cfg(not(target_os = "windows"))]
        let _ = log_tx.try_send(ScriptLogEntry::warning(
            category, &script_name, "Scheduled task check only available on Windows"
        ));
    }

    /// Check Power Settings
    fn execute_check_power_settings(
        &self,
        log_tx: Sender<ScriptLogEntry>,
        category: ScriptCategory,
        script_name: String,
    ) {
        #[cfg(target_os = "windows")]
        {
            std::thread::spawn(move || {
                match check_power_options() {
                    Ok(_) => {
                        let _ = log_tx.try_send(ScriptLogEntry::success(
                            category, &script_name, "Sleep/Hibernation is disabled"
                        ));
                    },
                    Err(e) => {
                        let _ = log_tx.try_send(ScriptLogEntry::warning(
                            category, &script_name, format!("Power check: {}", e)
                        ));
                    }
                }
            });
        }
        
        #[cfg(not(target_os = "windows"))]
        let _ = log_tx.try_send(ScriptLogEntry::warning(
            category, &script_name, "Power check only available on Windows"
        ));
    }

    /// Check for Windows Updates
    fn execute_check_updates(
        &self,
        log_tx: Sender<ScriptLogEntry>,
        category: ScriptCategory,
        script_name: String,
    ) {
        let _ = log_tx.try_send(ScriptLogEntry::info(
            category.clone(), &script_name, "Checking for Windows Updates..."
        ));
        
        #[cfg(target_os = "windows")]
        {
            let tx = self.windows_update_tx.clone();
            std::thread::spawn(move || {
                let _ = install_windows_updates(tx, false, false); // Check only, don't install
            });
        }
        
        #[cfg(not(target_os = "windows"))]
        let _ = log_tx.try_send(ScriptLogEntry::warning(
            category, &script_name, "Windows Updates only available on Windows"
        ));
    }

    /// Run all prechecks
    fn execute_run_prechecks(
        &self,
        log_tx: Sender<ScriptLogEntry>,
        category: ScriptCategory,
        script_name: String,
    ) {
        let _ = log_tx.try_send(ScriptLogEntry::info(
            category.clone(), &script_name, "Running all prechecks..."
        ));
        
        // Run multiple checks
        self.execute_check_windows_activation(log_tx.clone(), category.clone(), "Windows Activation".to_string());
        self.execute_check_program_installed(log_tx.clone(), category.clone(), "Webroot Check".to_string(), "webroot");
        self.execute_check_program_installed(log_tx.clone(), category.clone(), "SAS Check".to_string(), "superantispyware");
        self.execute_check_program_installed(log_tx.clone(), category.clone(), "SEB Check".to_string(), "supereasybackup");
        self.execute_align_taskbar(log_tx.clone(), ScriptCategory::Tuneup, "Taskbar Alignment".to_string());
    }

    /// Execute a junkware removal script
    fn execute_junkware_script(
        &self,
        script: &ScriptItem,
        log_tx: Sender<ScriptLogEntry>,
    ) {
        let script_name = script.name.clone();
        let category = script.category.clone();

        match script_name.as_str() {
            "Uninstall Microsoft 365" => {
                self.execute_uninstall_microsoft365(log_tx, category, script_name);
            },
            "Uninstall OneDrive" => {
                self.execute_uninstall_onedrive(log_tx, category, script_name);
            },
            "Disable OneDrive Startup" => {
                self.execute_disable_onedrive_startup(log_tx, category, script_name);
            },
            "Disable Edge Startup Boost" => {
                self.execute_disable_edge_startup_boost(log_tx, category, script_name);
            },
            _ => {
                self.execute_remove_junkware(log_tx, category, script_name.clone(), &script_name);
            }
        }
    }

    /// Execute all junkware removal
    fn execute_all_junkware(&self, log_tx: Sender<ScriptLogEntry>) {
        let junkware_list = [
            "OneLaunch", "WebNavigator Browser", "Wave Browser", "Clear Browser",
            "Shift Browser", "Avast Browser", "Mcaffee Safe", "Driver Support", "Winzip"
        ];
        
        for junkware in junkware_list {
            self.execute_remove_junkware(
                log_tx.clone(),
                ScriptCategory::JunkwareRemoval,
                junkware.to_string(),
                junkware
            );
        }
    }

    /// Remove specific junkware
    fn execute_remove_junkware(
        &self,
        log_tx: Sender<ScriptLogEntry>,
        category: ScriptCategory,
        script_name: String,
        junkware_name: &str,
    ) {
        let junkware = junkware_name.to_string();
        
        #[cfg(target_os = "windows")]
        {
            std::thread::spawn(move || {
                let _ = log_tx.try_send(ScriptLogEntry::info(
                    category.clone(), &script_name, format!("Searching for {}...", junkware)
                ));
                
                let publisher_match = match junkware.as_str() {
                    "OneLaunch" => "onelaunch",
                    "WebNavigator Browser" => "webnavigator",
                    "Wave Browser" => "wavesor",
                    "Clear Browser" => "clear browser",
                    "Shift Browser" => "shift technologies",
                    "Avast Browser" => "avast",
                    "Mcaffee Safe" => "mcafee",
                    "Driver Support" => "driver support",
                    "Winzip" => "winzip",
                    _ => &junkware.to_lowercase(),
                };
                
                if let Ok(mut programs) = InstalledProgram::get_installed_programs() {
                    for program in &mut programs {
                        if let Some(publisher) = &program.publisher {
                            let publisher_lower = publisher.to_lowercase();
                            if publisher_lower.contains(publisher_match) {
                                let _ = log_tx.try_send(ScriptLogEntry::info(
                                    category.clone(), &script_name,
                                    format!("Found {}, attempting uninstall...", junkware)
                                ));
                                
                                match program.uninstall() {
                                    Ok(_) => {
                                        let _ = log_tx.try_send(ScriptLogEntry::success(
                                            category.clone(), &script_name,
                                            format!("Uninstalled {}", junkware)
                                        ));
                                    },
                                    Err(e) => {
                                        let _ = log_tx.try_send(ScriptLogEntry::error(
                                            category.clone(), &script_name,
                                            format!("Failed to uninstall {}: {}", junkware, e)
                                        ));
                                    }
                                }
                                return;
                            }
                        }
                    }
                }
                
                let _ = log_tx.try_send(ScriptLogEntry::info(
                    category, &script_name, format!("{} not found (OK)", junkware)
                ));
            });
        }
        
        #[cfg(not(target_os = "windows"))]
        let _ = log_tx.try_send(ScriptLogEntry::warning(
            category, &script_name, "Junkware removal only available on Windows"
        ));
    }

    /// Execute Change Timezone to Mountain script
    fn execute_change_timezone(
        &self,
        log_tx: Sender<ScriptLogEntry>,
        category: ScriptCategory,
        script_name: String,
    ) {
        #[cfg(target_os = "windows")]
        {
            std::thread::spawn(move || {
                use powershell_script::PsScriptBuilder;
                let _ = log_tx.try_send(ScriptLogEntry::info(
                    category.clone(), &script_name, "Setting timezone to Mountain Standard Time..."
                ));
                let ps = PsScriptBuilder::new()
                    .no_profile(true)
                    .non_interactive(true)
                    .hidden(true)
                    .print_commands(false)
                    .build();
                match ps.run("tzutil /s \"Mountain Standard Time\"") {
                    Ok(_) => {
                        let _ = log_tx.try_send(ScriptLogEntry::success(
                            category, &script_name, "Timezone set to Mountain Standard Time"
                        ));
                    },
                    Err(e) => {
                        let _ = log_tx.try_send(ScriptLogEntry::error(
                            category, &script_name, format!("Failed to set timezone: {}", e)
                        ));
                    }
                }
            });
        }

        #[cfg(not(target_os = "windows"))]
        let _ = log_tx.try_send(ScriptLogEntry::warning(
            category, &script_name, "Timezone change only available on Windows"
        ));
    }

    /// Execute Disable BitLocker script
    fn execute_disable_bitlocker(
        &self,
        log_tx: Sender<ScriptLogEntry>,
        category: ScriptCategory,
        script_name: String,
    ) {
        #[cfg(target_os = "windows")]
        {
            std::thread::spawn(move || {
                use powershell_script::PsScriptBuilder;
                let _ = log_tx.try_send(ScriptLogEntry::info(
                    category.clone(), &script_name, "Checking BitLocker status on all drives..."
                ));
                let ps = PsScriptBuilder::new()
                    .no_profile(true)
                    .non_interactive(true)
                    .hidden(true)
                    .print_commands(false)
                    .build();
                let check_script = r#"
                    $volumes = Get-BitLockerVolume -ErrorAction SilentlyContinue
                    if ($volumes) {
                        $volumes | ForEach-Object {
                            "$($_.MountPoint): $($_.VolumeStatus) / $($_.ProtectionStatus)"
                        }
                    } else {
                        "BitLocker not available or no encrypted volumes found"
                    }
                "#;
                match ps.run(check_script) {
                    Ok(output) => {
                        let stdout = output.stdout().unwrap_or_default();
                        for line in stdout.lines() {
                            let _ = log_tx.try_send(ScriptLogEntry::info(
                                category.clone(), &script_name, line.to_string()
                            ));
                        }
                    },
                    Err(e) => {
                        let _ = log_tx.try_send(ScriptLogEntry::warning(
                            category.clone(), &script_name,
                            format!("Could not query BitLocker volumes: {}", e)
                        ));
                    }
                }

                let disable_script = r#"
                    $volumes = Get-BitLockerVolume -ErrorAction SilentlyContinue |
                        Where-Object { $_.ProtectionStatus -eq 'On' -or $_.VolumeStatus -ne 'FullyDecrypted' }
                    if ($volumes) {
                        foreach ($vol in $volumes) {
                            Disable-BitLocker -MountPoint $vol.MountPoint -ErrorAction SilentlyContinue | Out-Null
                            "Disabling BitLocker on $($vol.MountPoint)"
                        }
                    } else {
                        "No BitLocker-protected volumes found"
                    }
                "#;
                let ps2 = PsScriptBuilder::new()
                    .no_profile(true)
                    .non_interactive(true)
                    .hidden(true)
                    .print_commands(false)
                    .build();
                match ps2.run(disable_script) {
                    Ok(output) => {
                        let stdout = output.stdout().unwrap_or_default();
                        for line in stdout.lines() {
                            let _ = log_tx.try_send(ScriptLogEntry::info(
                                category.clone(), &script_name, line.to_string()
                            ));
                        }
                        let _ = log_tx.try_send(ScriptLogEntry::success(
                            category, &script_name, "BitLocker disable command completed"
                        ));
                    },
                    Err(e) => {
                        let _ = log_tx.try_send(ScriptLogEntry::error(
                            category, &script_name, format!("Failed to disable BitLocker: {}", e)
                        ));
                    }
                }
            });
        }

        #[cfg(not(target_os = "windows"))]
        let _ = log_tx.try_send(ScriptLogEntry::warning(
            category, &script_name, "BitLocker control only available on Windows"
        ));
    }

    /// Execute Uninstall Microsoft 365 script
    fn execute_uninstall_microsoft365(
        &self,
        log_tx: Sender<ScriptLogEntry>,
        category: ScriptCategory,
        script_name: String,
    ) {
        #[cfg(target_os = "windows")]
        {
            std::thread::spawn(move || {
                use powershell_script::PsScriptBuilder;
                let _ = log_tx.try_send(ScriptLogEntry::info(
                    category.clone(), &script_name, "Searching for Microsoft 365 / Office installations..."
                ));
                let ps = PsScriptBuilder::new()
                    .no_profile(true)
                    .non_interactive(true)
                    .hidden(true)
                    .print_commands(false)
                    .build();
                let script = r#"
                    $paths = @(
                        "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*",
                        "HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*",
                        "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*"
                    )
                    $office = $paths | ForEach-Object {
                        if (Test-Path $_) {
                            Get-ItemProperty $_ -ErrorAction SilentlyContinue |
                                Where-Object { $_.DisplayName -match "Microsoft 365|Microsoft Office" }
                        }
                    }
                    if ($office) {
                        foreach ($app in $office) {
                            if ($app.UninstallString) {
                                "Found: $($app.DisplayName) — uninstalling..."
                                $cmd = $app.UninstallString
                                if ($cmd -match "OfficeClickToRun") {
                                    & "$env:CommonProgramFiles\Microsoft Shared\ClickToRun\OfficeC2RClient.exe" /update user displaylevel=false forceappshutdown=true updatepromptuser=false
                                    Start-Sleep -Seconds 2
                                    & "$env:CommonProgramFiles\Microsoft Shared\ClickToRun\OfficeC2RClient.exe" /uninstall displaylevel=false
                                } elseif ($cmd -match "MsiExec") {
                                    $productCode = ([regex]'\{[A-F0-9-]+\}').Match($cmd).Value
                                    if ($productCode) { msiexec /x $productCode /quiet /norestart }
                                } else {
                                    Invoke-Expression "& $cmd /silent /norestart" 2>$null
                                }
                            }
                        }
                        "Microsoft 365/Office uninstall initiated"
                    } else {
                        "Microsoft 365/Office not found"
                    }
                "#;
                match ps.run(script) {
                    Ok(output) => {
                        let stdout = output.stdout().unwrap_or_default();
                        for line in stdout.lines() {
                            let _ = log_tx.try_send(ScriptLogEntry::info(
                                category.clone(), &script_name, line.to_string()
                            ));
                        }
                        let _ = log_tx.try_send(ScriptLogEntry::success(
                            category, &script_name, "Microsoft 365 uninstall script completed"
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

        #[cfg(not(target_os = "windows"))]
        let _ = log_tx.try_send(ScriptLogEntry::warning(
            category, &script_name, "Microsoft 365 uninstall only available on Windows"
        ));
    }

    /// Execute Uninstall OneDrive script
    fn execute_uninstall_onedrive(
        &self,
        log_tx: Sender<ScriptLogEntry>,
        category: ScriptCategory,
        script_name: String,
    ) {
        #[cfg(target_os = "windows")]
        {
            std::thread::spawn(move || {
                use powershell_script::PsScriptBuilder;
                let _ = log_tx.try_send(ScriptLogEntry::info(
                    category.clone(), &script_name, "Uninstalling OneDrive..."
                ));
                let ps = PsScriptBuilder::new()
                    .no_profile(true)
                    .non_interactive(true)
                    .hidden(true)
                    .print_commands(false)
                    .build();
                let script = r#"
                    taskkill /F /IM OneDrive.exe 2>$null
                    Start-Sleep -Seconds 1
                    $setup64 = "$env:SystemRoot\SysWOW64\OneDriveSetup.exe"
                    $setup32 = "$env:SystemRoot\System32\OneDriveSetup.exe"
                    if (Test-Path $setup64) {
                        & $setup64 /uninstall
                        "OneDrive (64-bit) uninstall initiated"
                    } elseif (Test-Path $setup32) {
                        & $setup32 /uninstall
                        "OneDrive (32-bit) uninstall initiated"
                    } else {
                        "OneDriveSetup.exe not found, trying winget..."
                        winget uninstall "Microsoft.OneDrive" --silent --accept-source-agreements 2>$null
                    }
                "#;
                match ps.run(script) {
                    Ok(output) => {
                        let stdout = output.stdout().unwrap_or_default();
                        for line in stdout.lines() {
                            let _ = log_tx.try_send(ScriptLogEntry::info(
                                category.clone(), &script_name, line.to_string()
                            ));
                        }
                        let _ = log_tx.try_send(ScriptLogEntry::success(
                            category, &script_name, "OneDrive uninstall completed"
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

        #[cfg(not(target_os = "windows"))]
        let _ = log_tx.try_send(ScriptLogEntry::warning(
            category, &script_name, "OneDrive uninstall only available on Windows"
        ));
    }

    /// Execute Disable OneDrive Startup script
    fn execute_disable_onedrive_startup(
        &self,
        log_tx: Sender<ScriptLogEntry>,
        category: ScriptCategory,
        script_name: String,
    ) {
        #[cfg(target_os = "windows")]
        {
            std::thread::spawn(move || {
                use powershell_script::PsScriptBuilder;
                let _ = log_tx.try_send(ScriptLogEntry::info(
                    category.clone(), &script_name, "Disabling OneDrive startup..."
                ));
                let ps = PsScriptBuilder::new()
                    .no_profile(true)
                    .non_interactive(true)
                    .hidden(true)
                    .print_commands(false)
                    .build();
                let script = r#"
                    $runKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
                    if (Get-ItemProperty -Path $runKey -Name "OneDrive" -ErrorAction SilentlyContinue) {
                        Remove-ItemProperty -Path $runKey -Name "OneDrive" -ErrorAction SilentlyContinue
                        "Removed OneDrive from HKCU Run key"
                    } else {
                        "OneDrive not found in Run key"
                    }
                    $odPolicies = "HKLM:\SOFTWARE\Policies\Microsoft\OneDrive"
                    if (-not (Test-Path $odPolicies)) { New-Item -Path $odPolicies -Force | Out-Null }
                    Set-ItemProperty -Path $odPolicies -Name "KFMBlockOptIn" -Value 1 -Type DWord
                    "OneDrive Known Folder Move blocked via policy"
                    taskkill /F /IM OneDrive.exe 2>$null
                    "OneDrive process terminated"
                "#;
                match ps.run(script) {
                    Ok(output) => {
                        let stdout = output.stdout().unwrap_or_default();
                        for line in stdout.lines() {
                            let _ = log_tx.try_send(ScriptLogEntry::info(
                                category.clone(), &script_name, line.to_string()
                            ));
                        }
                        let _ = log_tx.try_send(ScriptLogEntry::success(
                            category, &script_name, "OneDrive startup disabled"
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

        #[cfg(not(target_os = "windows"))]
        let _ = log_tx.try_send(ScriptLogEntry::warning(
            category, &script_name, "OneDrive startup control only available on Windows"
        ));
    }

    /// Execute Disable Edge Startup Boost script
    fn execute_disable_edge_startup_boost(
        &self,
        log_tx: Sender<ScriptLogEntry>,
        category: ScriptCategory,
        script_name: String,
    ) {
        #[cfg(target_os = "windows")]
        {
            std::thread::spawn(move || {
                use powershell_script::PsScriptBuilder;
                let _ = log_tx.try_send(ScriptLogEntry::info(
                    category.clone(), &script_name, "Disabling Edge startup boost and background running..."
                ));
                let ps = PsScriptBuilder::new()
                    .no_profile(true)
                    .non_interactive(true)
                    .hidden(true)
                    .print_commands(false)
                    .build();
                let script = r#"
                    $edgePolicy = "HKLM:\SOFTWARE\Policies\Microsoft\Edge"
                    if (-not (Test-Path $edgePolicy)) { New-Item -Path $edgePolicy -Force | Out-Null }
                    Set-ItemProperty -Path $edgePolicy -Name "StartupBoostEnabled" -Value 0 -Type DWord
                    "Edge StartupBoost disabled via policy"
                    Set-ItemProperty -Path $edgePolicy -Name "BackgroundModeEnabled" -Value 0 -Type DWord
                    "Edge BackgroundMode disabled via policy"
                    $runKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
                    if (Get-ItemProperty -Path $runKey -Name "MicrosoftEdge*" -ErrorAction SilentlyContinue) {
                        Remove-ItemProperty -Path $runKey -Name "MicrosoftEdge*" -ErrorAction SilentlyContinue
                        "Removed Edge from HKCU Run key"
                    }
                    taskkill /F /IM msedge.exe 2>$null
                    "Edge process terminated"
                "#;
                match ps.run(script) {
                    Ok(output) => {
                        let stdout = output.stdout().unwrap_or_default();
                        for line in stdout.lines() {
                            let _ = log_tx.try_send(ScriptLogEntry::info(
                                category.clone(), &script_name, line.to_string()
                            ));
                        }
                        let _ = log_tx.try_send(ScriptLogEntry::success(
                            category, &script_name, "Edge startup boost disabled"
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

        #[cfg(not(target_os = "windows"))]
        let _ = log_tx.try_send(ScriptLogEntry::warning(
            category, &script_name, "Edge startup boost control only available on Windows"
        ));
    }

    /// Log helper methods
    fn log_info(&mut self, script: &str, message: impl Into<String>) {        self.state.log(ScriptLogEntry::info(
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
}

// ============================================================================
// Windows-specific helper functions
// ============================================================================

#[cfg(target_os = "windows")]
fn disable_hibernation_and_sleep() -> anyhow::Result<bool> {
    crate::terminal_mode::tabs::script_categories::disable_hibernation_and_sleep()
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

/// Get data transfer candidates (user profiles with sizes)
#[cfg(target_os = "windows")]
pub fn get_data_transfer_candidates() -> anyhow::Result<Vec<(String, String)>> {
    use std::path::Path;
    use sysinfo::Disks;
    use walkdir::WalkDir;
    
    let disks = Disks::new_with_refreshed_list();
    let mount_points: Vec<&Path> = disks.iter().map(|d| d.mount_point()).collect();

    let mut paths_with_sizes = Vec::new();

    for drive in mount_points {
        let users_path = drive.join("Users");
        if !users_path.exists() {
            continue;
        }
        
        let results: Vec<PathBuf> = WalkDir::new(&users_path)
            .min_depth(1)
            .max_depth(1)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|entry| entry.path().is_dir())
            .map(|entry| entry.path().to_path_buf())
            .filter(|path| {
                let exclude = path.file_name()
                    .map(|name| {
                        let name_str = name.to_string_lossy().to_lowercase();
                        name_str.contains("default") 
                        || name_str == "all users"
                        || name_str == "public"
                    })
                    .unwrap_or(false);
                !exclude
            })
            .collect();
        
        for path in results {
            let dir_size = get_directory_size(&path);
            let formatted_size = format_size(dir_size);
            paths_with_sizes.push((path.to_string_lossy().to_string(), formatted_size));
        }
    }

    Ok(paths_with_sizes)
}

#[cfg(target_os = "windows")]
fn get_directory_size(path: &PathBuf) -> u64 {
    use walkdir::WalkDir;
    WalkDir::new(path)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter_map(|e| e.metadata().ok())
        .filter(|metadata| metadata.is_file())
        .map(|metadata| metadata.len())
        .sum()
}

#[cfg(target_os = "windows")]
fn format_size(bytes: u64) -> String {
    const MB: u64 = 1024 * 1024;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    }
}

#[cfg(not(target_os = "windows"))]
pub fn get_data_transfer_candidates() -> anyhow::Result<Vec<(String, String)>> {
    Ok(Vec::new())
}

// ============================================================================
// UI Integration with MastertechContext
// ============================================================================

impl MastertechContext {
    /// Render the new scripts UI
    pub fn scripts(&mut self, ui: &mut Ui) {
        // Sync service number from ticket data
        if !self.ticket_data.service_number.is_empty() {
            self.scripts_tab.service_number_input = self.ticket_data.service_number.clone();
        }
        
        // Sync customer email
        if !self.customer_data.email.is_empty() {
            self.scripts_tab.customer_email = Some(self.customer_data.email.clone());
        }

        // Show data transfer UI if needed
        if self.scripts_tab.show_data_transfer_ui {
            self.render_data_transfer_ui(ui);
            return;
        }

        // Top bar with service number and controls
        ui.horizontal(|ui| {
            ui.add_space(8.0);
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

        ui.add_space(2.0);
        ui.separator();
        ui.add_space(2.0);

        // Three-column layout
        let available_width = ui.available_width() / 1.2;
        let available_height = ui.available_height();
        let left_width = available_width * 0.10;
        let middle_width = available_width * 0.20;
        let right_width = available_width * 0.5;
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            ui.columns(3, |ui| {
                // Left: Categories
                ui[0].vertical(|ui| {
                    ui.set_min_size(vec2(left_width, available_height));
                    self.render_categories_panel(ui);
                });

                // Middle: Queue
                ui[1].vertical(|ui| {
                    ui.set_min_size(vec2(middle_width, available_height));
                    self.render_queue_panel(ui);
                });

                // Right: Logs
                ui[2].vertical(|ui| {
                    ui.set_min_size(vec2(right_width, available_height));
                    self.render_logs_panel(ui);
                });
            });
        });
    }

    /// Render Data Transfer UI
    fn render_data_transfer_ui(&mut self, ui: &mut Ui) {
        egui::Frame::default()
            .fill(colors::PANEL_BG)
            .inner_margin(16.0)
            .corner_radius(8.0)
            .show(ui, |ui| {
                ui.heading(RichText::new("📂 Data Transfer").color(colors::CATEGORY_HEADER));
                ui.add_space(8.0);
                ui.label("Select source folders to transfer and a destination:");
                ui.add_space(16.0);

                // Sources
                ui.label(RichText::new("Source Folders:").strong());
                let candidates = self.scripts_tab.data_transfer_candidates.clone();
                egui::ScrollArea::vertical()
                    .max_height(200.0)
                    .show(ui, |ui| {
                        for (path, size) in &candidates {
                            let is_selected = self.scripts_tab.selected_sources.contains(path);
                            let text = format!("{} ({})", path, size);
                            let mut checked = is_selected;
                            if ui.checkbox(&mut checked, &text).changed() {
                                if checked {
                                    self.scripts_tab.selected_sources.push(path.clone());
                                } else {
                                    self.scripts_tab.selected_sources.retain(|p| p != path);
                                }
                            }
                        }
                    });

                ui.add_space(16.0);

                // Destination selection
                ui.label(RichText::new("Destination:").strong());
                let dest_display = self.scripts_tab.selected_destination.clone()
                    .unwrap_or_else(|| "Select destination...".to_string());
                ui.horizontal(|ui| {
                    ui.label(&dest_display);
                    if ui.button("Browse...").clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_folder() {
                            self.scripts_tab.selected_destination = Some(path.to_string_lossy().to_string());
                        }
                    }
                });

                ui.add_space(24.0);

                // Action buttons
                ui.horizontal(|ui| {
                    let can_start = !self.scripts_tab.selected_sources.is_empty() 
                        && self.scripts_tab.selected_destination.is_some();
                    
                    if ui.add_enabled(can_start, egui::Button::new(
                        RichText::new("▶ Start Transfer").color(colors::COMPLETED)
                    )).clicked() {
                        let sources = self.scripts_tab.selected_sources.clone();
                        let dest = self.scripts_tab.selected_destination.clone().unwrap();
                        self.scripts_tab.start_data_transfer(sources, dest);
                    }

                    if ui.button(RichText::new("✕ Cancel").color(colors::FAILED)).clicked() {
                        self.scripts_tab.show_data_transfer_ui = false;
                        self.scripts_tab.selected_sources.clear();
                        self.scripts_tab.selected_destination = None;
                    }
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
            let collapse_icon = if expanded { "⏷" } else { "⏵" };
            if Button::new(
                RichText::new(format!("{collapse_icon}  {icon}  {name} ")).strong().color(colors::CATEGORY_HEADER)
            )
            .min_size(vec2(100.0, 20.0))
            .ui(ui)
            .clicked() {
                self.scripts_tab.state.category_expanded.insert(category.clone(), !expanded);
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(scripts) = self.scripts_tab.state.categories.get(category) {
                    let any_selected = scripts.iter().any(|s| s.is_selected());
                    let btn_text = if any_selected { " 🗙 " } else { " ✅ " };
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
                                                        if ui.small_button("⬆").clicked() {
                                                            move_action = Some((i, i - 1));
                                                        }
                                                    } else {
                                                        ui.add_enabled(false, egui::Button::new("⬆").small());
                                                    }
                                                    if i < queue_len - 1 {
                                                        if ui.small_button("⬇").clicked() {
                                                            move_action = Some((i, i + 1));
                                                        }
                                                    } else {
                                                        ui.add_enabled(false, egui::Button::new("⬇").small());
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
                                                    if ui.small_button("🗙").clicked() {
                                                        remove_index = Some(i);
                                                    }
                                                    
                                                    let status_text = match item.script.status {
                                                        ScriptStatus::Running => "⏳",
                                                        ScriptStatus::Completed => "✅",
                                                        ScriptStatus::Failed => "🗙",
                                                        ScriptStatus::Skipped => "⏩",
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
                                LogLevel::Success => "✅",
                                LogLevel::Warning => "⚠",
                                LogLevel::Error => "🗙",
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