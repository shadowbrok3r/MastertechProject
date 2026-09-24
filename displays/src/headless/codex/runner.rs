//! One Codex thread: connects to zc-codexd, streams codex events into
//! agent_event rows, answers the requests codex addresses to its client (tool
//! calls, technician questions, approvals) and applies technician turns.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use database::schema::agent_thread::AgentThreadState;
use database::schema::{
    AgentApproval, AgentEvent, AgentThread, AgentTurn, AssistRequest, NewAgentApproval, RecordId,
    RecordIdExt,
};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use zc_codex_client::{decision, elicitation, Client, Event};

use super::coalesce::{self, Partial, ThreadRow, TranscriptBuffer};
use super::tools::{scope_violation, ToolHost, ToolOutcome, ToolPolicy};
use super::zeroclaw::{self, ZeroclawMemory};
use super::{manager, prompt, register_runner, runner_for, unregister_runner, Config};

/// What the outside world may ask a running thread to do.
#[derive(Debug, Clone)]
pub enum RunnerCmd {
    Turn(AgentTurn),
}

/// Reconnect attempts before a thread is marked failed.
const RECONNECT_ATTEMPTS: u32 = 20;
/// How often a pending approval row is re-read.
const APPROVAL_POLL: Duration = Duration::from_millis(750);
/// Characters of tool output kept in a transcript row (the reply to codex is capped separately).
const ROW_TEXT_CHARS: usize = 4_000;
/// Page size and page cap when a resumed thread's items are backfilled.
const BACKFILL_PAGE: u32 = 100;
const BACKFILL_MAX_PAGES: usize = 20;
/// How often buffered transcript text and thread fields are checked for a due write.
const FLUSH_TICK: Duration = Duration::from_millis(250);
/// Longest `data:` URL kept verbatim in a stored row.
const DATA_URL_KEEP: usize = 256;

/// Starts the thread's runner task and registers its command channel; a thread
/// that already has a runner gets that runner's channel back instead.
pub fn spawn(cfg: Arc<Config>, thread: AgentThread, opening: Option<String>) -> mpsc::Sender<RunnerCmd> {
    let key = thread.id.key_string();
    if let Some(existing) = runner_for(&key) {
        return existing;
    }
    let (tx, rx) = mpsc::channel(32);
    if !register_runner(&key, tx.clone()) {
        return runner_for(&key).unwrap_or(tx);
    }
    tokio::spawn(async move {
        if let Err(e) = Runner::run(cfg, thread, opening, rx).await {
            log::warn!("codex: thread {key} ended with error: {e}");
        }
        unregister_runner(&key);
    });
    tx
}

enum Flow {
    Continue,
    Reconnect,
    Closed,
}

struct Runner {
    cfg: Arc<Config>,
    thread: AgentThread,
    tools: Arc<ToolHost>,
    client: Client,
    codex_thread_id: Option<String>,
    next_seq: i64,
    seqs: HashMap<String, i64>,
    /// Items whose authoritative row is written; later deltas for them are dropped.
    completed: HashSet<String>,
    transcript: TranscriptBuffer,
    row: ThreadRow,
    turn_no: u32,
    remembered: HashSet<String>,
    /// Set once the first attach succeeds; a later attach is an in-process reconnect.
    attached: bool,
    memory: Option<Arc<ZeroclawMemory>>,
}

impl Runner {
    async fn run(
        cfg: Arc<Config>,
        thread: AgentThread,
        opening: Option<String>,
        mut rx: mpsc::Receiver<RunnerCmd>,
    ) -> anyhow::Result<()> {
        let Some(manager) = manager() else {
            AgentThread::set_status(&thread.id, "failed", Some("plugin manager not initialised")).await?;
            anyhow::bail!("plugin manager not initialised");
        };
        let tools = match ToolHost::start(
            manager,
            ToolPolicy::from_env(),
            cfg.tool_output_chars,
            Duration::from_secs(cfg.tool_timeout_secs),
        )
        .await
        {
            Ok(t) => Arc::new(t),
            Err(e) => {
                AgentThread::set_status(&thread.id, "failed", Some(&format!("tool host: {e}"))).await?;
                return Err(e);
            }
        };
        let (client, mut events) = match Self::connect(&cfg).await {
            Ok(c) => c,
            Err(e) => {
                AgentThread::set_status(&thread.id, "failed", Some(&format!("codex daemon: {e}"))).await?;
                return Err(e);
            }
        };
        let recorded_seq = database::agent_chat::last_seq(&thread.id).await.unwrap_or(0);
        let stored_seq = thread.last_seq.unwrap_or(0);
        let mut row = ThreadRow::new(stored_seq, (thread.tokens_used, thread.tokens_window), coalesce::THREAD_FLUSH);
        row.touch(recorded_seq);
        let mut me = Self {
            next_seq: stored_seq.max(recorded_seq),
            codex_thread_id: thread.codex_thread_id.clone(),
            memory: cfg.zeroclaw.clone(),
            cfg,
            thread,
            tools,
            client,
            seqs: HashMap::new(),
            completed: HashSet::new(),
            transcript: TranscriptBuffer::new(coalesce::ITEM_FLUSH),
            row,
            turn_no: 0,
            remembered: HashSet::new(),
            attached: false,
        };
        if let Err(e) = me.attach().await {
            me.write_status("failed", Some(&format!("thread start: {e}"))).await?;
            return Err(e);
        }
        if let Some(text) = opening {
            let text = me.with_memory_brief(text).await;
            me.send_turn("start", &text).await?;
        }

        let mut tick = tokio::time::interval(FLUSH_TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            let flow = tokio::select! {
                ev = events.recv() => match ev {
                    Some(ev) => me.on_event(ev).await,
                    None => Flow::Reconnect,
                },
                cmd = rx.recv() => match cmd {
                    Some(RunnerCmd::Turn(turn)) => me.on_turn(turn).await,
                    None => Flow::Closed,
                },
                _ = tick.tick() => {
                    me.flush_due().await;
                    Flow::Continue
                }
            };
            match flow {
                Flow::Continue => {}
                Flow::Closed => break,
                Flow::Reconnect => match me.reconnect().await {
                    Ok(new_events) => events = new_events,
                    Err(e) => {
                        me.write_status("failed", Some(&e.to_string())).await?;
                        return Err(e);
                    }
                },
            }
        }
        me.flush_all().await;
        Ok(())
    }

