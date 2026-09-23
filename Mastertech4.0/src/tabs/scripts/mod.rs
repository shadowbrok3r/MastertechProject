//! Scripts tab for Mastertech egui application
//! 
//! Uses the shared scripts module from displays crate and adds 
//! Windows-specific script executors.

use crate::tabs::file_browser::command::{run_robocopy, RobocopyMessage};
use displays::scripts::catalog::{CATALOG, ScriptDef, Surface};
use displays::scripts::executor::{CancelToken, ScriptHandle, ScriptOutcome, ScriptResult};
use displays::scripts::id::ScriptId;
use displays::scripts::{
    ScriptCategory, ScriptChannels, ScriptContext, ScriptLogEntry,
    ScriptStatus, ScriptsState, LogLevel,
    script_run_request_receiver, script_run_result_sender,
    ScriptRunRequest, ScriptRunResult,
};
use crossbeam::channel::{Receiver, Sender};
#[allow(unused_imports)]
use futures::StreamExt;
use rust_embed::Embed;
use std::collections::HashMap;
use std::path::PathBuf;

#[allow(unused_imports)]
use tokio::{fs, io::{self, AsyncWriteExt}, process::Command};

#[cfg(target_os = "windows")]
#[allow(unused_imports)]
use crate::utilities::windows::antivirus::check_antivirus;

mod view;

#[derive(Embed)]
#[folder = "src/assets/superanti/"]
pub struct SasAsset;

#[cfg(target_os = "windows")]
#[allow(unused_imports)]
use wmi::{WMIConnection, WMIError};

#[cfg(target_os = "windows")]
#[allow(dead_code)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Budget for a script the catalog gives no timeout.
const QUEUE_SCRIPT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60 * 60);
const DATA_TRANSFER: &str = "data-transfer";

/// A Data Transfer entry, finished from the picker or when robocopy returns.
#[derive(Clone)]
struct TransferRun {
    id: ScriptId,
    run_token: u64,
    started: std::time::Instant,
    done: Sender<ScriptOutcome>,
}

impl TransferRun {
    fn finish(&self, result: ScriptResult) {
        let outcome =
            ScriptOutcome::plain(self.id.clone(), self.run_token, result, self.started.elapsed());
        let _ = self.done.try_send(outcome);
    }
}

/// Egui Scripts Tab state
pub struct EguiScriptsTab {
    /// Shared scripts state (categories, queue, logs)
    pub state: ScriptsState,
    /// Communication channels
    pub channels: ScriptChannels,
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
    /// Is data transfer UI showing
    pub show_data_transfer_ui: bool,
    /// A completed script recommends a reboot; drives the reboot prompt modal.
    pub reboot_prompt_open: bool,
    /// Selected source paths for data transfer
    pub selected_sources: Vec<String>,
    /// Selected destination for data transfer
    pub selected_destination: Option<String>,
    /// In-flight runs requested by the MCP `scripts_run` tool, resolved by their outcomes.
    pub pending_mcp_runs: Vec<McpPendingRun>,
    /// diagnostic_session id from the latest MCP scripts_run request.
    pub mcp_diagnostic_session_id: Option<String>,
    /// Completion handle for the running entry.
    running: Option<ScriptHandle>,
    /// When the running entry started, for its time budget.
    run_started: Option<std::time::Instant>,
    /// Entry still finishing after Stop, as (run token, name).
    stopping: Option<(u64, String)>,
    /// The Data Transfer entry waiting on the picker or on robocopy.
    transfer_run: Option<TransferRun>,
    /// Last ticket service number copied into the input.
    pub seeded_service_number: String,
    pub search: String,
    /// `None` shows every level.
    pub log_filter: Option<LogLevel>,
    pub focus: Option<view::ScriptFocus>,
    /// Completed outcomes, keyed by queue entry.
    pub outcomes: HashMap<u64, ScriptOutcome>,
}

