#![cfg(not(target_arch = "wasm32"))]
//! Multi-turn headless Claude Code chat session shared by the egui AI playground
//! and the terminal-mode Assistant tab. Spawns `claude` (subscription auth) with
//! the Mastertech MCP on :9004, parses stream-json events into `ChatMessage`s,
//! and supports resume, cancel, per-event/session timeouts, and audit logging.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use crossbeam::channel::Sender;

use crate::tabs::ai_playground::{ChatMessage, ChatMessageType, SentFrom};

/// Prefix for tool-activity lines so UIs can style them and history filters can skip them.
pub const TOOL_PREFIX: &str = "\u{00BB} "; // "» "

const MCP_CONFIG: &str =
    r#"{ "mcpServers": { "mastertech": { "type": "http", "url": "http://127.0.0.1:9004/mcp" } } }"#;

// Reads + diagnostic-session writes only; machine-touching tools omitted until an approval gate exists.
const DEFAULT_ALLOWED_TOOLS: &str = "ToolSearch,mcp__mastertech__query_surrealdb,mcp__mastertech__search_diagnostics,\
mcp__mastertech__get_diagnostic_session,mcp__mastertech__get_computer_details,\
mcp__mastertech__search_service_orders,mcp__mastertech__create_diagnostic_session,\
mcp__mastertech__log_diagnostic_entry,mcp__mastertech__search_customers,\
mcp__mastertech__get_customer_details,mcp__mastertech__telemetry_snapshot,\
mcp__mastertech__egui_inspect_status,mcp__mastertech__egui_inspect_tree,\
mcp__mastertech__egui_inspect_screenshot,mcp__mastertech__crash_intel_search,\
mcp__mastertech__crash_intel_signature,mcp__mastertech__crash_verdict_record,\
mcp__mastertech__known_bad_driver_list,mcp__mastertech__driver_snapshots_list,\
mcp__mastertech__driver_snapshot_diff";

const SYSTEM_PROMPT: &str = "You are the Mastertech diagnostic assistant. A PC Laptops technician is \
chatting with you from inside the Mastertech app. Use the mastertech MCP tools for live data instead \
of guessing. Log significant findings with log_diagnostic_entry when a diagnostic session exists. \
Keep answers concise.";

fn env_secs(key: &str, default: u64) -> Duration {
    Duration::from_secs(
        std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default),
    )
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn audit(session: &str, kind: &str, detail: &str) {
    let detail: String = detail.chars().take(400).collect();
    log::info!(target: "cc_audit", "[cc-audit] session={session} {kind}: {detail}");
}

/// Kills the child on scope exit so an early return never leaks a `claude` process.
struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn spawn_claude(args: &[String]) -> std::io::Result<Child> {
    let candidates: &[&str] = if cfg!(windows) {
        &["claude.cmd", "claude.exe", "claude"]
    } else {
        &["claude"]
    };
    let mut last_err = None;
    for bin in candidates {
        let mut cmd = Command::new(bin);
        cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW: no console flash from a GUI process.
            cmd.creation_flags(0x0800_0000);
        }
        match cmd.spawn() {
            Ok(child) => return Ok(child),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "claude not found")))
}

/// One conversational Claude Code session. `send` runs one turn on a background
/// thread; later turns resume the same CLI session so context carries over.
#[derive(Clone, Default)]
pub struct ClaudeCodeSession {
    cancel: Arc<AtomicBool>,
    busy: Arc<AtomicBool>,
    session_id: Arc<Mutex<Option<String>>>,
}