    async fn connect(cfg: &Config) -> anyhow::Result<(Client, mpsc::Receiver<Event>)> {
        let token = if cfg.token.is_empty() { None } else { Some(cfg.token.as_str()) };
        Client::connect_with_token(&cfg.url, "mastertech-broker", token).await
    }

    fn general(&self) -> bool {
        super::is_general(&self.thread.connection_string)
    }

    /// Mastertech tools plus the ZeroClaw memory tools when the gateway is configured.
    fn dynamic_tools(&self, general: bool) -> Vec<Value> {
        let mut specs = self.tools.dynamic_specs(general);
        if self.memory.is_some() {
            specs.extend(zeroclaw::tool_specs());
        }
        specs
    }

    /// Parameters shared by `thread/start` and `thread/resume`.
    fn thread_params(&self) -> Value {
        let general = self.general();
        let mut offered = self.tools.offered(general);
        if self.memory.is_some() {
            offered.extend(zeroclaw::TOOL_NAMES.iter().map(|s| s.to_string()));
        }
        json!({
            "cwd": self.cfg.cwd,
            "model": self.cfg.model,
            "modelProvider": self.cfg.provider,
            "approvalPolicy": "on-request",
            "sandbox": "read-only",
            "developerInstructions": prompt::developer_instructions(
                &self.cfg, &self.thread, &offered, &self.tools.policy.prompt, self.memory.is_some()),
            "dynamicTools": self.dynamic_tools(general),
            "config": {
                "features.shell_tool": false,
                "features.multi_agent": false,
                "features.goals": false,
                "features.view_image": false,
                "web_search": "disabled",
                "mcp_servers.mastertech.enabled": false,
            },
        })
    }

    /// Resumes the recorded codex thread, or starts a new one.
    async fn attach(&mut self) -> anyhow::Result<()> {
        let first = !self.attached;
        if let Some(existing) = self.codex_thread_id.clone() {
            let mut params = self.thread_params();
            params["threadId"] = json!(existing);
            match self.client.request("thread/resume", params).await {
                Ok(_) => {
                    log::info!("codex: resumed codex thread {existing} for {}", self.thread.connection_string);
                    self.backfill(&existing).await;
                    if first && matches!(self.thread.status.as_str(), "running" | "waiting_approval") {
                        self.reconcile_interrupted().await;
                    }
                    self.attached = true;
                    self.write_status("idle", None).await?;
                    return Ok(());
                }
                Err(e) => {
                    log::warn!("codex: resume of {existing} failed ({e}); starting a fresh thread");
                    self.marker("error", &format!("Could not resume the previous agent thread ({e}); starting a new one."), None).await;
                }
            }
        }
        let id = self.client.thread_start(self.thread_params()).await?;
        AgentThread::set_codex_thread(&self.thread.id, &id).await?;
        self.write_status("idle", None).await?;
        log::info!("codex: thread {} -> codex {id}", self.thread.id.key_string());
        self.codex_thread_id = Some(id);
        self.attached = true;
        Ok(())
    }

    /// Records the items codex produced while no broker was attached.
    async fn backfill(&mut self, codex_thread_id: &str) {
        let known: HashSet<String> = match AgentEvent::item_ids(&self.thread.id).await {
            Ok(ids) => ids.into_iter().collect(),
            Err(e) => {
                log::warn!("codex: backfill skipped, transcript unreadable: {e}");
                return;
            }
        };
        let mut cursor: Option<String> = None;
        let mut added = 0usize;
        for _ in 0..BACKFILL_MAX_PAGES {
            let mut params = json!({ "threadId": codex_thread_id, "limit": BACKFILL_PAGE, "sortDirection": "asc" });
            if let Some(c) = &cursor {
                params["cursor"] = json!(c);
            }
            let mut page = match self.client.request("thread/items/list", params).await {
                Ok(p) => p,
                Err(e) => {
                    log::warn!("codex: thread/items/list failed: {e}");
                    break;
                }
            };
            let entries = page.get_mut("data").and_then(Value::as_array_mut).map(std::mem::take).unwrap_or_default();
            let exhausted = entries.is_empty();
            for mut entry in entries {
                let turn = entry.get("turnId").and_then(Value::as_str).map(str::to_string);
                let Some(mut item) = entry.get_mut("item").map(Value::take) else { continue };
                let Some(id) = item.get("id").and_then(Value::as_str).map(str::to_string) else { continue };
                if known.contains(&id) || self.seqs.contains_key(&id) {
                    continue;
                }
                let kind = kind_for(item.get("type").and_then(Value::as_str).unwrap_or(""));
                let seq = self.seq_for(&id);
                redact_images(&mut item);
                let text = item_text(kind, &item);
                match AgentEvent::complete(&self.thread.id, &id, seq, turn.as_deref(), kind, &text, item).await {
                    Ok(()) => {
                        added += 1;
                        self.completed.insert(id);
                    }
                    Err(e) => log::warn!("codex: backfill write failed: {e}"),
                }
            }
            cursor = page.get("nextCursor").and_then(Value::as_str).map(str::to_string);
            if cursor.is_none() || exhausted {
                break;
            }
        }
        if added > 0 {
            log::info!("codex: backfilled {added} items for {}", self.thread.id.key_string());
            self.marker("other", &format!("Recovered {added} transcript items produced while the broker was away."), None).await;
        }
    }

