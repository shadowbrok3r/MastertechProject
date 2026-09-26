//! One Codex thread: connects to zc-codexd, streams codex events into
//! agent_event rows, answers the requests codex addresses to its client (tool
//! calls, technician questions, approvals) and applies technician turns.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine;
use database::schema::agent_approval::ACCEPTED_ALL_FOR_SESSION;
use database::schema::agent_thread::AgentThreadState;
use database::schema::agent_turn::APPROVALS_PROMPT;
use database::schema::{
    AgentActivity, AgentApproval, AgentEvent, AgentThread, AgentTurn, AssistRequest,
    DEFAULT_UPLOAD_DIR, NewAgentApproval, RecordId, RecordIdExt, TurnImage, upload_name,
};
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot};
use zc_codex_client::{decision, elicitation, Client, Event};

use super::busy::{self, Busy, Signal};
use super::coalesce::{self, Partial, ThreadRow, TranscriptBuffer};
use super::parse_check;
use super::queue::{self, TurnQueue};
use super::tools::{scope_violation, ToolHost, ToolOutcome, ToolPolicy};
use super::wait;
use super::zeroclaw::{self, ZeroclawMemory};
use super::{manager, prompt, register_runner, runner_for, unregister_runner, Config};

/// What the outside world may ask a running thread to do.
#[derive(Debug, Clone)]
pub enum RunnerCmd {
    Turn(AgentTurn),
    /// A stop request, sent from its own task, got its answer.
    Stopped {
        turn: Option<RecordId>,
        result: Result<(), String>,
    },
    /// Frees the pool slot if the thread is idle; the thread stays open.
    Release,
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
/// Idle time after which a runner frees its pool slot; the thread's next turn brings one up again.
const IDLE_RELEASE: Duration = Duration::from_secs(10 * 60);
/// Longest decoded picture the broker stages.
const IMAGE_MAX_BYTES: usize = 8 * 1024 * 1024;
/// How long a stop request may take before it is reported as failed.
const STOP_TIMEOUT: Duration = Duration::from_secs(30);
/// Ceiling on the small reads (`thread/read`, `daemon/hello`) the runner makes between turns.
const READ_TIMEOUT: Duration = Duration::from_secs(20);
/// Transcript text of a context compaction while it runs and once it is done.
const COMPACTING: &str = "Compacting the conversation to free context\u{2026}";
const COMPACTED: &str = "Conversation compacted to free context.";
/// Transcript notes for approve-all turning on and off.
const APPROVED_ALL: &str = "Technician approved everything for this session.";
pub(super) const PROMPTS_ON: &str = "Technician turned approval prompts back on; gated tool calls ask again.";
/// Characters of a call's reason kept in its approval summary.
const SUMMARY_REASON_CHARS: usize = 160;

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
    let own = tx.clone();
    tokio::spawn(async move {
        if let Err(e) = Runner::run(cfg, thread, opening, rx, own.clone()).await {
            log::warn!("codex: thread {key} ended with error: {e}");
        }
        unregister_runner(&key, &own);
    });
    tx
}

enum Flow {
    Continue,
    Reconnect,
    Closed,
    /// Idle past [`IDLE_RELEASE`]: the runner hands its pool slot back.
    Release,
}

/// What the broker does with an approval row it read back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    /// Act on the row as it stands.
    Accept,
    /// Keep polling.
    Wait,
    /// A decision from someone not allowed to make it; return the row to pending.
    Reopen,
    /// Give up and treat the request as unanswered.
    Expire,
}

/// The verdict on `row`; `allowed` is the decider check, `None` when it could not be made.
fn verdict(row: &AgentApproval, allowed: Option<bool>, at_deadline: bool) -> Verdict {
    if row.is_pending() {
        return if at_deadline { Verdict::Expire } else { Verdict::Wait };
    }
    if !row.is_human_decision() {
        return Verdict::Accept;
    }
    match allowed {
        Some(true) => Verdict::Accept,
        _ if at_deadline => Verdict::Expire,
        Some(false) => Verdict::Reopen,
        None => Verdict::Wait,
    }
}

/// Where a verified poll leaves `wait_for_decision`.
enum Step {
    Done(Option<AgentApproval>),
    Wait,
}

/// A `wait` call answered by its own task.
struct PendingWait {
    request_id: Value,
    cancel: oneshot::Sender<String>,
}

struct Runner {
    cfg: Arc<Config>,
    thread: AgentThread,
    tools: Arc<ToolHost>,
    client: Client,
    /// The runner's own command channel, for tasks that report back.
    own: mpsc::Sender<RunnerCmd>,
    codex_thread_id: Option<String>,
    next_seq: i64,
    seqs: HashMap<String, i64>,
    /// Items whose authoritative row is written; later deltas for them are dropped.
    completed: HashSet<String>,
    transcript: TranscriptBuffer,
    row: ThreadRow,
    waits: Vec<PendingWait>,
    turn_no: u32,
    remembered: HashSet<String>,
    /// Every gated call runs without asking; restored from the thread row.
    approve_all: bool,
    /// Set once the first attach succeeds; a later attach is an in-process reconnect.
    attached: bool,
    memory: Option<Arc<ZeroclawMemory>>,
    /// When the thread last went idle; `None` while a turn is in progress.
    idle_since: Option<Instant>,
    busy: Busy,
    queue: TurnQueue,
    /// Compaction asked for while a turn ran, sent once it ends.
    pending_compact: Option<AgentTurn>,
    /// Commands that arrived while a tool call or an approval held the loop.
    deferred: VecDeque<RunnerCmd>,
    /// The daemon's staging directory, read from its hello on first use per connection.
    upload_dir: Option<String>,
    /// A non-retry error ended the current turn.
    turn_failed: bool,
    /// A stop request for the current turn is out.
    stopping: bool,
    /// A compaction was sent and no turn has started for it yet.
    compact_unstarted: bool,
}

