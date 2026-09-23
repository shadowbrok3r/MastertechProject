//! Runs catalog scripts through the shared executors: the tab's queue one at a time, and MCP requests.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crossbeam::channel::{Receiver, TryRecvError};
use displays::scripts::catalog::{CATALOG, Requirement, ScriptDef, Surface};
use displays::scripts::executor::{CancelToken, ScriptContext, ScriptHandle, ScriptOutcome, ScriptResult};
use displays::scripts::{
    script_run_request_receiver, script_run_result_sender, LogLevel, ScriptCategory, ScriptChannels,
    ScriptLogEntry, ScriptRunRequest, ScriptRunResult,
};

use super::checklist::TodoItem;
use super::render::Reporter;
use super::ScriptsTab;

const DATA_TRANSFER: &str = "data-transfer";
/// How long a script needing the customer email waits for a ticket lookup.
const TICKET_WAIT: Duration = Duration::from_secs(20);

/// A queued script in flight.
pub(super) struct QueuedRun {
    item: TodoItem,
    started: Instant,
    /// Catalog budget; a user script has none.
    budget: Option<Duration>,
    done: RunDone,
}

enum RunDone {
    Catalog(ScriptHandle),
    UserScript(Receiver<bool>),
}

enum RunEnd {
    Outcome(ScriptOutcome),
    UserScript(bool),
    OverBudget(Duration),
    Died,
}

#[derive(Default)]
pub(super) struct QueueTally {
    passed: usize,
    warned: usize,
    failed: usize,
    skipped: usize,
}

impl QueueTally {
    fn summary(&self) -> String {
        let mut line = format!(
            "All selected scripts finished: {} passed, {} with warnings, {} failed",
            self.passed, self.warned, self.failed
        );
        if self.skipped > 0 {
            line.push_str(&format!(", {} skipped", self.skipped));
        }
        line
    }
}

/// One in-flight run requested by the MCP `scripts_run` tool.
pub(super) struct TerminalMcpPendingRun {
    request_id: String,
    script_name: String,
    category: ScriptCategory,
    dispatched_at: Instant,
    timeout: Duration,
    log_lines: Vec<String>,
    channels: ScriptChannels,
    handle: ScriptHandle,
}

enum McpEnd {
    Outcome(ScriptOutcome),
    Failed(String),
}

/// The report label for a log line; `None` keeps the current one.
fn reporter_for(category: &ScriptCategory) -> Option<Reporter> {
    match category {
        ScriptCategory::Tuneup => Some(Reporter::Tuneup),
        ScriptCategory::Informational => Some(Reporter::Informational),
        ScriptCategory::JunkwareRemoval => Some(Reporter::JunkwareRemoval),
        ScriptCategory::StressTests => Some(Reporter::StressTest),
        ScriptCategory::UserScripts(_) => Some(Reporter::UserScript),
        ScriptCategory::Custom(_) => None,
    }
}

/// The catalog entry an MCP `scripts_run` request may start here, or why it is refused.
fn mcp_script(name: &str, service_number: &str) -> Result<&'static ScriptDef, String> {
    let def = CATALOG
        .id_for_legacy_name(name)
        .and_then(|id| CATALOG.get(id))
        .ok_or_else(|| format!("'{name}' is not in the script catalog"))?;
    if def.id.as_str() == DATA_TRANSFER {
        return Err("Data Transfer needs its destination picked on this machine".into());
    }
    if def.requires(Requirement::ServiceNumber) && service_number.trim().is_empty() {
        return Err(format!("'{}' requires a service number", def.name));
    }
    Ok(def)
}