    /// The previous broker died mid-turn: the decisions it was relaying cannot be answered any more.
    async fn reconcile_interrupted(&mut self) {
        match AgentApproval::fail_pending_for_thread(
            &self.thread.id,
            "The broker restarted before this decision could be relayed; the agent can ask again.",
        )
        .await
        {
            Ok(0) => {}
            Ok(n) => self.marker("approval", &format!("{n} pending approval(s) were cancelled by a broker restart."), None).await,
            Err(e) => log::warn!("codex: could not reconcile pending approvals: {e}"),
        }
        self.marker("other", "Broker restarted while the agent was working; send a message if it went quiet.", None).await;
    }

    async fn reconnect(&mut self) -> anyhow::Result<mpsc::Receiver<Event>> {
        self.flush_all().await;
        self.marker("error", "Connection to the agent host dropped; reconnecting.", None).await;
        let mut delay = Duration::from_secs(2);
        for attempt in 1..=RECONNECT_ATTEMPTS {
            tokio::time::sleep(delay).await;
            match Self::connect(&self.cfg).await {
                Ok((client, events)) => {
                    self.client = client;
                    match self.attach().await {
                        Ok(()) => {
                            self.marker("other", "Reconnected to the agent host.", None).await;
                            return Ok(events);
                        }
                        Err(e) => log::warn!("codex: reattach attempt {attempt} failed: {e}"),
                    }
                }
                Err(e) => log::warn!("codex: reconnect attempt {attempt} failed: {e}"),
            }
            delay = (delay * 2).min(Duration::from_secs(30));
        }
        anyhow::bail!("agent host unreachable after {RECONNECT_ATTEMPTS} attempts")
    }

    fn turn_label(&self) -> Option<String> {
        (self.turn_no > 0).then(|| format!("t{}", self.turn_no))
    }

    /// The transcript position of an item, assigned on first sight.
    fn seq_for(&mut self, item_id: &str) -> i64 {
        if let Some(seq) = self.seqs.get(item_id) {
            return *seq;
        }
        self.next_seq += 1;
        let seq = self.next_seq;
        self.seqs.insert(item_id.to_string(), seq);
        self.row.touch(seq);
        seq
    }

    /// A standalone transcript row with no codex item behind it, written after any buffered text.
    async fn marker(&mut self, kind: &str, text: &str, item: Option<Value>) {
        self.flush_transcript().await;
        self.next_seq += 1;
        let seq = self.next_seq;
        let turn = self.turn_label();
        if let Err(e) = AgentEvent::marker(&self.thread.id, seq, turn.as_deref(), kind, text, item).await {
            log::warn!("codex: marker write failed: {e}");
        }
        self.row.touch(seq);
    }

    async fn write_partial(&self, partial: Partial) {
        let Partial { item_id, seq, kind, turn, text } = partial;
        if let Err(e) =
            AgentEvent::upsert_text(&self.thread.id, &item_id, seq, turn.as_deref(), kind, &text, false).await
        {
            log::warn!("codex: transcript write failed: {e}");
        }
    }

    async fn save_row(&self, state: AgentThreadState) {
        if let Err(e) = AgentThread::save_state(&self.thread.id, &state).await {
            log::warn!("codex: thread write failed: {e}");
        }
    }

    /// Writes every buffered transcript change.
    async fn flush_transcript(&mut self) {
        for partial in self.transcript.drain(Instant::now()) {
            self.write_partial(partial).await;
        }
    }

    /// Writes buffered text and thread fields whose interval has passed.
    async fn flush_due(&mut self) {
        let now = Instant::now();
        for partial in self.transcript.due(now) {
            self.write_partial(partial).await;
        }
        if let Some(state) = self.row.due(now) {
            self.save_row(state).await;
        }
    }

    /// Writes everything buffered.
    async fn flush_all(&mut self) {
        self.flush_transcript().await;
        if let Some(state) = self.row.drain(Instant::now()) {
            self.save_row(state).await;
        }
    }

    /// Writes buffered transcript text, then the status when it changed or carries an error.
    async fn write_status(&mut self, status: &str, error: Option<&str>) -> anyhow::Result<()> {
        self.flush_transcript().await;
        let Some(state) = self.row.status(status, error, Instant::now()) else { return Ok(()) };
        let written = AgentThread::save_state(&self.thread.id, &state).await;
        if written.is_err() {
            self.row.forget_status();
        }
        written
    }

    async fn set_status(&mut self, status: &str) {
        if let Err(e) = self.write_status(status, None).await {
            log::warn!("codex: status write failed: {e}");
        }
    }