/// One in-flight MCP-initiated script run.
pub struct McpPendingRun {
    pub request_id: String,
    pub script_name: String,
    /// Log cursor at dispatch; the run's lines from here are returned with its result.
    pub log_start_index: usize,
    pub dispatched_at: std::time::Instant,
    /// The script's own budget from the catalog.
    pub timeout: std::time::Duration,
    handle: ScriptHandle,
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
        
        Self {
            state: {
                let mut state = ScriptsState::new();
                state.categories = CATALOG.items_for(Surface::Egui);
                state
            },
            channels: ScriptChannels::default(),
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
            show_data_transfer_ui: false,
            reboot_prompt_open: false,
            selected_sources: Vec::new(),
            selected_destination: None,
            pending_mcp_runs: Vec::new(),
            mcp_diagnostic_session_id: None,
            running: None,
            run_started: None,
            stopping: None,
            transfer_run: None,
            seeded_service_number: String::new(),
            search: String::new(),
            log_filter: None,
            focus: None,
            outcomes: HashMap::new(),
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
        if let Some(sn) = req.service_number.as_deref()
            && !sn.is_empty()
        {
            self.service_number_input = sn.to_string();
        }
        if let Some(em) = req.customer_email.as_deref()
            && !em.is_empty()
        {
            self.customer_email = Some(em.to_string());
        }
        self.mcp_diagnostic_session_id = req
            .diagnostic_session_id
            .clone()
            .filter(|s| !s.trim().is_empty());

        let log_start_index = self.state.log_cursor();
        self.log_info(
            "MCP",
            format!(
                "MCP requested: run '{}' (category {:?}, request_id {})",
                req.script_name, req.category, req.request_id
            ),
        );

        let refuse = |request_id: String, message: String| {
            let _ = script_run_result_sender().send(ScriptRunResult {
                request_id,
                success: false,
                message,
                logs: Vec::new(),
            });
        };

        let def = match mcp_script(&req.script_name) {
            Ok(def) => def,
            Err(message) => {
                refuse(req.request_id, message);
                return;
            }
        };

        let handle = if def.id.as_str() == DATA_TRANSFER {
            if self.transfer_run.is_some() {
                refuse(req.request_id, "A data transfer is already waiting".into());
                return;
            }
            let (tx, done) = crossbeam::channel::bounded(1);
            self.transfer_run = Some(TransferRun {
                id: def.id.clone(),
                run_token: 0,
                started: std::time::Instant::now(),
                done: tx,
            });
            let log_tx = self.channels.log_tx.clone();
            self.execute_data_transfer(log_tx);
            ScriptHandle {
                run_token: 0,
                done,
                cancel: CancelToken::new(),
            }
        } else {
            let ctx = ScriptContext {
                surface: Some(Surface::Mcp),
                ..self.get_context()
            };
            crate::scripts_exec::registry().spawn(def, &ctx, 0, CancelToken::new())
        };

        self.pending_mcp_runs.push(McpPendingRun {
            request_id: req.request_id,
            script_name: req.script_name,
            log_start_index,
            dispatched_at: std::time::Instant::now(),
            timeout: std::time::Duration::from_secs(def.timeout_secs),
            handle,
        });
    }

    /// Reports each MCP run whose outcome has arrived, or whose budget ran out.
    pub fn process_mcp_completions(&mut self) {
        if self.pending_mcp_runs.is_empty() {
            return;
        }

        let mut ready: Vec<(usize, bool, String)> = Vec::new();
        for (idx, pending) in self.pending_mcp_runs.iter().enumerate() {
            match pending.handle.done.try_recv() {
                Ok(outcome) => {
                    let message = outcome.result.message().to_string();
                    ready.push((idx, outcome.result.is_success(), message));
                }
                Err(crossbeam::channel::TryRecvError::Empty) => {
                    if pending.dispatched_at.elapsed() > pending.timeout {
                        pending.handle.cancel.cancel();
                        let message = format!(
                            "Script '{}' did not finish within {}s. It may still be running on the host.",
                            pending.script_name,
                            pending.timeout.as_secs()
                        );
                        ready.push((idx, false, message));
                    }
                }
                Err(crossbeam::channel::TryRecvError::Disconnected) => {
                    let message = format!("Script '{}' stopped without reporting", pending.script_name);
                    ready.push((idx, false, message));
                }
            }
        }
        if ready.is_empty() {
            return;
        }

        // Each worker logs before it reports, so its last lines are already queued.
        self.drain_logs();
        let end = self.state.log_cursor();
        for (idx, success, message) in &ready {
            let pending = &self.pending_mcp_runs[*idx];
            let _ = script_run_result_sender().send(ScriptRunResult {
                request_id: pending.request_id.clone(),
                success: *success,
                message: message.clone(),
                logs: self.collect_pending_logs(pending, end),
            });
        }
        for (idx, _, _) in ready.iter().rev() {
            self.pending_mcp_runs.remove(*idx);
        }
    }