impl ClaudeCodeSession {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::Acquire)
    }

    pub fn session_id(&self) -> Option<String> {
        self.session_id.lock().ok().and_then(|g| g.clone())
    }

    /// Requests the running turn stop; the pump kills the child within ~200ms.
    pub fn cancel(&self) {
        if self.is_busy() {
            self.cancel.store(true, Ordering::Release);
        }
    }

    /// Cancels any running turn and forgets the CLI session (next send starts fresh).
    pub fn reset(&self) {
        self.cancel();
        if let Ok(mut sid) = self.session_id.lock() {
            *sid = None;
        }
    }

    /// Runs one turn. Emits streamed `Text`/`Reasoning`, `»`-prefixed tool lines,
    /// `Error`s, and a terminal `Done` over `tx`.
    pub fn send(
        &self,
        prompt: String,
        connection_string: Option<String>,
        thread_id: String,
        tx: Sender<ChatMessage>,
    ) {
        if self.busy.swap(true, Ordering::AcqRel) {
            emit(&tx, &thread_id, ChatMessageType::Error("Claude Code is still working — Stop it first.".into()));
            return;
        }
        self.cancel.store(false, Ordering::Release);

        let busy = self.busy.clone();
        let cancel = self.cancel.clone();
        let session_slot = self.session_id.clone();
        std::thread::spawn(move || {
            run_turn(prompt, connection_string, thread_id.clone(), &tx, &cancel, &session_slot);
            busy.store(false, Ordering::Release);
            emit(&tx, &thread_id, ChatMessageType::Done);
        });
    }
}

fn emit(tx: &Sender<ChatMessage>, thread_id: &str, content: ChatMessageType) {
    emit_id(tx, thread_id, new_id(), content);
}

fn emit_id(tx: &Sender<ChatMessage>, thread_id: &str, id: String, content: ChatMessageType) {
    let _ = tx.try_send(ChatMessage {
        id,
        thread_id: thread_id.to_string(),
        ts: 0,
        from: SentFrom::Gpt,
        content,
    });
}