    async fn stream_text(&mut self, item_id: &str, kind: &'static str, delta: &str, final_chunk: bool) {
        if self.completed.contains(item_id) {
            return;
        }
        let now = Instant::now();
        if !self.transcript.contains(item_id) {
            let seq = self.seq_for(item_id);
            let turn = self.turn_label();
            self.transcript.open(item_id, seq, kind, turn, String::new(), now);
        }
        if final_chunk {
            self.transcript.append(item_id, delta);
        } else if let Some(partial) = self.transcript.push(item_id, delta, now) {
            self.write_partial(partial).await;
        }
    }

    async fn on_event(&mut self, ev: Event) -> Flow {
        match ev {
            Event::Text { item_id, text, final_chunk, .. } => {
                self.stream_text(&item_id, "agent", &text, final_chunk).await
            }
            Event::Reasoning { item_id, text, final_chunk, .. } => {
                self.stream_text(&item_id, "reasoning", &text, final_chunk).await
            }
            Event::CommandOutput { item_id, text, final_chunk, .. } => {
                self.stream_text(&item_id, "command", &text, final_chunk).await
            }
            Event::Item { item_type, completed, item, .. } => self.on_item(&item_type, completed, item).await,
            Event::Ask { request_id, method, params, .. } => {
                self.on_ask(request_id, &method, params).await;
            }
            Event::AskResolved { .. } => {}
            Event::TurnStarted { .. } => {
                self.turn_no += 1;
                self.marker("turn_started", "", None).await;
                self.set_status("running").await;
            }
            Event::TurnCompleted { .. } => {
                self.flush_transcript().await;
                self.transcript.clear();
                self.marker("turn_completed", "", None).await;
                self.set_status("idle").await;
            }
            Event::Error { message, will_retry, .. } => {
                let text = if will_retry {
                    format!("Agent hit a transient error and is retrying: {message}")
                } else {
                    format!("Agent error: {message}")
                };
                self.marker("error", &text, None).await;
                if !will_retry {
                    if let Err(e) = self.write_status("idle", Some(&message)).await {
                        log::warn!("codex: status write failed: {e}");
                    }
                }
            }
            Event::TokenUsage { used, window, .. } => {
                self.row.set_tokens(
                    used.and_then(|u| i64::try_from(u).ok()),
                    window.and_then(|w| i64::try_from(w).ok()),
                );
            }
            Event::Other { method, .. } => {
                if method == "connection/closed" {
                    return Flow::Reconnect;
                }
                log::debug!("codex: {method} ignored");
            }
        }
        Flow::Continue
    }

    async fn on_item(&mut self, item_type: &str, completed: bool, mut item: Value) {
        let Some(item_id) = item.get("id").and_then(Value::as_str).map(str::to_string) else { return };
        let kind = kind_for(item_type);
        let now = Instant::now();
        if !completed {
            if !self.transcript.contains(&item_id) && !self.completed.contains(&item_id) {
                let seq = self.seq_for(&item_id);
                let turn = self.turn_label();
                self.transcript.open(&item_id, seq, kind, turn, item_text(kind, &item), now);
            }
            return;
        }
        let seq = self.seq_for(&item_id);
        self.transcript.close(&item_id);
        self.completed.insert(item_id.clone());
        for partial in self.transcript.unwritten_before(seq, now) {
            self.write_partial(partial).await;
        }
        redact_images(&mut item);
        let text = item_text(kind, &item);
        let turn = self.turn_label();
        if let Err(e) = AgentEvent::complete(&self.thread.id, &item_id, seq, turn.as_deref(), kind, &text, item).await {
            log::warn!("codex: item write failed: {e}");
        }
    }

    async fn respond(&self, request_id: &Value, body: Value) {
        if let Err(e) = self.client.respond(request_id, body).await {
            log::warn!("codex: reply to request {request_id} failed: {e}");
        }
    }

    async fn on_ask(&mut self, request_id: Value, method: &str, params: Value) {
        match method {
            "item/tool/call" => self.on_tool_call(request_id, params).await,
            "item/tool/requestUserInput" => self.on_question(request_id, params).await,
            "item/commandExecution/requestApproval" => {
                let cmd = params.get("command").map(|c| c.to_string()).unwrap_or_default();
                self.marker("approval", &format!("Shell command refused by policy: {cmd}"), Some(params)).await;
                self.respond(&request_id, decision::decline()).await;
            }
            "item/fileChange/requestApproval" => {
                self.marker("approval", "File change refused by policy.", Some(params)).await;
                self.respond(&request_id, decision::decline()).await;
            }
            "item/permissions/requestApproval" => {
                self.marker("approval", "Permission escalation refused by policy.", Some(params)).await;
                self.respond(&request_id, json!({ "permissions": {} })).await;
            }
            "mcpServer/elicitation/request" => {
                self.respond(&request_id, elicitation::decline()).await;
            }
            other => {
                log::warn!("codex: unsupported server request {other}");
                let _ = self.client.respond_error(&request_id, -32601, "unsupported by the Mastertech broker").await;
            }
        }
    }

    /// Runs a tool, writing buffered transcript text once it has run for one flush interval.
    async fn call_tool(&mut self, tool: &str, arguments: Value, general: bool) -> ToolOutcome {
        let tools = self.tools.clone();
        let call = tools.call(tool, arguments, general);
        tokio::pin!(call);
        tokio::select! {
            outcome = &mut call => return outcome,
            _ = tokio::time::sleep(coalesce::ITEM_FLUSH) => {}
        }
        self.flush_transcript().await;
        call.await
    }