impl ScriptsTab<'_> {
    /// Queues the selected scripts in Job Builder order and starts the first.
    pub fn run_selected_scripts(&mut self) {
        let mut selected = self.get_selected_scripts();
        if selected.is_empty() {
            self.log_message("No scripts selected to run.");
            return;
        }
        let position: HashMap<&str, usize> = Self::CHECKLIST_ORDERED
            .iter()
            .filter_map(|name| self.checklists.get(*name))
            .flat_map(|list| list.items.iter())
            .enumerate()
            .map(|(i, item)| (item.text.as_str(), i))
            .collect();
        selected.sort_by_key(|item| position.get(item.text.as_str()).copied().unwrap_or(usize::MAX));

        self.clear_selected_scripts();
        self.tally = QueueTally::default();
        self.log_message(format!("Running {} script(s), one at a time", selected.len()));
        self.queue.extend(selected);
        self.start_next();
    }

    pub fn queue_busy(&self) -> bool {
        self.running.is_some() || !self.queue.is_empty()
    }

    /// Stops the queue, or abandons a script that is still stopping.
    pub fn stop_queue(&mut self) {
        let dropped = self.queue.len();
        self.queue.clear();
        self.awaiting_ticket = None;
        let Some(run) = self.running.as_ref() else {
            if dropped > 0 {
                self.finish_queue();
            }
            return;
        };
        let name = run.item.text.clone();
        if self.stopping {
            self.running = None;
            self.stopping = false;
            self.tally.failed += 1;
            self.log_message(format!("Abandoned {name}; it may still be running"));
            self.finish_queue();
            return;
        }
        if let RunDone::Catalog(handle) = &run.done {
            handle.cancel.cancel();
        }
        self.stopping = true;
        self.log_message(format!("Stopping {name}; {dropped} queued script(s) dropped"));
    }

    /// Records the running script's end and starts the next one.
    pub(super) fn advance_queue(&mut self) {
        let Some(run) = self.running.as_ref() else {
            if !self.queue.is_empty() {
                self.start_next();
            }
            return;
        };
        let end = match &run.done {
            RunDone::Catalog(handle) => match handle.done.try_recv() {
                Ok(outcome) => RunEnd::Outcome(outcome),
                Err(TryRecvError::Disconnected) => RunEnd::Died,
                Err(TryRecvError::Empty) => match run.budget {
                    Some(budget) if !self.stopping && run.started.elapsed() >= budget => {
                        handle.cancel.cancel();
                        RunEnd::OverBudget(budget)
                    }
                    _ => return,
                },
            },
            RunDone::UserScript(done) => match done.try_recv() {
                Ok(ok) => RunEnd::UserScript(ok),
                Err(TryRecvError::Disconnected) => RunEnd::Died,
                Err(TryRecvError::Empty) => return,
            },
        };
        let Some(run) = self.running.take() else {
            return;
        };
        // The worker logs before it reports.
        self.drain_script_logs();
        self.finish_run(&run, end);
        if self.stopping {
            self.stopping = false;
            self.finish_queue();
        } else {
            self.start_next();
        }
    }

    fn start_next(&mut self) {
        while self.running.is_none() {
            let Some(needs_email) = self
                .queue
                .front()
                .map(|item| self.needs_customer_email(item))
            else {
                self.finish_queue();
                return;
            };
            if needs_email && self.ticket_pending() {
                return;
            }
            let Some(item) = self.queue.pop_front() else {
                return;
            };
            self.start(item);
        }
    }

    fn start(&mut self, item: TodoItem) {
        let category = item.category();
        self.current_script
            .replace(Some((category.clone(), item.text.clone())));
        if let Some(reporter) = reporter_for(&category) {
            self.current_reporter.replace(reporter);
        }
        self.log_message(format!("Starting {}", item.text));

        if let ScriptCategory::UserScripts(path) = &category {
            let done = self.start_user_script(path, &item.text);
            self.running = Some(QueuedRun {
                item,
                started: Instant::now(),
                budget: None,
                done: RunDone::UserScript(done),
            });
            return;
        }
        let Some(def) = item.def() else {
            self.log_message(format!("{} is not in the script catalog", item.text));
            self.tally.failed += 1;
            return;
        };
        if def.id.as_str() == DATA_TRANSFER {
            self.open_data_transfer(&item);
            return;
        }
        let ctx = self.script_context(self.channels.clone());
        let token = self.next_token();
        let handle = crate::scripts_exec::registry().spawn(def, &ctx, token, CancelToken::new());
        self.running = Some(QueuedRun {
            item,
            started: Instant::now(),
            budget: Some(Duration::from_secs(def.timeout_secs)),
            done: RunDone::Catalog(handle),
        });
    }

    /// Opens the destination picker; the copy itself runs outside the queue.
    fn open_data_transfer(&mut self, item: &TodoItem) {
        #[cfg(target_os = "windows")]
        {
            self.data_transfer(&item.text, &item.category());
            self.current_reporter.replace(Reporter::Robocopy);
            self.tally.passed += 1;
        }
        #[cfg(not(target_os = "windows"))]
        {
            self.log_message(format!("{} requires Windows", item.text));
            self.tally.failed += 1;
        }
    }

    fn finish_run(&mut self, run: &QueuedRun, end: RunEnd) {
        let name = run.item.text.as_str();
        let secs = run.started.elapsed().as_secs_f32();
        let ran = match end {
            RunEnd::Outcome(outcome) => {
                if outcome.reboot_recommended {
                    self.reboot_pending.replace(true);
                }
                if let Some(code) = outcome.exit_code {
                    self.log_message(format!("{name}: exit code {code}"));
                }
                let (ran, verdict) = match outcome.result {
                    ScriptResult::Success(_) => {
                        self.tally.passed += 1;
                        (true, "passed")
                    }
                    ScriptResult::Warning(_) => {
                        self.tally.warned += 1;
                        (true, "finished with a warning")
                    }
                    ScriptResult::Error(_) => {
                        self.tally.failed += 1;
                        (false, "failed")
                    }
                    ScriptResult::Skipped(_) => {
                        self.tally.skipped += 1;
                        (false, "was skipped")
                    }
                };
                self.log_message(format!("{name} {verdict} in {secs:.1}s"));
                ran
            }
            RunEnd::UserScript(ok) => {
                if ok {
                    self.tally.passed += 1;
                } else {
                    self.tally.failed += 1;
                }
                let verdict = if ok { "passed" } else { "failed" };
                self.log_message(format!("{name} {verdict} in {secs:.1}s"));
                ok
            }
            RunEnd::OverBudget(budget) => {
                self.tally.failed += 1;
                self.log_message(format!(
                    "{name}: no result after {}s; cancelled",
                    budget.as_secs()
                ));
                false
            }
            RunEnd::Died => {
                self.tally.failed += 1;
                self.log_message(format!("{name}: the script stopped without reporting"));
                false
            }
        };
        self.update_checklist(run.item.category(), name, ran);
    }

    fn finish_queue(&mut self) {
        self.current_script.replace(None);
        let tally = std::mem::take(&mut self.tally);
        self.log_message(tally.summary());
    }

    fn needs_customer_email(&self, item: &TodoItem) -> bool {
        item.def()
            .is_some_and(|def| def.requires(Requirement::CustomerEmail))
            && self.customer_email.trim().is_empty()
    }

    /// True while a ticket lookup may still bring the customer email.
    fn ticket_pending(&mut self) -> bool {
        match self.awaiting_ticket {
            Some(asked) if asked.elapsed() < TICKET_WAIT => true,
            Some(_) => {
                self.awaiting_ticket = None;
                self.log_message("The ticket lookup did not answer; continuing without the customer email");
                false
            }
            None => false,
        }
    }

    /// Takes the customer email for this service number, or looks its ticket up.
    pub(super) fn prepare_ticket(&mut self) {
        if self.service_number.is_empty() {
            return;
        }
        let Ok(mut ctx) = self.ctx.lock() else {
            return;
        };
        let same_ticket = ctx.service_data.ticket_data.service_number.trim() == self.service_number;
        let email = ctx.service_data.customer_data.email.trim().to_string();
        if same_ticket && !email.is_empty() {
            self.customer_email = email;
            return;
        }
        self.customer_email.clear();
        ctx.service_data.ticket_data.service_number = self.service_number.clone();
        ctx.service_data.get_ticket();
        self.awaiting_ticket = Some(Instant::now());
    }

    /// Fills the service number and email from the service form when its ticket changes.
    pub(super) fn seed_from_ticket(&mut self) {
        let Ok(ctx) = self.ctx.try_lock() else {
            return;
        };
        let ticket = ctx.service_data.ticket_data.service_number.trim().to_string();
        let email = ctx.service_data.customer_data.email.trim().to_string();
        drop(ctx);
        if !ticket.is_empty() && ticket != self.seeded_service_number {
            self.service_number_field.set_text(&ticket);
            self.service_number = ticket.clone();
            self.seeded_service_number = ticket;
        }
        if !email.is_empty() && self.service_number == self.seeded_service_number {
            self.customer_email = email;
        }
    }

    pub(super) fn script_context(&self, channels: ScriptChannels) -> ScriptContext {
        let present = |s: &str| (!s.trim().is_empty()).then(|| s.trim().to_owned());
        ScriptContext {
            service_number: present(&self.service_number),
            customer_email: present(&self.customer_email),
            diagnostic_session_id: self.mcp_diagnostic_session_id.clone(),
            surface: Some(Surface::Terminal),
            channels,
        }
    }

    /// Moves queued runs' log lines and progress into the report.
    pub(super) fn drain_script_logs(&self) {
        while let Ok(entry) = self.channels.log_rx.try_recv() {
            self.log_entry(&entry);
        }
        while let Ok((_, current, total)) = self.channels.progress_rx.try_recv() {
            self.show_progress(current, total);
        }
    }

    fn log_entry(&self, entry: &ScriptLogEntry) {
        if let Some(reporter) = reporter_for(&entry.category) {
            self.current_reporter.replace(reporter);
        }
        match entry.level {
            LogLevel::Warning => self.log_message(format!("WARNING: {}", entry.message)),
            LogLevel::Error => self.log_message(format!("ERROR: {}", entry.message)),
            LogLevel::Info | LogLevel::Success => self.log_message(&entry.message),
        }
    }

    fn show_progress(&self, current: u64, total: u64) {
        if total > 0 {
            self.progress.replace(Some((current.min(total), total)));
        }
    }

    fn next_token(&mut self) -> u64 {
        self.next_run_token += 1;
        self.next_run_token
    }

    /// Labels the Run button for the queue's state.
    pub(super) fn sync_run_button(&mut self) {
        let label = if self.stopping {
            "Abandon"
        } else if self.queue_busy() {
            "Stop"
        } else {
            "Run Selected"
        };
        if self.run_btn.get_label() != label {
            self.run_btn.set_label(label.to_string());
        }
    }

    /// True when a selected script needs a service number and none is entered.
    pub fn run_button_should_be_disabled(&self) -> bool {
        if self.queue_busy() {
            return false;
        }
        let needs = self
            .get_selected_scripts()
            .iter()
            .any(|item| self.needs_service_number(item));
        needs && !self.has_service_number()
    }

    /// A service number is needed directly, or to look up a missing customer email.
    fn needs_service_number(&self, item: &TodoItem) -> bool {
        item.def().is_some_and(|def| {
            def.requires(Requirement::ServiceNumber)
                || (def.requires(Requirement::CustomerEmail) && self.customer_email.trim().is_empty())
        })
    }

    fn has_service_number(&self) -> bool {
        !self.service_number.trim().is_empty()
            || self
                .service_number_field
                .get_text()
                .first()
                .is_some_and(|s| !s.trim().is_empty())
    }

    /// Runs a user script from the SurrealDB bucket through PowerShell.
    fn start_user_script(&self, full_path: &str, name: &str) -> Receiver<bool> {
        let (tx, done) = crossbeam::channel::bounded(1);
        let bucket = self.filesystem.user.get_user_bucket_name();
        let path = full_path.to_string();
        let log_tx = self.script_log_tx.clone();
        self.log_message(format!("Running custom script '{name}'"));

        tokio::spawn(async move {
            use database::schema::file_storage;

            let script_content = match file_storage::get_file_as_string(&bucket, &path).await {
                Ok(Some(content)) => content,
                Ok(None) => {
                    let _ = log_tx.try_send(format!("Script not found: {path}"));
                    let _ = tx.send(false);
                    return;
                }
                Err(e) => {
                    let _ = log_tx.try_send(format!("Failed to load script: {e}"));
                    let _ = tx.send(false);
                    return;
                }
            };

            #[cfg(target_os = "windows")]
            {
                let result = tokio::task::spawn_blocking(move || {
                    powershell_script::PsScriptBuilder::new()
                        .no_profile(true)
                        .non_interactive(true)
                        .hidden(true)
                        .print_commands(false)
                        .build()
                        .run(&script_content)
                })
                .await;

                let success = match result {
                    Ok(Ok(output)) => {
                        if let Some(stdout) = output.stdout() {
                            let trimmed = stdout.trim();
                            if !trimmed.is_empty() {
                                let _ = log_tx.try_send(trimmed.to_string());
                            }
                        }
                        if let Some(stderr) = output.stderr() {
                            let trimmed = stderr.trim();
                            if !trimmed.is_empty() {
                                let _ = log_tx.try_send(format!("stderr: {trimmed}"));
                            }
                        }
                        output.success()
                    }
                    Ok(Err(e)) => {
                        let _ = log_tx.try_send(format!("Script error: {e}"));
                        false
                    }
                    Err(e) => {
                        let _ = log_tx.try_send(format!("Script task failed: {e}"));
                        false
                    }
                };
                let _ = tx.send(success);
            }

            #[cfg(not(target_os = "windows"))]
            {
                let _ = script_content;
                let _ = log_tx.try_send("User scripts require Windows".into());
                let _ = tx.send(false);
            }
        });
        done
    }

    /// Starts each MCP `scripts_run` request waiting on the global channel.
    pub fn process_mcp_requests(&mut self) {
        while let Ok(req) = script_run_request_receiver().try_recv() {
            self.dispatch_mcp_request(req);
        }
    }

    fn dispatch_mcp_request(&mut self, req: ScriptRunRequest) {
        if let Some(sn) = req.service_number.as_deref().filter(|s| !s.trim().is_empty()) {
            self.service_number = sn.trim().to_string();
        }
        if let Some(email) = req.customer_email.as_deref().filter(|s| !s.trim().is_empty()) {
            self.customer_email = email.trim().to_string();
        }
        self.mcp_diagnostic_session_id = req
            .diagnostic_session_id
            .clone()
            .filter(|s| !s.trim().is_empty());

        let def = match mcp_script(&req.script_name, &self.service_number) {
            Ok(def) => def,
            Err(message) => {
                let _ = script_run_result_sender().send(ScriptRunResult {
                    request_id: req.request_id,
                    success: false,
                    message,
                    logs: Vec::new(),
                });
                return;
            }
        };

        let requested = format!("MCP requested: {} (request_id {})", def.name, req.request_id);
        self.log_message(&requested);
        let channels = ScriptChannels::default();
        let ctx = self.script_context(channels.clone());
        let token = self.next_token();
        let handle = crate::scripts_exec::registry().spawn(def, &ctx, token, CancelToken::new());
        self.pending_mcp_runs.push(TerminalMcpPendingRun {
            request_id: req.request_id,
            script_name: def.name.clone(),
            category: def.category(),
            dispatched_at: Instant::now(),
            timeout: Duration::from_secs(def.timeout_secs),
            log_lines: vec![requested],
            channels,
            handle,
        });
    }

    /// Reports each MCP run whose outcome has arrived, or whose budget ran out.
    pub fn process_mcp_completions(&mut self) {
        let mut idx = 0;
        while idx < self.pending_mcp_runs.len() {
            let entries: Vec<ScriptLogEntry> =
                self.pending_mcp_runs[idx].channels.log_rx.try_iter().collect();
            for entry in &entries {
                self.log_entry(entry);
            }
            let progress: Vec<(String, u64, u64)> =
                self.pending_mcp_runs[idx].channels.progress_rx.try_iter().collect();
            for (_, current, total) in progress {
                self.show_progress(current, total);
            }

            let pending = &mut self.pending_mcp_runs[idx];
            pending.log_lines.extend(entries.into_iter().map(|e| e.message));
            let end = match pending.handle.done.try_recv() {
                Ok(outcome) => Some(McpEnd::Outcome(outcome)),
                Err(TryRecvError::Disconnected) => Some(McpEnd::Failed(format!(
                    "Script '{}' stopped without reporting",
                    pending.script_name
                ))),
                Err(TryRecvError::Empty) if pending.dispatched_at.elapsed() > pending.timeout => {
                    pending.handle.cancel.cancel();
                    Some(McpEnd::Failed(format!(
                        "Script '{}' did not finish within {}s. It may still be running on the host.",
                        pending.script_name,
                        pending.timeout.as_secs()
                    )))
                }
                Err(TryRecvError::Empty) => None,
            };
            match end {
                Some(end) => {
                    let pending = self.pending_mcp_runs.remove(idx);
                    self.report_mcp_run(pending, end);
                }
                None => idx += 1,
            }
        }
    }

    fn report_mcp_run(&mut self, mut pending: TerminalMcpPendingRun, end: McpEnd) {
        for entry in pending.channels.log_rx.try_iter() {
            self.log_entry(&entry);
            pending.log_lines.push(entry.message);
        }
        let (success, message) = match end {
            McpEnd::Outcome(outcome) => {
                if outcome.reboot_recommended {
                    self.reboot_pending.replace(true);
                }
                let ran = matches!(
                    outcome.result,
                    ScriptResult::Success(_) | ScriptResult::Warning(_)
                );
                self.update_checklist(pending.category.clone(), &pending.script_name, ran);
                (outcome.result.is_success(), outcome.result.message().to_string())
            }
            McpEnd::Failed(message) => (false, message),
        };
        let _ = script_run_result_sender().send(ScriptRunResult {
            request_id: pending.request_id,
            success,
            message,
            logs: pending.log_lines,
        });
    }
}