fn run_turn(
    prompt: String,
    connection_string: Option<String>,
    thread_id: String,
    tx: &Sender<ChatMessage>,
    cancel: &AtomicBool,
    session_slot: &Mutex<Option<String>>,
) {
    let full_prompt = match &connection_string {
        Some(cs) => format!(
            "Target client connection_string = {cs}. The remote mastertech tools \
             (call_remote_plugin_tool / scripts_run_remote) act on that client. {prompt}"
        ),
        None => prompt,
    };

    let cfg_path = std::env::temp_dir().join("mtech-claude-mcp.json");
    if let Err(e) = std::fs::write(&cfg_path, MCP_CONFIG) {
        emit(tx, &thread_id, ChatMessageType::Error(format!("Claude Code: MCP config write failed: {e}")));
        return;
    }

    let allowed = std::env::var("CC_ALLOWED_TOOLS").unwrap_or_else(|_| DEFAULT_ALLOWED_TOOLS.to_string());
    let perm_mode = std::env::var("CC_PERMISSION_MODE").unwrap_or_else(|_| "default".to_string());
    let event_timeout = env_secs("CC_EVENT_TIMEOUT_SECS", 60);
    let session_timeout = env_secs("CC_SESSION_TIMEOUT_SECS", 900);

    let resume_id = session_slot.lock().ok().and_then(|g| g.clone());
    let mut args: Vec<String> = vec![
        "-p".into(), full_prompt.clone(),
        "--output-format".into(), "stream-json".into(),
        "--verbose".into(),
        "--include-partial-messages".into(),
        "--mcp-config".into(), cfg_path.to_string_lossy().into_owned(),
        "--strict-mcp-config".into(),
        "--allowedTools".into(), allowed,
        "--permission-mode".into(), perm_mode,
        "--append-system-prompt".into(), SYSTEM_PROMPT.into(),
    ];
    if let Some(sid) = &resume_id {
        args.push("--resume".into());
        args.push(sid.clone());
    }

    audit(resume_id.as_deref().unwrap_or("new"), "prompt", &full_prompt);

    let mut child = match spawn_claude(&args) {
        Ok(c) => c,
        Err(e) => {
            emit(tx, &thread_id, ChatMessageType::Error(format!(
                "Claude Code (`claude`) not found or failed to start: {e}. Install it and run `claude login` on this machine."
            )));
            return;
        }
    };

    // Drain stderr concurrently so a full pipe can't deadlock the child.
    let stderr = child.stderr.take();
    let stderr_handle = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(err) = stderr {
            let _ = BufReader::new(err).read_to_string(&mut s);
        }
        s
    });

    let stdout = child.stdout.take().expect("claude stdout piped");
    let (line_tx, line_rx) = mpsc::channel::<String>();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            match line {
                Ok(l) => {
                    if line_tx.send(l).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let mut guard = ChildGuard(child);
    let t0 = Instant::now();
    let mut last_event = Instant::now();
    let mut abort: Option<String> = None;
    let mut state = TurnState::default();

    loop {
        if cancel.load(Ordering::Acquire) {
            audit(state.session_id.as_deref().unwrap_or("?"), "cancel", "stopped by user");
            abort = Some("Stopped by user.".into());
            break;
        }
        if t0.elapsed() > session_timeout {
            abort = Some(format!("Session exceeded {}s — stopped.", session_timeout.as_secs()));
            break;
        }
        if last_event.elapsed() > event_timeout {
            abort = Some(format!(
                "No output for {}s — MCP tool stalled (:9004 session?). Stopped.",
                event_timeout.as_secs()
            ));
            break;
        }
        match line_rx.recv_timeout(Duration::from_millis(200)) {
            Ok(line) => {
                last_event = Instant::now();
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
                if handle_event(&v, &thread_id, tx, session_slot, &mut state) {
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    if let Some(reason) = abort {
        let _ = guard.0.kill();
        emit(tx, &thread_id, ChatMessageType::Error(format!("Claude Code: {reason}")));
    }
    let _ = guard.0.wait();
    let _ = reader.join();
    if let Ok(errs) = stderr_handle.join() {
        let errs = errs.trim();
        if !errs.is_empty() {
            log::warn!("claude stderr: {errs}");
        }
    }
}

#[derive(Default)]
struct TurnState {
    session_id: Option<String>,
    text_id: Option<String>,
    think_id: Option<String>,
    /// Content-block index -> (message id, tool name, accumulated args json).
    open_tools: HashMap<u64, (String, String, String)>,
    /// Tool message ids awaiting a tool_result, in call order.
    pending_results: Vec<(String, String)>,
}

/// Maps one stream-json event to chat messages. Returns true when the turn is over.
fn handle_event(
    v: &serde_json::Value,
    thread_id: &str,
    tx: &Sender<ChatMessage>,
    session_slot: &Mutex<Option<String>>,
    state: &mut TurnState,
) -> bool {
    match v["type"].as_str().unwrap_or("") {
        "system" => {
            if v["subtype"].as_str() == Some("init") {
                if let Some(sid) = v["session_id"].as_str() {
                    state.session_id = Some(sid.to_string());
                    if let Ok(mut slot) = session_slot.lock() {
                        *slot = Some(sid.to_string());
                    }
                    audit(sid, "init", &format!("tools={}", v["tools"].as_array().map(|a| a.len()).unwrap_or(0)));
                }
            }
            false
        }
        "stream_event" => {
            let ev = &v["event"];
            match ev["type"].as_str().unwrap_or("") {
                "content_block_delta" => {
                    let idx = ev["index"].as_u64().unwrap_or(0);
                    if let Some(t) = ev["delta"]["text"].as_str() {
                        let id = state.text_id.get_or_insert_with(new_id).clone();
                        emit_id(tx, thread_id, id, ChatMessageType::Text(t.to_string()));
                    } else if let Some(t) = ev["delta"]["thinking"].as_str() {
                        let id = state.think_id.get_or_insert_with(new_id).clone();
                        emit_id(tx, thread_id, id, ChatMessageType::Reasoning(t.to_string()));
                    } else if let Some(j) = ev["delta"]["partial_json"].as_str() {
                        if let Some(entry) = state.open_tools.get_mut(&idx) {
                            entry.2.push_str(j);
                        }
                    }
                }
                "content_block_start" => {
                    if ev["content_block"]["type"].as_str() == Some("tool_use") {
                        let idx = ev["index"].as_u64().unwrap_or(0);
                        let name = ev["content_block"]["name"].as_str().unwrap_or("tool");
                        let pretty = name.strip_prefix("mcp__mastertech__").unwrap_or(name).to_string();
                        let id = new_id();
                        emit_id(tx, thread_id, id.clone(), ChatMessageType::Text(format!("{TOOL_PREFIX}{pretty}")));
                        state.open_tools.insert(idx, (id, pretty, String::new()));
                        state.text_id = None;
                        state.think_id = None;
                    }
                }
                "content_block_stop" => {
                    let idx = ev["index"].as_u64().unwrap_or(0);
                    if let Some((id, name, args)) = state.open_tools.remove(&idx) {
                        let summary = summarize_args(&args);
                        if !summary.is_empty() {
                            emit_id(tx, thread_id, id.clone(), ChatMessageType::Text(format!(" {summary}")));
                        }
                        audit(state.session_id.as_deref().unwrap_or("?"), "tool", &format!("{name} {args}"));
                        state.pending_results.push((id, name));
                    }
                }
                _ => {}
            }
            false
        }
        // Tool results return as a user turn; surface errors instead of masking them.
        "user" => {
            if let Some(content) = v["message"]["content"].as_array() {
                for b in content {
                    if b["type"].as_str() != Some("tool_result") {
                        continue;
                    }
                    let is_err = b["is_error"].as_bool() == Some(true);
                    let snippet = result_snippet(b);
                    let (tool_id, name) = if state.pending_results.is_empty() {
                        (None, "tool".to_string())
                    } else {
                        let (id, name) = state.pending_results.remove(0);
                        (Some(id), name)
                    };
                    audit(
                        state.session_id.as_deref().unwrap_or("?"),
                        if is_err { "tool-error" } else { "tool-ok" },
                        &format!("{name}: {snippet}"),
                    );
                    if is_err {
                        emit(tx, thread_id, ChatMessageType::Error(format!("{name} failed: {snippet}")));
                    } else if let Some(id) = tool_id {
                        emit_id(tx, thread_id, id, ChatMessageType::Text(" \u{2192} ok".to_string()));
                    }
                }
            }
            state.text_id = None;
            state.think_id = None;
            false
        }
        "result" => {
            if let Some(sid) = v["session_id"].as_str() {
                if let Ok(mut slot) = session_slot.lock() {
                    *slot = Some(sid.to_string());
                }
            }
            let is_err = v["is_error"].as_bool().unwrap_or(false) || v["subtype"].as_str() == Some("error");
            if is_err {
                let msg = v["error"]["message"]
                    .as_str()
                    .or_else(|| v["result"].as_str())
                    .unwrap_or("Claude Code error");
                emit(tx, thread_id, ChatMessageType::Error(format!("Claude Code: {msg}")));
            }
            audit(
                v["session_id"].as_str().unwrap_or("?"),
                "result",
                &format!("subtype={} is_error={is_err}", v["subtype"].as_str().unwrap_or("success")),
            );
            true
        }
        _ => false,
    }
}

fn summarize_args(args_json: &str) -> String {
    let trimmed = args_json.trim();
    if trimmed.is_empty() || trimmed == "{}" {
        return String::new();
    }
    if trimmed.chars().count() <= 80 {
        format!("({trimmed})")
    } else {
        format!("({}\u{2026})", trimmed.chars().take(80).collect::<String>())
    }
}

fn result_snippet(block: &serde_json::Value) -> String {
    let raw = if let Some(s) = block["content"].as_str() {
        s.to_string()
    } else if let Some(arr) = block["content"].as_array() {
        arr.iter().filter_map(|x| x["text"].as_str()).collect::<Vec<_>>().join(" ")
    } else {
        String::new()
    };
    raw.chars().take(160).collect::<String>().replace('\n', " ")
}