    async fn on_tool_call(&mut self, request_id: Value, params: Value) {
        let tool = params.get("tool").and_then(Value::as_str).unwrap_or("").to_string();
        let arguments = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
        if zeroclaw::is_memory_tool(&tool) {
            let outcome = self.memory_tool(&tool, &arguments).await;
            self.respond(&request_id, outcome.response()).await;
            return;
        }
        let general = self.general();
        if let Some(refusal) = scope_violation(&arguments, &self.thread.connection_string) {
            self.marker("approval", &refusal, None).await;
            self.respond(&request_id, ToolOutcome::failure(refusal).response()).await;
            return;
        }
        let needs_human = self.tools.policy.needs_approval(&tool) && !self.remembered.contains(&tool);
        if !needs_human {
            let outcome = self.call_tool(&tool, arguments, general).await;
            self.after_tool(&tool, &outcome).await;
            self.respond(&request_id, outcome.response()).await;
            return;
        }

        let summary = format!(
            "run {tool} on {}",
            self.thread.hostname.clone().unwrap_or_else(|| self.thread.connection_string.clone())
        );
        let new = NewAgentApproval {
            thread: Some(self.thread.id.clone()),
            kind: "tool_call".into(),
            method: "item/tool/call".into(),
            codex_request_id: request_id.to_string(),
            summary: summary.clone(),
            server: Some("mastertech".into()),
            tool: Some(tool.clone()),
            arguments: Some(arguments.clone()),
            params: Some(params.clone()),
            questions: None,
            assignee: self.thread.assignee.clone(),
            connection_string: Some(self.thread.connection_string.clone()),
            store: self.thread.store.clone(),
            ttl_secs: self.cfg.approval_ttl_secs,
        };
        let approval_id = match AgentApproval::create(&new).await {
            Ok(id) => id,
            Err(e) => {
                log::warn!("codex: could not record approval: {e}");
                let outcome = ToolOutcome::failure("approval could not be requested; the call was not run".into());
                self.respond(&request_id, outcome.response()).await;
                return;
            }
        };
        self.set_status("waiting_approval").await;
        self.marker(
            "approval",
            &format!("Waiting for a technician to approve: {summary}"),
            Some(json!({ "approval": approval_id.key_string(), "tool": tool, "arguments": arguments })),
        )
        .await;

        let row = self.wait_for_decision(&approval_id).await;
        self.set_status("running").await;
        let status = row.as_ref().map(|r| r.status.clone()).unwrap_or_else(|| "expired".into());
        let note = row.as_ref().and_then(|r| r.deny_note.clone()).unwrap_or_default();
        match status.as_str() {
            "accepted" | "accepted_for_session" => {
                if status == "accepted_for_session" && self.tools.policy.may_remember(&tool) {
                    self.remembered.insert(tool.clone());
                }
                self.marker("approval", &format!("Approved: {summary}"), None).await;
                let outcome = self.call_tool(&tool, arguments, general).await;
                self.after_tool(&tool, &outcome).await;
                let _ = AgentApproval::resolve_by_broker(&approval_id, &status, Some(outcome.record())).await;
                self.respond(&request_id, outcome.response()).await;
            }
            "cancelled" => {
                self.marker("approval", &format!("Stopped by the technician: {summary}"), None).await;
                let outcome = ToolOutcome::failure("The technician stopped the agent; the call was not run.".into());
                let _ = AgentApproval::resolve_by_broker(&approval_id, "cancelled", Some(outcome.record())).await;
                self.respond(&request_id, outcome.response()).await;
                if let Some(t) = self.codex_thread_id.clone() {
                    let _ = self.client.turn_interrupt(&t).await;
                }
            }
            "declined" => {
                let why = if note.trim().is_empty() { String::new() } else { format!(": {}", note.trim()) };
                self.marker("approval", &format!("Declined by the technician{why}: {summary}"), None).await;
                let outcome = ToolOutcome::failure(format!(
                    "Declined by the technician{why}. Do not retry this call; explain what you needed and ask them in chat."
                ));
                let _ = AgentApproval::resolve_by_broker(&approval_id, "declined", Some(outcome.record())).await;
                self.respond(&request_id, outcome.response()).await;
            }
            _ => {
                let mins = self.cfg.approval_ttl_secs / 60;
                self.marker("approval", &format!("No technician answered within {mins} min: {summary}"), None).await;
                let outcome = ToolOutcome::failure(format!(
                    "No technician answered within {mins} minutes, so the call was not run. Continue with what you can do without it and ask the technician in chat."
                ));
                let _ = AgentApproval::resolve_by_broker(&approval_id, "expired", Some(outcome.record())).await;
                self.respond(&request_id, outcome.response()).await;
            }
        }
    }

    /// Prepends what ZeroClaw remembers about the machine to the opening turn.
    async fn with_memory_brief(&self, opening: String) -> String {
        let Some(mem) = &self.memory else { return opening };
        if self.general() {
            return opening;
        }
        let query = format!(
            "{} {}",
            self.thread.hostname.clone().unwrap_or_default(),
            self.thread.service_number.clone().unwrap_or_default()
        )
        .trim()
        .to_string();
        if query.is_empty() {
            return opening;
        }
        match tokio::time::timeout(Duration::from_secs(10), mem.recall(zeroclaw::MACHINE_AGENT, &query)).await {
            Ok(Ok(entries)) if !entries.is_empty() => format!(
                "ZEROCLAW MEMORY BRIEF (agent {}; verify against this machine before acting):\n{}\n\n{opening}",
                zeroclaw::MACHINE_AGENT,
                zeroclaw::render_entries(&entries)
            ),
            Ok(Ok(_)) => opening,
            Ok(Err(e)) => {
                log::warn!("codex: memory brief failed: {e}");
                opening
            }
            Err(_) => {
                log::warn!("codex: memory brief timed out");
                opening
            }
        }
    }