impl Runner {
    async fn run(
        cfg: Arc<Config>,
        thread: AgentThread,
        opening: Option<String>,
        mut rx: mpsc::Receiver<RunnerCmd>,
        own: mpsc::Sender<RunnerCmd>,
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
            approve_all: thread.approves_all(),
            memory: cfg.zeroclaw.clone(),
            cfg,
            thread,
            tools,
            client,
            own,
            seqs: HashMap::new(),
            completed: HashSet::new(),
            transcript: TranscriptBuffer::new(coalesce::ITEM_FLUSH),
            row,
            waits: Vec::new(),
            turn_no: 0,
            remembered: HashSet::new(),
            attached: false,
            idle_since: None,
            busy: Busy::default(),
            queue: TurnQueue::default(),
            pending_compact: None,
            deferred: VecDeque::new(),
            upload_dir: None,
            turn_failed: false,
            stopping: false,
            compact_unstarted: false,
        };
        if let Err(e) = me.attach().await {
            me.write_status("failed", Some(&format!("thread start: {e}"))).await?;
            return Err(e);
        }
        me.load_queue().await;
        if let Some(text) = opening {
            let text = me.with_memory_brief(text).await;
            me.send_text("start", &text).await?;
        }
        me.pump().await;

        let mut tick = tokio::time::interval(FLUSH_TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            let flow = match me.deferred.pop_front() {
                Some(cmd) => me.on_cmd(cmd).await,
                None => tokio::select! {
                    ev = events.recv() => match ev {
                        Some(ev) => me.on_event(ev, &mut rx).await,
                        None => Flow::Reconnect,
                    },
                    cmd = rx.recv() => match cmd {
                        Some(cmd) => me.on_cmd(cmd).await,
                        None => Flow::Closed,
                    },
                    _ = tick.tick() => {
                        me.flush_due().await;
                        if me.idle_expired() { Flow::Release } else { Flow::Continue }
                    }
                },
            };
            match flow {
                Flow::Continue => {}
                Flow::Closed => break,
                Flow::Release => {
                    me.release(rx).await;
                    return Ok(());
                }
                Flow::Reconnect => match me.reconnect().await {
                    Ok(new_events) => {
                        events = new_events;
                        me.pump().await;
                    }
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

    /// Mastertech tools, `wait` on a machine session, and the ZeroClaw memory tools when configured.
    fn dynamic_tools(&self, general: bool) -> Vec<Value> {
        let mut specs = self.tools.dynamic_specs(general);
        if !general {
            specs.push(wait::tool_spec());
        }
        if self.memory.is_some() {
            specs.extend(zeroclaw::tool_specs());
        }
        specs
    }

    /// Parameters shared by `thread/start` and `thread/resume`.
    fn thread_params(&self) -> Value {
        let general = self.general();
        let mut offered = self.tools.offered(general);
        if !general {
            offered.push(wait::TOOL_NAME.to_string());
        }
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

    /// Resumes the recorded codex thread, or starts a new one, and records whether a turn still runs.
    async fn attach(&mut self) -> anyhow::Result<()> {
        let first = !self.attached;
        self.upload_dir = None;
        if let Some(existing) = self.codex_thread_id.clone() {
            let mut params = self.thread_params();
            params["threadId"] = json!(existing);
            match self.client.request("thread/resume", params).await {
                Ok(_) => {
                    log::info!("codex: resumed codex thread {existing} for {}", self.thread.connection_string);
                    self.backfill(&existing).await;
                    let in_progress = self.turn_in_progress().await.unwrap_or(false);
                    if first && matches!(self.thread.status.as_str(), "running" | "waiting_approval") {
                        self.reconcile_interrupted(in_progress).await;
                    }
                    self.attached = true;
                    if !in_progress {
                        self.stopping = false;
                    }
                    self.busy = self.busy.after(&Signal::Attached { in_progress });
                    self.row.set_activity(self.busy.activity.to_db());
                    self.write_status(self.busy.phase.status(), None).await?;
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
        self.busy = Busy::default();
        self.stopping = false;
        self.row.set_activity(self.busy.activity.to_db());
        self.write_status("idle", None).await?;
        log::info!("codex: thread {} -> codex {id}", self.thread.id.key_string());
        self.codex_thread_id = Some(id);
        self.attached = true;
        Ok(())
    }

    /// Whether the app-server still runs a turn on this thread; `None` when it could not be read.
    async fn turn_in_progress(&self) -> Option<bool> {
        let id = self.codex_thread_id.as_deref()?;
        let params = json!({ "threadId": id, "includeTurns": true });
        let snapshot = match self
            .client
            .request_with_timeout("thread/read", params, READ_TIMEOUT)
            .await
        {
            Ok(s) => s,
            Err(e) => {
                log::debug!("codex: thread/read failed: {e}");
                return None;
            }
        };
        let turns = snapshot.pointer("/thread/turns")?.as_array()?;
        Some(turns.iter().any(|t| t["status"] == "inProgress"))
    }

    /// Queue turns claimed before this runner started, in the order they were sent.
    async fn load_queue(&mut self) {
        let rows = match AgentTurn::queue_of(&self.thread.id).await {
            Ok(rows) => rows,
            Err(e) => {
                log::warn!(
                    "codex: could not read the queue of {}: {e}",
                    self.thread.id.key_string()
                );
                return;
            }
        };
        let mut held = false;
        for turn in rows {
            if turn.text.trim().is_empty() && turn.images.is_empty() {
                let _ = AgentTurn::take_queued(&turn.id).await;
                continue;
            }
            held |= turn.status == "held";
            self.queue.push(turn);
        }
        if held
            && self.queue.hold(queue::STOPPED)
            && let Err(e) = AgentTurn::hold_queue(&self.thread.id).await
        {
            log::warn!(
                "codex: could not hold the queue of {}: {e}",
                self.thread.id.key_string()
            );
        }
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
    async fn reconcile_interrupted(&mut self, in_progress: bool) {
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
        let note = if in_progress {
            "Broker restarted mid-turn and reattached to the running turn."
        } else {
            "Broker restarted while the agent was working; send a message if it went quiet."
        };
        self.marker("other", note, None).await;
    }

    async fn reconnect(&mut self) -> anyhow::Result<mpsc::Receiver<Event>> {
        self.stop_waits("the connection to the agent host dropped");
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

    async fn save_row(&mut self, state: AgentThreadState) {
        if let Err(e) = AgentThread::save_state(&self.thread.id, &state).await {
            log::warn!("codex: thread write failed: {e}");
            self.row.failed(&state);
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
        self.idle_since = match status {
            "idle" => self.idle_since.or_else(|| Some(Instant::now())),
            _ => None,
        };
        self.flush_transcript().await;
        let Some(state) = self.row.status(status, error, Instant::now()) else { return Ok(()) };
        let written = AgentThread::save_state(&self.thread.id, &state).await;
        if written.is_err() {
            self.row.failed(&state);
        }
        written
    }

    async fn set_status(&mut self, status: &str) {
        if let Err(e) = self.write_status(status, None).await {
            log::warn!("codex: status write failed: {e}");
        }
    }

    /// Moves the phase and activity on `signal`; a phase change is written at once, an activity later.
    async fn signal(&mut self, signal: Signal) {
        let next = self.busy.after(&signal);
        if next == self.busy {
            return;
        }
        let moved = next.phase != self.busy.phase;
        self.busy = next;
        self.row.set_activity(self.busy.activity.to_db());
        if moved {
            self.set_status(self.busy.phase.status()).await;
        }
    }

    /// Idle with no wait, stop or compaction outstanding.
    fn releasable(&self) -> bool {
        self.waits.is_empty()
            && self.busy.is_idle()
            && !self.stopping
            && self.pending_compact.is_none()
            && self.idle_since.is_some()
    }

    fn idle_expired(&self) -> bool {
        self.releasable() && self.idle_since.is_some_and(|t| t.elapsed() >= IDLE_RELEASE)
    }

    /// Frees the pool slot; a turn that reached this runner first goes to a fresh runner.
    async fn release(mut self, mut rx: mpsc::Receiver<RunnerCmd>) {
        let key = self.thread.id.key_string();
        unregister_runner(&key, &self.own);
        rx.close();
        let raced: Vec<AgentTurn> = std::mem::take(&mut self.deferred)
            .into_iter()
            .chain(std::iter::from_fn(|| rx.try_recv().ok()))
            .filter_map(|cmd| match cmd {
                RunnerCmd::Turn(turn) => Some(turn),
                RunnerCmd::Stopped { .. } | RunnerCmd::Release => None,
            })
            .collect();
        self.flush_all().await;
        log::info!("codex: released idle runner for thread {key}");
        if raced.is_empty() {
            return;
        }
        let thread = match AgentThread::get(&self.thread.id).await {
            Ok(Some(t)) => t,
            _ => self.thread.clone(),
        };
        let tx = spawn(self.cfg.clone(), thread, None);
        for turn in raced {
            let id = turn.id.clone();
            if tx.send(RunnerCmd::Turn(turn)).await.is_err() {
                let _ = AgentTurn::mark_failed(&id, "runner did not accept the turn").await;
            }
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

    async fn on_cmd(&mut self, cmd: RunnerCmd) -> Flow {
        match cmd {
            RunnerCmd::Turn(turn) => self.on_turn(turn).await,
            RunnerCmd::Stopped { turn, result } => {
                self.stop_answered(turn.as_ref(), result).await;
                Flow::Continue
            }
            RunnerCmd::Release if self.releasable() => Flow::Release,
            RunnerCmd::Release => Flow::Continue,
        }
    }

    async fn on_event(&mut self, ev: Event, rx: &mut mpsc::Receiver<RunnerCmd>) -> Flow {
        match ev {
            Event::Text { item_id, text, final_chunk, .. } => {
                if !final_chunk {
                    self.signal(Signal::Streaming(AgentActivity::Writing)).await;
                }
                self.stream_text(&item_id, "agent", &text, final_chunk).await
            }
            Event::Reasoning { item_id, text, final_chunk, .. } => {
                if !final_chunk {
                    self.signal(Signal::Streaming(AgentActivity::Thinking))
                        .await;
                }
                self.stream_text(&item_id, "reasoning", &text, final_chunk).await
            }
            Event::CommandOutput { item_id, text, final_chunk, .. } => {
                if !final_chunk {
                    self.signal(Signal::Streaming(AgentActivity::Command)).await;
                }
                self.stream_text(&item_id, "command", &text, final_chunk).await
            }
            Event::Item {
                item_type,
                completed,
                item,
                ..
            } => {
                if let Some(signal) = busy::item_signal(&item_type, completed, &item) {
                    self.signal(signal).await;
                }
                self.on_item(&item_type, completed, item).await
            }
            Event::Ask { request_id, method, params, .. } => {
                self.on_ask(request_id, &method, params, rx).await;
            }
            Event::AskResolved { request_id } => {
                self.stop_wait(&request_id, "the request was settled elsewhere");
            }
            Event::TurnStarted { .. } => {
                self.turn_no += 1;
                self.compact_unstarted = false;
                self.marker("turn_started", "", None).await;
                self.signal(Signal::TurnStarted).await;
            }
            Event::TurnCompleted { .. } => self.turn_completed().await,
            Event::Error { message, will_retry, .. } => {
                let text = if will_retry {
                    format!("Agent hit a transient error and is retrying: {message}")
                } else {
                    format!("Agent error: {message}")
                };
                self.marker("error", &text, None).await;
                if will_retry {
                    self.signal(Signal::Error { will_retry }).await;
                } else {
                    self.turn_failed = true;
                    self.busy = self.busy.after(&Signal::Error { will_retry });
                    self.row.set_activity(self.busy.activity.to_db());
                    if let Err(e) = self
                        .write_status(self.busy.phase.status(), Some(&message))
                        .await
                    {
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
            Event::Other { method, params } => match method.as_str() {
                "connection/closed" => return Flow::Reconnect,
                "thread/status/changed" => {
                    if let Some(status) = busy::server_status(&params) {
                        self.signal(Signal::Server(status)).await;
                    }
                }
                "thread/compacted" if self.compact_unstarted => {
                    self.compact_unstarted = false;
                    if !self.busy.is_idle() {
                        self.signal(Signal::TurnEnded).await;
                        self.pump().await;
                    }
                }
                _ => log::debug!("codex: {method} ignored"),
            },
        }
        Flow::Continue
    }

    /// A turn ended: a failed one holds the queue, then the next queued message goes out.
    async fn turn_completed(&mut self) {
        self.stop_waits("the turn ended");
        self.flush_transcript().await;
        self.transcript.clear();
        self.marker("turn_completed", "", None).await;
        self.signal(Signal::TurnEnded).await;
        let failed = std::mem::take(&mut self.turn_failed);
        let stopped = std::mem::take(&mut self.stopping);
        if failed && !stopped {
            self.hold_queue(queue::FAILED).await;
        }
        self.pump().await;
    }

    async fn on_item(&mut self, item_type: &str, completed: bool, mut item: Value) {
        let Some(item_id) = item.get("id").and_then(Value::as_str).map(str::to_string) else { return };
        let kind = kind_for(item_type);
        let now = Instant::now();
        if !completed {
            if !self.transcript.contains(&item_id) && !self.completed.contains(&item_id) {
                let seq = self.seq_for(&item_id);
                let turn = self.turn_label();
                let text = if item_type == "contextCompaction" {
                    COMPACTING.to_string()
                } else {
                    item_text(kind, &item)
                };
                self.transcript.open(&item_id, seq, kind, turn, text, now);
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

    async fn on_ask(
        &mut self,
        request_id: Value,
        method: &str,
        params: Value,
        rx: &mut mpsc::Receiver<RunnerCmd>,
    ) {
        match method {
            "item/tool/call" => self.on_tool_call(request_id, params, rx).await,
            "item/tool/requestUserInput" => self.on_question(request_id, params, rx).await,
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

    /// Starts a task that runs the `wait` and answers the call itself.
    async fn start_wait(&mut self, request_id: Value, arguments: &Value) {
        let spec = match wait::WaitSpec::parse(arguments) {
            Ok(spec) => spec,
            Err(e) => {
                self.respond(&request_id, ToolOutcome::failure(e).response()).await;
                return;
            }
        };
        if spec.needs_machine() && self.general() {
            let refusal = ToolOutcome::failure("no machine is in scope in this session; only a plain wait works here".into());
            self.respond(&request_id, refusal.response()).await;
            return;
        }
        self.waits.retain(|w| !w.cancel.is_closed());
        let (cancel, cancelled) = oneshot::channel();
        let observer = wait::SessionTools {
            tools: self.tools.clone(),
            connection_string: self.thread.connection_string.clone(),
        };
        let client = self.client.clone();
        let reply_to = request_id.clone();
        tokio::spawn(async move {
            let outcome = wait::run(observer, spec, wait::CHECK_EVERY, cancelled).await;
            if let Err(e) = client.respond(&reply_to, outcome.response()).await {
                log::warn!("codex: wait reply failed: {e}");
            }
        });
        self.waits.push(PendingWait { request_id, cancel });
    }

    /// Ends every running wait with `reason`.
    fn stop_waits(&mut self, reason: &str) {
        for wait in self.waits.drain(..) {
            let _ = wait.cancel.send(reason.to_string());
        }
    }

    fn stop_wait(&mut self, request_id: &Value, reason: &str) {
        if let Some(i) = self.waits.iter().position(|w| &w.request_id == request_id) {
            let _ = self.waits.swap_remove(i).cancel.send(reason.to_string());
        }
    }

    /// A command that arrived while a tool call or an approval held the loop: a stop acts now, the rest wait.
    async fn side_cmd(&mut self, cmd: Option<RunnerCmd>) {
        match cmd {
            Some(RunnerCmd::Turn(turn)) if turn.kind == "interrupt" => self.interrupt(&turn).await,
            Some(RunnerCmd::Turn(turn)) if turn.kind == "approvals" => self.approvals(&turn).await,
            Some(RunnerCmd::Turn(turn)) if turn.kind == "close" => {
                if !self.stopping {
                    self.begin_stop(None);
                }
                self.deferred.push_back(RunnerCmd::Turn(turn));
            }
            Some(cmd) => self.deferred.push_back(cmd),
            None => {}
        }
    }

    /// Runs a tool while answering stop requests and writing buffered rows.
    async fn call_tool(
        &mut self,
        tool: &str,
        arguments: Value,
        general: bool,
        rx: &mut mpsc::Receiver<RunnerCmd>,
    ) -> ToolOutcome {
        self.signal(Signal::Working(AgentActivity::Tool(tool.to_string())))
            .await;
        let tools = self.tools.clone();
        let call = tools.call(tool, arguments, general);
        tokio::pin!(call);
        let mut tick = tokio::time::interval(FLUSH_TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                outcome = &mut call => return outcome,
                cmd = rx.recv() => self.side_cmd(cmd).await,
                _ = tick.tick() => self.flush_due().await,
            }
        }
    }

    async fn on_tool_call(
        &mut self,
        request_id: Value,
        params: Value,
        rx: &mut mpsc::Receiver<RunnerCmd>,
    ) {
        let tool = params
            .get("tool")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        if tool == wait::TOOL_NAME {
            self.start_wait(request_id, &arguments).await;
            return;
        }
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
        let summary = self.call_summary(&tool, &arguments);
        let gate = self.tools.policy.gate(&tool, &arguments, self.approve_all, &self.remembered);
        if !gate.needs_human() {
            if let Some(note) = gate.note() {
                let details = json!({ "tool": tool, "arguments": arguments });
                self.marker("approval", &format!("{note}: {summary}"), Some(details)).await;
            }
            let outcome = self.call_tool(&tool, arguments, general, rx).await;
            self.after_tool(&tool, &outcome).await;
            self.respond(&request_id, outcome.response()).await;
            return;
        }
        if let Some(refusal) = self.parse_refusal(&tool, &arguments, general, rx).await {
            let details = json!({ "tool": tool, "arguments": arguments });
            self.marker("approval", &format!("{refusal}\nCall: {summary}"), Some(details)).await;
            self.respond(&request_id, ToolOutcome::failure(refusal).response()).await;
            return;
        }

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
        self.signal(Signal::Approval(tool.clone())).await;
        self.marker(
            "approval",
            &format!("Waiting for a technician to approve: {summary}"),
            Some(json!({ "approval": approval_id.key_string(), "tool": tool, "arguments": arguments })),
        )
        .await;

        let row = self.wait_for_decision(&approval_id, rx).await;
        self.signal(Signal::Decided).await;
        let status = match &row {
            _ if self.stopping => "cancelled".to_string(),
            Some(r) => r.status.clone(),
            None => "expired".to_string(),
        };
        let note = row.as_ref().and_then(|r| r.deny_note.clone()).unwrap_or_default();
        match decision(&status) {
            Decision::Run { remember, approve_all } => {
                if remember && self.tools.policy.may_remember(&tool) {
                    self.remembered.insert(tool.clone());
                }
                self.marker("approval", &format!("Approved: {summary}"), None).await;
                let outcome = self.call_tool(&tool, arguments, general, rx).await;
                self.after_tool(&tool, &outcome).await;
                if approve_all {
                    self.set_approve_all(true).await;
                }
                let _ = AgentApproval::resolve_by_broker(&approval_id, &status, Some(outcome.record())).await;
                self.respond(&request_id, outcome.response()).await;
            }
            Decision::Cancelled => {
                self.marker("approval", &format!("Stopped by the technician: {summary}"), None).await;
                let outcome = ToolOutcome::failure("The technician stopped the agent; the call was not run.".into());
                let _ = AgentApproval::resolve_by_broker(&approval_id, "cancelled", Some(outcome.record())).await;
                self.respond(&request_id, outcome.response()).await;
                if !self.stopping {
                    self.hold_queue(queue::STOPPED).await;
                    self.begin_stop(None);
                }
            }
            Decision::Declined => {
                let why = if note.trim().is_empty() { String::new() } else { format!(": {}", note.trim()) };
                self.marker("approval", &format!("Declined by the technician{why}: {summary}"), None).await;
                let outcome = ToolOutcome::failure(format!(
                    "Declined by the technician{why}. Do not retry this call; explain what you needed and ask them in chat."
                ));
                let _ = AgentApproval::resolve_by_broker(&approval_id, "declined", Some(outcome.record())).await;
                self.respond(&request_id, outcome.response()).await;
            }
            Decision::Expired => {
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

    /// `run <tool> on <machine>`, with the call's stated reason when it gives one.
    fn call_summary(&self, tool: &str, arguments: &Value) -> String {
        let machine = self.thread.hostname.clone().unwrap_or_else(|| self.thread.connection_string.clone());
        match arguments.get("reason").and_then(Value::as_str).map(str::trim).filter(|r| !r.is_empty()) {
            Some(reason) => format!("run {tool} on {machine} ({})", clip(reason, SUMMARY_REASON_CHARS)),
            None => format!("run {tool} on {machine}"),
        }
    }

    /// Parse-checks a PowerShell job on the client; the refusal when its script does not parse.
    async fn parse_refusal(
        &mut self,
        tool: &str,
        arguments: &Value,
        general: bool,
        rx: &mut mpsc::Receiver<RunnerCmd>,
    ) -> Option<String> {
        if tool != "remote_exec_start" || self.stopping {
            return None;
        }
        let check = parse_check::Check::for_job(arguments, &self.thread.connection_string)?;
        let started = self.call_tool("remote_exec_start", check.start_arguments(), general, rx).await;
        let Some(job_id) = parse_check::job_id(&started.text).filter(|_| started.success) else {
            log::debug!("codex: parse check did not start: {}", clip(&started.text, 300));
            return None;
        };
        let waited = self.call_tool("remote_exec_wait", check.wait_arguments(&job_id), general, rx).await;
        let Some(problems) = parse_check::problems(&waited.text).filter(|_| waited.success) else {
            log::debug!("codex: parse check {job_id} gave no verdict: {}", clip(&waited.text, 300));
            return None;
        };
        (!problems.is_empty()).then(|| parse_check::refusal(&problems))
    }

    /// Turns approve-all on or off on the thread row too; a change leaves a transcript note.
    async fn set_approve_all(&mut self, on: bool) {
        let changed = self.approve_all != on;
        self.approve_all = on;
        self.thread.approve_all = Some(on);
        if let Err(e) = AgentThread::set_approve_all(&self.thread.id, on).await {
            log::warn!("codex: could not record approve-all on {}: {e}", self.thread.id.key_string());
        }
        if changed {
            self.marker("approval", if on { APPROVED_ALL } else { PROMPTS_ON }, None).await;
        }
    }

    /// An `approvals` turn: `prompt` turns approve-all off.
    async fn approvals(&mut self, turn: &AgentTurn) {
        if turn.text.trim() == APPROVALS_PROMPT {
            self.set_approve_all(false).await;
        } else {
            let _ = AgentTurn::mark_failed(&turn.id, "unknown approvals setting").await;
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
        let host = self.thread.hostname.clone().unwrap_or_default();
        let service = self.thread.service_number.clone().unwrap_or_default();
        let about = [host.as_str(), service.as_str(), self.thread.connection_string.as_str()];
        match tokio::time::timeout(Duration::from_secs(10), mem.recall(zeroclaw::MACHINE_AGENT, &query)).await {
            Ok(Ok(entries)) => {
                let entries = zeroclaw::entries_about(entries, &about);
                if entries.is_empty() {
                    return opening;
                }
                format!(
                    "ZEROCLAW MEMORY BRIEF (agent {}; verify against this machine before acting):\n{}\n\n{opening}",
                    zeroclaw::MACHINE_AGENT,
                    zeroclaw::render_entries(&entries)
                )
            }
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

    async fn on_question(
        &mut self,
        request_id: Value,
        params: Value,
        rx: &mut mpsc::Receiver<RunnerCmd>,
    ) {
        let questions = params
            .get("questions")
            .cloned()
            .unwrap_or_else(|| json!([]));
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
        self.signal(Signal::Approval("question".into())).await;
        self.marker(
            "approval",
            &format!("Agent asks the technician: {first}"),
            Some(json!({ "approval": approval_id.key_string(), "questions": questions })),
        )
        .await;
        let row = self.wait_for_decision(&approval_id, rx).await;
        self.signal(Signal::Decided).await;
        let answered = !self.stopping && row.as_ref().is_some_and(|r| r.status == "answered");
        let answers = row
            .as_ref()
            .filter(|_| answered)
            .and_then(|r| r.answers.clone())
            .map(normalize_answers)
            .unwrap_or_else(|| json!({}));
        let body = json!({ "answers": answers });
        let final_status = if answered {
            "answered"
        } else if self.stopping {
            "cancelled"
        } else {
            "expired"
        };
        let _ = AgentApproval::resolve_by_broker(&approval_id, final_status, Some(body.clone())).await;
        let note = match final_status {
            "answered" => "Technician answered.",
            "cancelled" => "The technician stopped the agent before answering.",
            _ => "No answer from a technician.",
        };
        self.marker("approval", note, None).await;
        self.respond(&request_id, body).await;
    }

    /// Polls the approval row until a technician decides, the deadline passes, or a stop arrives.
    async fn wait_for_decision(
        &mut self,
        approval_id: &RecordId,
        rx: &mut mpsc::Receiver<RunnerCmd>,
    ) -> Option<AgentApproval> {
        let deadline =
            tokio::time::Instant::now() + Duration::from_secs(self.cfg.approval_ttl_secs + 15);
        let mut poll = tokio::time::interval(APPROVAL_POLL);
        poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        poll.tick().await;
        loop {
            tokio::select! {
                _ = poll.tick() => {}
                cmd = rx.recv() => self.side_cmd(cmd).await,
            }
            if self.stopping {
                return None;
            }
            self.flush_due().await;
            let at_deadline = tokio::time::Instant::now() >= deadline;
            if at_deadline {
                let _ = AgentApproval::expire_stale().await;
            }
            match AgentApproval::fetch(approval_id).await {
                Ok(Some(row)) => match self.verified(row, at_deadline).await {
                    Step::Done(row) => return row,
                    Step::Wait => {}
                },
                // A vanished row is a refusal, never a permission.
                Ok(None) => return None,
                Err(e) if at_deadline => {
                    log::warn!("codex: approval poll failed at the deadline: {e}");
                    return None;
                }
                Err(e) => log::warn!("codex: approval poll failed: {e}"),
            }
        }
    }

    /// Checks who decided `row`; a decision by anyone but the thread's assignee or an active Root never runs.
    async fn verified(&mut self, row: AgentApproval, at_deadline: bool) -> Step {
        let allowed = if row.is_human_decision() {
            match AgentApproval::decider_allowed(row.decided_by.as_ref(), self.thread.assignee.as_ref()).await {
                Ok(allowed) => Some(allowed),
                Err(e) => {
                    log::warn!("codex: decider check failed for {}: {e}", row.id.key_string());
                    None
                }
            }
        } else {
            None
        };
        match verdict(&row, allowed, at_deadline) {
            Verdict::Accept => Step::Done(Some(row)),
            Verdict::Expire => Step::Done(None),
            Verdict::Wait => Step::Wait,
            Verdict::Reopen => {
                log::warn!(
                    "codex: {} was {} by {:?}, who is neither the assignee nor an active Root; reopening",
                    row.id.key_string(),
                    row.status,
                    row.decided_by.as_ref().map(RecordIdExt::key_string)
                );
                match AgentApproval::reopen(&row.id, &row.status, row.decided_by.as_ref()).await {
                    Ok(reopened) => {
                        if reopened {
                            self.marker(
                                "approval",
                                "Ignored a decision from someone who is not this session's technician or a Root user.",
                                None,
                            )
                            .await;
                        }
                        Step::Wait
                    }
                    Err(e) => {
                        log::warn!("codex: could not reopen {}: {e}", row.id.key_string());
                        Step::Done(None)
                    }
                }
            }
        }
    }

    /// Sends one text input as a new turn, or into the running one for `steer`.
    async fn send_text(&mut self, kind: &str, text: &str) -> anyhow::Result<()> {
        let Some(thread_id) = self.codex_thread_id.clone() else {
            anyhow::bail!("no codex thread yet");
        };
        let input = vec![json!({ "type": "text", "text": text })];
        self.send_inputs(&thread_id, kind, input)
            .await
            .map_err(anyhow::Error::msg)
    }

    /// `turn/steer` into a running turn, else `turn/start`, which marks the thread running.
    async fn send_inputs(
        &mut self,
        thread_id: &str,
        kind: &str,
        input: Vec<Value>,
    ) -> Result<(), String> {
        let steer = kind == "steer" && !self.busy.is_idle();
        let sent = if steer {
            self.client.turn_steer_with_inputs(thread_id, input).await
        } else {
            self.client.turn_start_with_inputs(thread_id, input).await
        };
        sent.map_err(|e| e.to_string())?;
        if self.busy.is_idle() {
            self.signal(Signal::TurnStarted).await;
        }
        Ok(())
    }

    /// Stages the turn's pictures and sends it with its text.
    async fn deliver(&mut self, turn: &AgentTurn, kind: &str) -> Result<(), String> {
        let thread_id = self
            .codex_thread_id
            .clone()
            .ok_or_else(|| "no codex thread yet".to_string())?;
        let mut input = Vec::new();
        if !turn.images.is_empty() {
            let (items, staged) = self.stage_images(&turn.images).await?;
            input = items;
            if let Err(e) = AgentTurn::record_staged(&turn.id, &staged).await {
                log::warn!(
                    "codex: could not trim the staged pictures of {}: {e}",
                    turn.id.key_string()
                );
            }
        }
        if !turn.text.trim().is_empty() || input.is_empty() {
            input.push(json!({ "type": "text", "text": turn.text }));
        }
        self.send_inputs(&thread_id, kind, input).await
    }

    /// The daemon's staging directory from its hello, else the default.
    async fn upload_dir(&mut self) -> String {
        if let Some(dir) = &self.upload_dir {
            return dir.clone();
        }
        let reported = match self
            .client
            .request_with_timeout("daemon/hello", json!({}), READ_TIMEOUT)
            .await
        {
            Ok(hello) => hello
                .get("uploadDir")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|d| !d.is_empty())
                .map(str::to_string),
            Err(e) => {
                log::debug!("codex: daemon/hello failed: {e}");
                None
            }
        };
        let dir = reported.unwrap_or_else(|| DEFAULT_UPLOAD_DIR.to_string());
        self.upload_dir = Some(dir.clone());
        dir
    }

    /// Writes each picture into the upload directory; returns the turn's image inputs and the trimmed rows.
    async fn stage_images(
        &mut self,
        images: &[TurnImage],
    ) -> Result<(Vec<Value>, Vec<TurnImage>), String> {
        let dir = self.upload_dir().await;
        let mut inputs = Vec::with_capacity(images.len());
        let mut staged = Vec::with_capacity(images.len());
        for image in images {
            let path = match (&image.data, &image.path) {
                (Some(data), _) => self.stage_one(&dir, image, data).await?,
                (None, Some(path)) => path.clone(),
                (None, None) => return Err(format!("{} carries no picture data", image.name)),
            };
            inputs.push(json!({ "type": "localImage", "path": path }));
            staged.push(image.staged(path));
        }
        Ok((inputs, staged))
    }

    async fn stage_one(&self, dir: &str, image: &TurnImage, data: &str) -> Result<String, String> {
        let data = data.trim();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|e| format!("{} is not valid base64: {e}", image.name))?;
        if bytes.is_empty() {
            return Err(format!("{} is empty", image.name));
        }
        if bytes.len() > IMAGE_MAX_BYTES {
            return Err(format!(
                "{} is {} KB; the limit is {} KB",
                image.name,
                bytes.len() / 1024,
                IMAGE_MAX_BYTES / 1024
            ));
        }
        let path = format!(
            "{}/{}",
            dir.trim_end_matches('/'),
            upload_name(&image.name, &bytes)
        );
        self.client
            .request("fs/writeFile", json!({ "path": path, "dataBase64": data }))
            .await
            .map_err(|e| format!("could not stage {}: {e}", image.name))?;
        Ok(path)
    }

    /// Holds the queue while it has anything in it, and records the hold on its rows.
    async fn hold_queue(&mut self, why: &str) {
        if self.queue.hold(why) {
            self.write_hold().await;
        }
    }

    async fn write_hold(&mut self) {
        if let Err(e) = AgentTurn::hold_queue(&self.thread.id).await {
            log::warn!(
                "codex: could not hold the queue of {}: {e}",
                self.thread.id.key_string()
            );
        }
        let why = self.queue.held().unwrap_or_default().to_string();
        let waiting = self.queue.len();
        self.marker(
            "other",
            &format!("Queue held with {waiting} waiting: {why}"),
            None,
        )
        .await;
    }

    /// Queues a message behind the running turn; one with no text or pictures resumes a held queue.
    async fn enqueue(&mut self, turn: AgentTurn) {
        if turn.text.trim().is_empty() && turn.images.is_empty() {
            let _ = AgentTurn::take_queued(&turn.id).await;
            if self.queue.resume() {
                if let Err(e) = AgentTurn::release_queue(&self.thread.id).await {
                    log::warn!(
                        "codex: could not release the queue of {}: {e}",
                        self.thread.id.key_string()
                    );
                }
                self.marker("other", "Queue resumed.", None).await;
            }
        } else if self.queue.push(turn)
            && self.queue.held().is_some()
            && let Err(e) = AgentTurn::hold_queue(&self.thread.id).await
        {
            log::warn!(
                "codex: could not hold the queue of {}: {e}",
                self.thread.id.key_string()
            );
        }
        self.pump().await;
    }

    /// While the thread is idle, sends a deferred compaction or the oldest queued message.
    async fn pump(&mut self) {
        if !self.busy.is_idle() {
            return;
        }
        if let Some(turn) = self.pending_compact.take() {
            self.compact(&turn).await;
            return;
        }
        while self.busy.is_idle() {
            let Some(turn) = self.queue.next() else {
                return;
            };
            match AgentTurn::take_queued(&turn.id).await {
                Ok(true) => {}
                Ok(false) => continue,
                Err(e) => {
                    log::warn!(
                        "codex: could not take queued turn {}: {e}",
                        turn.id.key_string()
                    );
                    self.queue.undelivered(turn, "the database did not answer");
                    self.write_hold().await;
                    return;
                }
            }
            if let Err(e) = self.deliver(&turn, "start").await {
                let _ = AgentTurn::mark_failed(&turn.id, &e).await;
                self.marker(
                    "error",
                    &format!("Could not send a queued message: {e}"),
                    None,
                )
                .await;
                self.hold_queue("A queued message could not be sent.").await;
                return;
            }
        }
    }

    async fn request_compact(&mut self, turn: AgentTurn) {
        if self.busy.is_idle() {
            self.compact(&turn).await;
            return;
        }
        if let Some(earlier) = self.pending_compact.replace(turn) {
            let _ =
                AgentTurn::mark_failed(&earlier.id, "a later compaction request replaced it").await;
        }
        self.marker(
            "other",
            "Compaction requested; it starts when this turn ends.",
            None,
        )
        .await;
    }

    async fn compact(&mut self, turn: &AgentTurn) {
        let Some(thread_id) = self.codex_thread_id.clone() else {
            let _ = AgentTurn::mark_failed(&turn.id, "no codex thread yet").await;
            return;
        };
        match self.client.compact(&thread_id).await {
            Ok(_) => {
                self.compact_unstarted = true;
                self.signal(Signal::Working(AgentActivity::Compacting))
                    .await;
                self.marker("other", "Compaction requested by the technician.", None)
                    .await;
            }
            Err(e) => {
                let _ = AgentTurn::mark_failed(&turn.id, &e.to_string()).await;
                self.marker("error", &format!("Could not start compaction: {e}"), None)
                    .await;
            }
        }
    }

    /// Stops the running turn, holds the queue and drops a deferred compaction.
    async fn interrupt(&mut self, turn: &AgentTurn) {
        self.stop_waits("the technician stopped the agent");
        self.hold_queue(queue::STOPPED).await;
        if let Some(compact) = self.pending_compact.take() {
            let _ =
                AgentTurn::mark_failed(&compact.id, "stopped before the compaction started").await;
        }
        if self.codex_thread_id.is_none() {
            let _ = AgentTurn::mark_failed(&turn.id, "no codex thread yet").await;
            return;
        }
        if self.busy.is_idle() {
            if self.turn_in_progress().await != Some(true) {
                self.row.forget_status();
                self.set_status("idle").await;
                self.marker("other", "Nothing was running; the session is idle.", None)
                    .await;
                return;
            }
            self.signal(Signal::Attached { in_progress: true }).await;
        }
        if self.stopping {
            return;
        }
        self.begin_stop(Some(turn.id.clone()));
        self.marker("other", "Technician stopped the agent.", None)
            .await;
    }

    /// Sends `turn/interrupt` from its own task, which reports back as [`RunnerCmd::Stopped`].
    fn begin_stop(&mut self, turn: Option<RecordId>) {
        let Some(thread_id) = self.codex_thread_id.clone() else {
            return;
        };
        self.stopping = true;
        let (client, own) = (self.client.clone(), self.own.clone());
        tokio::spawn(async move {
            let result =
                match tokio::time::timeout(STOP_TIMEOUT, client.turn_interrupt(&thread_id)).await {
                    Ok(Ok(_)) => Ok(()),
                    Ok(Err(e)) => Err(e.to_string()),
                    Err(_) => Err(format!("no answer within {} s", STOP_TIMEOUT.as_secs())),
                };
            let _ = own.send(RunnerCmd::Stopped { turn, result }).await;
        });
    }

    /// A refused stop: an idle server means nothing ran, otherwise the agent keeps going.
    async fn stop_answered(&mut self, turn: Option<&RecordId>, result: Result<(), String>) {
        let Err(e) = result else { return };
        if let Some(id) = turn {
            let _ = AgentTurn::mark_failed(id, &e).await;
        }
        self.stopping = false;
        if self.turn_in_progress().await == Some(false) {
            self.signal(Signal::TurnEnded).await;
            self.marker("other", "Nothing was running; the session is idle.", None)
                .await;
            self.pump().await;
        } else {
            self.marker("error", &format!("Stop did not reach the agent: {e}"), None)
                .await;
        }
    }

    async fn on_turn(&mut self, turn: AgentTurn) -> Flow {
        match turn.kind.as_str() {
            "start" | "steer" => {
                self.stop_waits("the technician sent a message");
                if let Err(e) = self.deliver(&turn, &turn.kind).await {
                    log::warn!("codex: turn {} failed: {e}", turn.id.key_string());
                    let _ = AgentTurn::mark_failed(&turn.id, &e).await;
                    self.marker("error", &format!("Could not deliver the technician's message: {e}"), None).await;
                }
                Flow::Continue
            }
            "queue" => {
                self.enqueue(turn).await;
                Flow::Continue
            }
            "compact" => {
                self.request_compact(turn).await;
                Flow::Continue
            }
            "interrupt" => {
                self.interrupt(&turn).await;
                Flow::Continue
            }
            "rename" => {
                super::turns::rename(&self.cfg, &turn).await;
                Flow::Continue
            }
            "approvals" => {
                self.approvals(&turn).await;
                Flow::Continue
            }
            "close" => {
                self.stop_waits("the session was closed");
                if !self.busy.is_idle() && !self.stopping {
                    self.begin_stop(None);
                }
                self.remember_session().await;
                if let Err(e) =
                    AgentTurn::cancel_waiting(&self.thread.id, "the session closed").await
                {
                    log::warn!(
                        "codex: could not cancel the queue of {}: {e}",
                        self.thread.id.key_string()
                    );
                }
                if let Some(t) = self.codex_thread_id.clone() {
                    let _ = self.client.request("thread/unsubscribe", json!({ "threadId": t })).await;
                }
                self.marker("other", "Session closed by the technician.", None).await;
                self.row.set_activity(AgentActivity::Idle.to_db());
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

/// What a decided tool-call approval lets the broker do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Decision {
    Run { remember: bool, approve_all: bool },
    Cancelled,
    Declined,
    Expired,
}

fn decision(status: &str) -> Decision {
    match status {
        "accepted" => Decision::Run { remember: false, approve_all: false },
        "accepted_for_session" => Decision::Run { remember: true, approve_all: false },
        ACCEPTED_ALL_FOR_SESSION => Decision::Run { remember: false, approve_all: true },
        "cancelled" => Decision::Cancelled,
        "declined" => Decision::Declined,
        _ => Decision::Expired,
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
        _ => match item.get("type").and_then(Value::as_str) {
            Some("contextCompaction") => COMPACTED.to_string(),
            other => other.unwrap_or("").to_string(),
        },
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

    fn decided(status: &str) -> AgentApproval {
        AgentApproval {
            id: RecordId::new("agent_approval", "a"),
            thread: RecordId::new("agent_thread", "t"),
            kind: "tool_call".into(),
            method: String::new(),
            codex_request_id: String::new(),
            summary: String::new(),
            server: None,
            tool: Some("remote_exec_start".into()),
            arguments: None,
            params: None,
            questions: None,
            answers: None,
            response_sent: None,
            status: status.into(),
            assignee: Some(RecordId::new("user", "tech")),
            connection_string: None,
            store: None,
            requested_at: None,
            expires_at: None,
            decided_at: None,
            sent_to_codex_at: None,
            decided_by: None,
            deny_note: None,
        }
    }

    #[test]
    fn a_verified_decision_is_acted_on_before_and_at_the_deadline() {
        for status in ["accepted", "accepted_for_session", "accepted_all_for_session", "declined", "cancelled", "answered"] {
            assert_eq!(verdict(&decided(status), Some(true), false), Verdict::Accept, "{status}");
            assert_eq!(verdict(&decided(status), Some(true), true), Verdict::Accept, "{status}");
        }
    }

    #[test]
    fn a_decision_by_someone_else_is_reopened_and_never_run() {
        assert_eq!(verdict(&decided("accepted"), Some(false), false), Verdict::Reopen);
        assert_eq!(verdict(&decided("answered"), Some(false), false), Verdict::Reopen);
        assert_eq!(verdict(&decided("accepted_all_for_session"), Some(false), false), Verdict::Reopen);
    }

    #[test]
    fn at_the_deadline_an_unverified_decision_expires() {
        assert_eq!(verdict(&decided("accepted"), Some(false), true), Verdict::Expire);
        assert_eq!(verdict(&decided("accepted_for_session"), None, true), Verdict::Expire);
        assert_eq!(verdict(&decided("pending"), None, true), Verdict::Expire);
    }

    #[test]
    fn a_failed_check_waits_instead_of_running() {
        assert_eq!(verdict(&decided("accepted"), None, false), Verdict::Wait);
        assert_eq!(verdict(&decided("pending"), None, false), Verdict::Wait);
    }

    #[test]
    fn broker_statuses_pass_without_a_check() {
        for status in ["expired", "failed", "auto_declined", "auto_accepted", "resolved_elsewhere"] {
            assert_eq!(verdict(&decided(status), None, false), Verdict::Accept, "{status}");
            assert_eq!(verdict(&decided(status), None, true), Verdict::Accept, "{status}");
        }
    }

    #[test]
    fn decisions_map_to_what_the_broker_does() {
        assert_eq!(decision("accepted"), Decision::Run { remember: false, approve_all: false });
        assert_eq!(decision("accepted_for_session"), Decision::Run { remember: true, approve_all: false });
        assert_eq!(decision("accepted_all_for_session"), Decision::Run { remember: false, approve_all: true });
        assert_eq!(decision("cancelled"), Decision::Cancelled);
        assert_eq!(decision("declined"), Decision::Declined);
        for status in ["expired", "failed", "pending", "", "ACCEPTED"] {
            assert_eq!(decision(status), Decision::Expired, "{status}");
        }
    }

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
    fn a_compaction_reads_as_a_sentence() {
        assert_eq!(
            item_text(
                kind_for("contextCompaction"),
                &json!({ "type": "contextCompaction", "id": "c" })
            ),
            COMPACTED
        );
        assert_eq!(item_text("other", &json!({ "type": "plan" })), "plan");
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