#[cfg(test)]
mod terminal_dispatch_tests {
    use super::*;

    /// A script the terminal lists but nothing can run would queue and fail every time.
    #[test]
    fn every_listed_script_can_run() {
        let registry = crate::scripts_exec::registry();
        for def in CATALOG.for_surface(Surface::Terminal) {
            assert!(
                def.id.as_str() == DATA_TRANSFER || registry.find(&def.id).is_some(),
                "{} is listed in terminal mode but has no executor",
                def.id
            );
        }
    }

    #[test]
    fn an_unknown_name_is_refused() {
        let refusal = mcp_script("Definitely Not A Script", "123").expect_err("refused");
        assert!(refusal.contains("not in the script catalog"));
    }

    #[test]
    fn data_transfer_is_refused() {
        assert!(mcp_script("Data Transfer", "123").is_err());
    }

    #[test]
    fn a_declared_service_number_is_enforced() {
        assert!(mcp_script("Cert: Bronze", "").is_err());
        assert!(mcp_script("Activate CPS", " ").is_err());
        assert!(mcp_script("Cert: Bronze", "12345").is_ok());
        assert!(mcp_script("Benchmark Suite", "").is_ok());
        assert!(mcp_script("Windows Version", "").is_ok());
    }

    #[test]
    fn the_old_test_entries_are_gone() {
        let names: Vec<&str> = CATALOG
            .for_surface(Surface::Terminal)
            .map(|d| d.name.as_str())
            .collect();
        for gone in ["Webroot TEST", "SuperAnti TEST", "Disable proxy settings"] {
            assert!(!names.contains(&gone), "{gone} is still listed");
        }
    }

    #[test]
    fn every_catalog_category_has_a_list() {
        for def in CATALOG.for_surface(Surface::Terminal) {
            assert!(
                super::super::checklist::list_name(&def.category()).is_some(),
                "{} has no Job Builder list",
                def.id
            );
        }
    }
}