    /// `zeroclaw_recall` / `zeroclaw_remember` against the session's memory alias.
    async fn memory_tool(&self, tool: &str, arguments: &Value) -> ToolOutcome {
        let Some(mem) = &self.memory else {
            return ToolOutcome::failure("ZeroClaw memory is not configured on this broker".into());
        };
        let agent = ZeroclawMemory::agent_for(&self.thread.connection_string);
        let arg = |k: &str| arguments.get(k).and_then(Value::as_str).unwrap_or("").trim().to_string();
        match tool {
            "zeroclaw_recall" => {
                let query = arg("query");
                if query.is_empty() {
                    return ToolOutcome::failure("query is required".into());
                }
                match mem.recall(agent, &query).await {
                    Ok(entries) if entries.is_empty() => ToolOutcome::ok("no matching memories".into()),
                    Ok(entries) => ToolOutcome::ok(zeroclaw::render_entries(&entries)),
                    Err(e) => ToolOutcome::failure(format!("memory recall failed: {e}")),
                }
            }
            "zeroclaw_remember" => {
                let (key, content) = (arg("key"), arg("content"));
                if key.is_empty() || content.is_empty() {
                    return ToolOutcome::failure("key and content are required".into());
                }
                let category = match arg("category").as_str() {
                    "" => "core".to_string(),
                    c => c.to_string(),
                };
                match mem.store(agent, &key, &content, &category).await {
                    Ok(()) => ToolOutcome::ok(format!("remembered `{key}` ({category}) for agent {agent}")),
                    Err(e) => ToolOutcome::failure(format!("memory store failed: {e}")),
                }
            }
            other => ToolOutcome::failure(format!("unknown memory tool {other}")),
        }
    }

    /// Leaves a daily note in ZeroClaw's memory with the agent's last word on the session.
    async fn remember_session(&self) {
        let Some(mem) = self.memory.clone() else { return };
        let Ok(rows) = AgentEvent::history(&self.thread.id, 0, 500).await else { return };
        let Some(last) = rows.iter().rev().find(|e| e.kind == "agent" && !e.text.trim().is_empty()) else { return };
        let agent = ZeroclawMemory::agent_for(&self.thread.connection_string);
        let key = format!("codex/{}", self.thread.id.key_string());
        let summary: String = last.text.chars().take(1200).collect();
        let content = format!(
            "{} ({}) session closed. Agent's last word:\n{summary}",
            self.thread.label(),
            self.thread.connection_string
        );
        tokio::spawn(async move {
            if let Err(e) = mem.store(agent, &key, &content, "daily").await {
                log::warn!("codex: memory write-back failed: {e}");
            }
        });
    }

    /// Side effects worth recording from a tool result.
    async fn after_tool(&self, tool: &str, outcome: &ToolOutcome) {
        if tool == "create_diagnostic_session" && outcome.success {
            if let Some(key) = session_key_in(&outcome.text) {
                let session = RecordId::new("diagnostic_session", key.as_str());
                let _ = AgentThread::set_diagnostic_session(&self.thread.id, &session).await;
            }
        }
    }

    async fn on_question(&mut self, request_id: Value, params: Value) {
        let questions = params.get("questions").cloned().unwrap_or_else(|| json!([]));
        let first = questions
            .as_array()
            .and_then(|a| a.first())
            .and_then(|q| q.get("question"))
            .and_then(Value::as_str)
            .unwrap_or("The agent has a question")
            .to_string();
        let new = NewAgentApproval {
            thread: Some(self.thread.id.clone()),
            kind: "question".into(),
            method: "item/tool/requestUserInput".into(),
            codex_request_id: request_id.to_string(),
            summary: first.clone(),
            server: None,
            tool: Some("request_user_input".into()),
            arguments: None,
            params: Some(params.clone()),
            questions: Some(questions.clone()),
            assignee: self.thread.assignee.clone(),
            connection_string: Some(self.thread.connection_string.clone()),
            store: self.thread.store.clone(),
            ttl_secs: self.cfg.approval_ttl_secs,
        };
        let approval_id = match AgentApproval::create(&new).await {
            Ok(id) => id,
            Err(e) => {
                log::warn!("codex: could not record question: {e}");
                self.respond(&request_id, json!({ "answers": {} })).await;
                return;
            }
        };
        self.set_status("waiting_approval").await;
        self.marker(
            "approval",
            &format!("Agent asks the technician: {first}"),
            Some(json!({ "approval": approval_id.key_string(), "questions": questions })),
        )
        .await;
        let row = self.wait_for_decision(&approval_id).await;
        self.set_status("running").await;
        let answers = row
            .as_ref()
            .filter(|r| r.status == "answered")
            .and_then(|r| r.answers.clone())
            .map(normalize_answers)
            .unwrap_or_else(|| json!({}));
        let body = json!({ "answers": answers });
        let final_status = if row.as_ref().is_some_and(|r| r.status == "answered") { "answered" } else { "expired" };
        let _ = AgentApproval::resolve_by_broker(&approval_id, final_status, Some(body.clone())).await;
        self.marker(
            "approval",
            if final_status == "answered" { "Technician answered." } else { "No answer from a technician." },
            None,
        )
        .await;
        self.respond(&request_id, body).await;
    }