    fn collect_pending_logs(&self, pending: &McpPendingRun, end_index: usize) -> Vec<String> {
        self.state
            .logs_between(pending.log_start_index, end_index)
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
    fn drain_logs(&mut self) {
        while let Ok(log_entry) = self.channels.log_rx.try_recv() {
            if log_entry.message.contains(displays::scripts::REBOOT_RECOMMENDED_MARKER) {
                self.reboot_prompt_open = true;
            }
            self.state.log(log_entry);
        }
    }

    pub fn receive(&mut self) {
        self.drain_logs();

        // Receive progress updates
        while let Ok((_script_id, current, total)) = self.channels.progress_rx.try_recv() {
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
            diagnostic_session_id: self.mcp_diagnostic_session_id.clone(),
            surface: Some(Surface::Egui),
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

    /// Queues each listed script the tab offers, skipping any already waiting in the queue.
    pub fn queue_ids(&mut self, label: &str, ids: &[String]) {
        let mut added = 0;
        for id in ids {
            let Some(item) = self
                .state
                .categories
                .values()
                .flatten()
                .find(|s| CATALOG.id_for_legacy_name(&s.name).is_some_and(|i| i.as_str() == id))
                .cloned()
            else {
                continue;
            };
            let waiting = self
                .state
                .queue
                .items()
                .iter()
                .any(|q| q.script.name == item.name && q.script.status == ScriptStatus::Pending);
            if waiting {
                continue;
            }
            let mut item = item;
            item.selected = false;
            item.status = ScriptStatus::Pending;
            self.state.queue.add(item);
            added += 1;
        }
        self.log_info("Queue", format!("{label}: queued {added} scripts"));
    }

    /// Stops the queue and signals the running entry. Its outcome is still collected,
    /// and nothing new starts until it has actually finished.
    pub fn stop_queue(&mut self) {
        let current = self
            .state
            .queue
            .current_script()
            .map(|q| (q.run_token, q.script.name.clone()));
        if self.show_data_transfer_ui {
            self.cancel_data_transfer();
        }
        match (self.running.as_ref(), current) {
            (Some(handle), Some(current)) => {
                handle.cancel.cancel();
                self.log_info(
                    "Queue",
                    format!("Stop requested; waiting for {} to finish", current.1),
                );
                self.stopping = Some(current);
            }
            _ => {
                self.running = None;
                self.run_started = None;
            }
        }
        self.state.queue.stop();
        // Shows the stopping entry as running until it settles.
        if let Some((token, _)) = self.stopping.clone() {
            self.set_entry_status(token, ScriptStatus::Running);
        }
        self.current_script_name = None;
    }

    /// The entry still finishing after Stop, if any.
    pub fn stopping_name(&self) -> Option<&str> {
        self.stopping.as_ref().map(|(_, name)| name.as_str())
    }

    /// Stops waiting for an entry that ignores Stop. Its worker is left to finish on its own.
    pub fn abandon_stopped_run(&mut self) {
        let Some((token, name)) = self.stopping.take() else {
            return;
        };
        self.set_entry_status(token, ScriptStatus::Failed);
        self.running = None;
        self.run_started = None;
        self.transfer_run = None;
        self.log_warning("Queue", format!("Stopped waiting for {name}; it may still be running"));
    }

    /// Collects the outcome of the entry that was running when Stop was pressed.
    fn settle_stopped_run(&mut self) {
        let Some((token, name)) = self.stopping.clone() else {
            return;
        };
        let Some(handle) = self.running.as_ref() else {
            self.stopping = None;
            return;
        };
        let outcome = match handle.done.try_recv() {
            Ok(outcome) => Some(outcome),
            Err(crossbeam::channel::TryRecvError::Empty) => {
                let budget = CATALOG
                    .timeout_secs(&name)
                    .map(std::time::Duration::from_secs)
                    .unwrap_or(QUEUE_SCRIPT_TIMEOUT);
                if self.run_started.is_some_and(|at| at.elapsed() >= budget) {
                    self.abandon_stopped_run();
                }
                return;
            }
            Err(crossbeam::channel::TryRecvError::Disconnected) => None,
        };
        let status = match outcome.as_ref().map(|o| &o.result) {
            Some(ScriptResult::Success(_) | ScriptResult::Warning(_)) => ScriptStatus::Completed,
            Some(ScriptResult::Skipped(_)) => ScriptStatus::Skipped,
            Some(ScriptResult::Error(_)) | None => ScriptStatus::Failed,
        };
        self.set_entry_status(token, status);
        if let Some(outcome) = outcome {
            self.outcomes.insert(token, outcome);
        }
        self.stopping = None;
        self.running = None;
        self.run_started = None;
        self.transfer_run = None;
        self.log_info("Queue", format!("Stopped; {name} finished"));
    }

    fn set_entry_status(&mut self, token: u64, status: ScriptStatus) {
        if let Some(entry) = self
            .state
            .queue
            .items_mut()
            .iter_mut()
            .find(|q| q.run_token == token)
        {
            entry.script.status = status;
        }
    }

    pub fn run_queue(&mut self) {
        if let Some(name) = self.stopping_name() {
            let msg = format!("{name} is still stopping; wait for it to finish");
            self.log_warning("Queue", msg);
            return;
        }
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

    /// Starts the current entry. Every entry reports its own completion through `running`.
    fn execute_next_script(&mut self) {
        let Some((script, run_token)) = self
            .state
            .queue
            .current_script()
            .map(|q| (q.script.clone(), q.run_token))
        else {
            return;
        };
        self.current_script_name = Some(script.name.clone());
        self.run_started = Some(std::time::Instant::now());
        self.log_info(&script.name, format!("Starting: {}", script.name));

        let Some(def) = CATALOG
            .id_for_legacy_name(&script.name)
            .and_then(|id| CATALOG.get(id))
        else {
            let (tx, done) = crossbeam::channel::bounded(1);
            let _ = tx.send(ScriptOutcome::plain(
                ScriptId::new(script.name.as_str()),
                run_token,
                ScriptResult::Error(format!("'{}' is not in the script catalog", script.name)),
                std::time::Duration::ZERO,
            ));
            self.running = Some(ScriptHandle {
                run_token,
                done,
                cancel: CancelToken::new(),
            });
            return;
        };

        if def.id.as_str() == DATA_TRANSFER {
            let (tx, done) = crossbeam::channel::bounded(1);
            self.transfer_run = Some(TransferRun {
                id: def.id.clone(),
                run_token,
                started: std::time::Instant::now(),
                done: tx,
            });
            self.running = Some(ScriptHandle {
                run_token,
                done,
                cancel: CancelToken::new(),
            });
            let log_tx = self.channels.log_tx.clone();
            self.execute_data_transfer(log_tx);
            return;
        }

        let ctx = self.get_context();
        self.running = Some(crate::scripts_exec::registry().spawn(
            def,
            &ctx,
            run_token,
            CancelToken::new(),
        ));
    }

    /// Advances the queue when the running entry reports its outcome.
    pub fn advance_queue_if_ready(&mut self) {
        if self.stopping.is_some() {
            self.settle_stopped_run();
            return;
        }
        if !self.state.queue.is_running() {
            return;
        }
        // The picker waits on the tech, so the entry's time budget has not started.
        if self.show_data_transfer_ui {
            return;
        }
        let Some(current_name) = self
            .state
            .queue
            .current_script()
            .map(|q| q.script.name.clone())
        else {
            return;
        };
        self.advance_on_outcome(&current_name);
    }

    /// Consumes the running entry's outcome. A dead worker is an error rather than a wait.
    fn advance_on_outcome(&mut self, current_name: &str) {
        let Some(handle) = self.running.as_ref() else {
            return;
        };
        match handle.done.try_recv() {
            Ok(outcome) => {
                self.outcomes.insert(outcome.run_token, outcome.clone());
                let failed = outcome.result.is_failure();
                let message = outcome.result.message().to_string();
                if outcome.reboot_recommended {
                    self.reboot_prompt_open = true;
                }
                if let Some(code) = outcome.exit_code {
                    self.log_info(current_name, format!("exit code {code}"));
                }
                if failed {
                    self.log_error(current_name, message);
                } else {
                    self.log_info(
                        current_name,
                        format!("Finished in {:.1}s", outcome.duration.as_secs_f32()),
                    );
                }
                self.running = None;
                self.advance_to_next(failed);
            }
            Err(crossbeam::channel::TryRecvError::Empty) => {
                let started_at = self.run_started;
                let budget = CATALOG
                    .timeout_secs(current_name)
                    .map(std::time::Duration::from_secs)
                    .unwrap_or(QUEUE_SCRIPT_TIMEOUT);
                if started_at.is_some_and(|at| at.elapsed() >= budget) {
                    self.log_warning(
                        current_name,
                        format!("No completion after {}s; cancelling", budget.as_secs()),
                    );
                    if let Some(handle) = self.running.as_ref() {
                        handle.cancel.cancel();
                    }
                    self.running = None;
                    self.advance_to_next(true);
                }
            }
            Err(crossbeam::channel::TryRecvError::Disconnected) => {
                self.log_error(current_name, "Script worker stopped without reporting");
                self.running = None;
                self.advance_to_next(true);
            }
        }
    }

    /// Mark the current entry finished and start whatever is next.
    fn advance_to_next(&mut self, failed: bool) {
        self.state.queue.finish_current(failed);
        self.state.queue.next();
        self.run_started = None;
        self.transfer_run = None;

        if self.state.queue.is_running() {
            self.execute_next_script();
        } else {
            let (completed, total) = self.state.queue.progress();
            self.current_script_name = None;
            self.log_info("Queue", format!("Queue complete ({}/{} finished)", completed, total));
        }
    }

    /// Scans for user profiles and opens the picker. A failed or empty scan finishes the entry.
    fn execute_data_transfer(&mut self, log_tx: Sender<ScriptLogEntry>) {
        let _ = log_tx.try_send(ScriptLogEntry::info(
            ScriptCategory::Tuneup, "Data Transfer", "Scanning for user profiles..."
        ));

        let tx = self.data_transfer_tx.clone();
        let run = self.transfer_run.clone();
        std::thread::spawn(move || match get_data_transfer_candidates() {
            Ok(paths) if paths.is_empty() && run.is_some() => {
                let msg = "No user profiles found to transfer";
                let _ = log_tx.try_send(ScriptLogEntry::warning(
                    ScriptCategory::Tuneup, "Data Transfer", msg,
                ));
                if let Some(run) = run {
                    run.finish(ScriptResult::Skipped(msg.into()));
                }
            }
            Ok(paths) => {
                let _ = tx.try_send(paths);
            }
            Err(e) => {
                log::error!("Error getting data transfer candidates: {e:?}");
                let msg = format!("Could not list user profiles: {e}");
                let _ = log_tx.try_send(ScriptLogEntry::error(
                    ScriptCategory::Tuneup, "Data Transfer", msg.clone(),
                ));
                if let Some(run) = run {
                    run.finish(ScriptResult::Error(msg));
                }
            }
        });
    }

    /// Closes the picker without transferring anything.
    pub fn cancel_data_transfer(&mut self) {
        self.show_data_transfer_ui = false;
        self.selected_sources.clear();
        self.selected_destination = None;
        if let Some(run) = self.transfer_run.take() {
            self.log_warning("Data Transfer", "Data transfer cancelled");
            run.finish(ScriptResult::Skipped("Data transfer cancelled".into()));
        }
    }

    /// Copies each source with robocopy, concurrently, and reports once every copy has returned.
    pub fn start_data_transfer(&mut self, sources: Vec<String>, destination: String) {
        let robocopy_tx = self.robocopy_tx.clone();
        let log_tx = self.channels.log_tx.clone();
        let run = self.transfer_run.take();
        if run.is_some() {
            self.run_started = Some(std::time::Instant::now());
        }

        let total = sources.len();
        let copies: Vec<_> = sources
            .into_iter()
            .map(|source| {
                let source_path = PathBuf::from(&source);
                let dest_path = PathBuf::from(&destination);
                let tx = robocopy_tx.clone();
                let log = log_tx.clone();
                let _ = log.try_send(ScriptLogEntry::info(
                    ScriptCategory::Tuneup, "Data Transfer",
                    format!("Starting transfer: {} -> {}", source, destination)
                ));
                async move {
                    match run_robocopy(&source_path, &dest_path, tx).await {
                        Ok(()) => true,
                        Err(e) => {
                            let _ = log.try_send(ScriptLogEntry::error(
                                ScriptCategory::Tuneup, "Data Transfer",
                                format!("Robocopy failed: {}", e)
                            ));
                            false
                        }
                    }
                }
            })
            .collect();

        tokio::spawn(async move {
            let failed = futures::future::join_all(copies)
                .await
                .into_iter()
                .filter(|ok| !ok)
                .count();
            let (entry, result) = if failed == 0 {
                let msg = format!("Transferred {total} folder(s)");
                (
                    ScriptLogEntry::success(ScriptCategory::Tuneup, "Data Transfer", msg.clone()),
                    ScriptResult::Success(msg),
                )
            } else {
                let msg = format!("{failed} of {total} transfers failed");
                (
                    ScriptLogEntry::error(ScriptCategory::Tuneup, "Data Transfer", msg.clone()),
                    ScriptResult::Error(msg),
                )
            };
            let _ = log_tx.try_send(entry);
            if let Some(run) = run {
                run.finish(result);
            }
        });

        self.show_data_transfer_ui = false;
        self.selected_sources.clear();
        self.selected_destination = None;
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

    fn log_error(&mut self, script: &str, message: impl Into<String>) {
        self.state.log(ScriptLogEntry::error(
            ScriptCategory::Custom("System".to_string()),
            script,
            message,
        ));
    }
}

// ============================================================================
// Windows-specific helper functions
// ============================================================================

/// The catalog entry an MCP `scripts_run` request may start, or why it is refused.
fn mcp_script(name: &str) -> Result<&'static ScriptDef, String> {
    let def = CATALOG
        .id_for_legacy_name(name)
        .and_then(|id| CATALOG.get(id))
        .ok_or_else(|| format!("'{name}' is not in the script catalog"))?;
    if def.category() == ScriptCategory::StressTests {
        return Err("Unsupported category: StressTests".into());
    }
    Ok(def)
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

#[cfg(test)]
mod mcp_dispatch_tests {
    use super::*;

    #[test]
    fn stress_scripts_are_refused() {
        let gpu_probe = CATALOG.get(&ScriptId::new("gpu-probe")).expect("catalog entry");
        assert!(mcp_script(&gpu_probe.name).is_err());
        let cpu = CATALOG.get(&ScriptId::new("stress-cpu")).expect("catalog entry");
        assert!(mcp_script(&cpu.name).is_err());
    }

    #[test]
    fn an_unknown_name_is_refused() {
        let refusal = mcp_script("Definitely Not A Script").expect_err("refused");
        assert!(refusal.contains("not in the script catalog"));
    }

    #[test]
    fn a_run_gets_its_own_budget() {
        let def = mcp_script("Install Windows Updates").expect("allowed");
        assert_eq!(def.timeout_secs, 3600);
        assert!(mcp_script("Data Transfer").is_ok());
    }
}