    /// Polls the approval row until a technician decides or the deadline passes.
    async fn wait_for_decision(&self, approval_id: &RecordId) -> Option<AgentApproval> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(self.cfg.approval_ttl_secs + 15);
        loop {
            tokio::time::sleep(APPROVAL_POLL).await;
            match AgentApproval::fetch(approval_id).await {
                Ok(Some(row)) if !row.is_pending() => return Some(row),
                Ok(Some(_)) => {}
                // A vanished row is a refusal, never a permission.
                Ok(None) => return None,
                Err(e) => log::warn!("codex: approval poll failed: {e}"),
            }
            if tokio::time::Instant::now() >= deadline {
                let _ = AgentApproval::expire_stale().await;
                return AgentApproval::fetch(approval_id).await.ok().flatten();
            }
        }
    }

    async fn send_turn(&mut self, kind: &str, text: &str) -> anyhow::Result<()> {
        let Some(thread_id) = self.codex_thread_id.clone() else {
            anyhow::bail!("no codex thread yet");
        };
        match kind {
            "steer" => self.client.turn_steer(&thread_id, text).await?,
            _ => self.client.turn_start(&thread_id, text).await?,
        };
        Ok(())
    }

    async fn on_turn(&mut self, turn: AgentTurn) -> Flow {
        match turn.kind.as_str() {
            "start" | "steer" => {
                if let Err(e) = self.send_turn(&turn.kind, &turn.text).await {
                    log::warn!("codex: turn {} failed: {e}", turn.id.key_string());
                    let _ = AgentTurn::mark_failed(&turn.id, &e.to_string()).await;
                    self.marker("error", &format!("Could not deliver the technician's message: {e}"), None).await;
                }
                Flow::Continue
            }
            "interrupt" => {
                if let Some(t) = self.codex_thread_id.clone() {
                    if let Err(e) = self.client.turn_interrupt(&t).await {
                        let _ = AgentTurn::mark_failed(&turn.id, &e.to_string()).await;
                    }
                }
                self.marker("other", "Technician interrupted the agent.", None).await;
                Flow::Continue
            }
            "close" => {
                self.remember_session().await;
                if let Some(t) = self.codex_thread_id.clone() {
                    let _ = self.client.request("thread/unsubscribe", json!({ "threadId": t })).await;
                }
                self.marker("other", "Session closed by the technician.", None).await;
                self.set_status("closed").await;
                if let Some(req) = &self.thread.assist_request {
                    let _ = AssistRequest::finish(req, "completed", None).await;
                }
                Flow::Closed
            }
            other => {
                let _ = AgentTurn::mark_failed(&turn.id, &format!("unknown turn kind {other}")).await;
                Flow::Continue
            }
        }
    }
}

/// Transcript kind for a codex item type.
fn kind_for(item_type: &str) -> &'static str {
    match item_type {
        "userMessage" => "user",
        "agentMessage" => "agent",
        "reasoning" => "reasoning",
        "commandExecution" => "command",
        "fileChange" => "file_change",
        "mcpToolCall" | "dynamicToolCall" | "functionCallOutput" => "tool_call",
        _ => "other",
    }
}

/// Replaces embedded image data in a stored item with a note of its size.
fn redact_images(value: &mut Value) {
    match value {
        Value::String(s) if s.starts_with("data:") && s.len() > DATA_URL_KEEP => {
            let mime: String = s[5..].split([';', ',']).next().unwrap_or_default().chars().take(40).collect();
            *s = format!("[{mime} omitted, {} bytes]", s.len());
        }
        Value::Array(items) => items.iter_mut().for_each(redact_images),
        Value::Object(map) => {
            let raw_image = map.get("type").and_then(Value::as_str) == Some("image");
            for (key, v) in map.iter_mut() {
                match v {
                    Value::String(s) if raw_image && key == "data" && s.len() > DATA_URL_KEEP => {
                        *s = format!("[image omitted, {} bytes]", s.len());
                    }
                    other => redact_images(other),
                }
            }
        }
        _ => {}
    }
}

fn join_texts(v: Option<&Value>) -> String {
    match v {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|i| i.as_str().map(str::to_string).or_else(|| i.get("text").and_then(Value::as_str).map(str::to_string)))
            .collect::<Vec<_>>()
            .join("\n"),
        Some(Value::String(s)) => s.clone(),
        _ => String::new(),
    }
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let kept: String = s.chars().take(max).collect();
        format!("{kept}…")
    }
}

/// The searchable text of a completed item; the item itself stays on the row.
fn item_text(kind: &str, item: &Value) -> String {
    match kind {
        "agent" => item.get("text").and_then(Value::as_str).unwrap_or("").to_string(),
        "user" => join_texts(item.get("content")),
        "reasoning" => {
            let summary = join_texts(item.get("summary"));
            let content = join_texts(item.get("content"));
            if summary.is_empty() { content } else if content.is_empty() { summary } else { format!("{summary}\n{content}") }
        }
        "command" => {
            let cmd = item.get("command").and_then(Value::as_str).unwrap_or("");
            let out = item.get("aggregatedOutput").and_then(Value::as_str).unwrap_or("");
            clip(&format!("$ {cmd}\n{out}"), ROW_TEXT_CHARS)
        }
        "tool_call" => {
            let tool = item.get("tool").and_then(Value::as_str).unwrap_or("tool");
            let args = item.get("arguments").map(|a| a.to_string()).unwrap_or_default();
            let mut text = format!("{tool}({})", clip(&args, 600));
            if let Some(err) = item.pointer("/error/message").and_then(Value::as_str) {
                text.push_str(&format!("\nerror: {err}"));
            } else if let Some(result) = item.get("result").filter(|r| !r.is_null()) {
                text.push_str(&format!("\n{}", clip(&join_texts(result.get("content")).to_string(), ROW_TEXT_CHARS)));
            }
            text
        }
        "file_change" => "file changes".to_string(),
        _ => item.get("type").and_then(Value::as_str).unwrap_or("").to_string(),
    }
}

/// Turns `{qid: "text"}` or `{qid: ["a","b"]}` into codex's `{qid: {answers: [...]}}`.
fn normalize_answers(raw: Value) -> Value {
    let Some(map) = raw.as_object() else { return json!({}) };
    let mut out = serde_json::Map::new();
    for (qid, v) in map {
        let answers: Vec<Value> = match v {
            Value::Array(a) => a.iter().map(|x| json!(x.as_str().map(str::to_string).unwrap_or_else(|| x.to_string()))).collect(),
            Value::Object(o) if o.contains_key("answers") => {
                out.insert(qid.clone(), v.clone());
                continue;
            }
            other => vec![json!(other.as_str().map(str::to_string).unwrap_or_else(|| other.to_string()))],
        };
        out.insert(qid.clone(), json!({ "answers": answers }));
    }
    Value::Object(out)
}

/// The diagnostic session a `create_diagnostic_session` result names, from its
/// JSON `session_id` (a bare key) or a `diagnostic_session:<key>` mention.
fn session_key_in(text: &str) -> Option<String> {
    let from_json = serde_json::from_str::<Value>(text).ok().and_then(|v| {
        v.get("session_id")
            .or_else(|| v.get("id"))
            .and_then(Value::as_str)
            .map(|s| s.trim().trim_start_matches("diagnostic_session:").trim_matches(|c| c == '⟨' || c == '⟩').to_string())
            .filter(|s| !s.is_empty())
    });
    from_json.or_else(|| find_record_key(text, "diagnostic_session"))
}

/// Finds `<table>:<key>` in tool output; keys may be bare or ⟨bracketed⟩.
fn find_record_key(text: &str, table: &str) -> Option<String> {
    let needle = format!("{table}:");
    let start = text.find(&needle)? + needle.len();
    let rest = &text[start..];
    if let Some(inner) = rest.strip_prefix('⟨') {
        return inner.split('⟩').next().map(str::to_string).filter(|s| !s.is_empty());
    }
    let key: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
        .collect();
    (!key.is_empty()).then_some(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_take_strings_arrays_and_passthrough() {
        let v = normalize_answers(json!({ "q1": "yes", "q2": ["a", "b"], "q3": { "answers": ["x"] } }));
        assert_eq!(v["q1"]["answers"], json!(["yes"]));
        assert_eq!(v["q2"]["answers"], json!(["a", "b"]));
        assert_eq!(v["q3"]["answers"], json!(["x"]));
    }

    #[test]
    fn record_keys_are_found_bare_or_bracketed() {
        assert_eq!(find_record_key("created diagnostic_session:abc12-3 ok", "diagnostic_session").as_deref(), Some("abc12-3"));
        assert_eq!(find_record_key("id diagnostic_session:⟨9f-0⟩", "diagnostic_session").as_deref(), Some("9f-0"));
        assert_eq!(find_record_key("nothing here", "diagnostic_session"), None);
    }

    #[test]
    fn session_key_comes_from_json_or_a_record_mention() {
        assert_eq!(session_key_in(r#"{"session_id":"b97ac649-b560","warnings":[]}"#).as_deref(), Some("b97ac649-b560"));
        assert_eq!(session_key_in(r#"{"id":"diagnostic_session:⟨abc⟩"}"#).as_deref(), Some("abc"));
        assert_eq!(session_key_in("opened diagnostic_session:xyz9").as_deref(), Some("xyz9"));
        assert_eq!(session_key_in("nothing"), None);
    }

    #[test]
    fn tool_rows_carry_the_call_and_the_error() {
        let item = json!({ "tool": "query_surrealdb", "arguments": { "query": "SELECT 1" }, "error": { "message": "boom" } });
        let text = item_text("tool_call", &item);
        assert!(text.starts_with("query_surrealdb("));
        assert!(text.ends_with("error: boom"));
    }

    #[test]
    fn stored_items_keep_no_image_data() {
        let url = format!("data:image/jpeg;base64,{}", "A".repeat(4000));
        let mut item = json!({
            "type": "dynamicToolCall",
            "contentItems": [
                { "type": "inputText", "text": "{\"image_width\":1280}" },
                { "type": "inputImage", "imageUrl": url },
            ],
            "result": { "content": [{ "type": "image", "mimeType": "image/png", "data": "B".repeat(4000) }] },
            "note": "data:short",
        });
        redact_images(&mut item);
        let stored = item.to_string();
        assert!(!stored.contains("AAAA") && !stored.contains("BBBB"), "{stored}");
        assert_eq!(item["contentItems"][1]["imageUrl"], json!(format!("[image/jpeg omitted, {} bytes]", url.len())));
        assert_eq!(item["result"]["content"][0]["data"], json!("[image omitted, 4000 bytes]"));
        assert_eq!(item["contentItems"][0]["text"], json!("{\"image_width\":1280}"));
        assert_eq!(item["note"], json!("data:short"));
    }
}
