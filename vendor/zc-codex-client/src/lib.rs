//! Presenter-agnostic client for the Codex app-server control plane.
//!
//! The protocol itself is documented in `docs/WIRE-GUIDE.md`; this crate turns it into
//! something a UI can consume. Two things it does that a raw JSON-RPC client does not:
//!
//! 1. **Demultiplexes the three inbound message shapes.** Codex sends responses,
//!    notifications, AND requests addressed to us. Approvals arrive as the third kind, so
//!    a client that only checks for `method` treats them as notifications, never answers,
//!    and every gated turn hangs forever. [`Event`] keeps them separate by construction.
//!
//! 2. **Coalesces token deltas.** One short sentence emits ~30 `item/agentMessage/delta`
//!    notifications and a real turn emits thousands. Every chat surface worth targeting
//!    rate-limits edits (Discord allows a handful per five seconds per channel), so the
//!    raw stream is unusable as a render trigger. [`Coalescer`] buffers per item and
//!    flushes on a timer, with an immediate flush when the item completes so the last
//!    words are never left sitting in a buffer.
//!
//! Deliberately knows nothing about Discord. The presenter consumes [`Event`] and calls
//! [`Client::respond`]; swapping Discord for a mobile app replaces only that layer.


use anyhow::{anyhow, Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, Mutex};

/// Something the server sent us that a presenter may care about.
#[derive(Debug, Clone)]
pub enum Event {
    /// Assistant text. Already coalesced; `final_chunk` marks the authoritative last one.
    Text {
        thread_id: String,
        item_id: String,
        text: String,
        final_chunk: bool,
    },
    /// Reasoning text, coalesced the same way. Separate from [`Event::Text`] so a presenter
    /// can style it apart from the reply or leave it out.
    Reasoning {
        thread_id: String,
        item_id: String,
        text: String,
        final_chunk: bool,
    },
    /// Live output from a running command, coalesced the same way.
    CommandOutput {
        thread_id: String,
        item_id: String,
        text: String,
        final_chunk: bool,
    },
    /// A thread item started or finished. `item` is the authoritative form on completion.
    Item {
        thread_id: String,
        item_type: String,
        completed: bool,
        item: Value,
    },
    /// The server is asking US something and the turn is blocked until we answer.
    /// Reply with [`Client::respond`] (or [`Client::respond_error`]) using `request_id`.
    Ask {
        /// Empty when the request carries no `threadId`.
        thread_id: String,
        request_id: Value,
        method: String,
        params: Value,
    },
    /// An `Ask` was settled — possibly by a different connected client. Retract any
    /// prompt still on screen for this id.
    AskResolved { request_id: Value },
    /// Turn boundaries.
    TurnStarted { thread_id: String },
    TurnCompleted { thread_id: String },
    /// Turn-level failure. NOT a JSON-RPC error; `will_retry` means it is informational.
    Error {
        thread_id: String,
        message: String,
        will_retry: bool,
    },
    /// Context window accounting, for offering compaction at the right moment.
    TokenUsage {
        thread_id: String,
        used: Option<u64>,
        window: Option<u64>,
    },
    /// Anything we do not model. Never dropped: new event types ship constantly and a
    /// presenter should be able to log them rather than have the bridge swallow them.
    Other { method: String, params: Value },
}

/// Buffers text fragments per item and releases them on a timer.
///
/// `flush_after` is the maximum time a fragment waits. 750ms-1.5s suits chat surfaces:
/// long enough to collapse thousands of deltas into a handful of edits, short enough that
/// the output still reads as live.
pub struct Coalescer {
    buf: HashMap<String, String>,
    flush_after: Duration,
    last_flush: std::time::Instant,
}

impl Coalescer {
    pub fn new(flush_after: Duration) -> Self {
        Self {
            buf: HashMap::new(),
            flush_after,
            last_flush: std::time::Instant::now(),
        }
    }

    /// Add a fragment. Returns the accumulated text if the timer says release it now.
    pub fn push(&mut self, item_id: &str, delta: &str) -> Option<String> {
        self.buf.entry(item_id.to_string()).or_default().push_str(delta);
        if self.last_flush.elapsed() >= self.flush_after {
            self.last_flush = std::time::Instant::now();
            self.take(item_id)
        } else {
            None
        }
    }

    /// Release whatever is buffered for one item, ignoring the timer. Call this on
    /// `item/completed` and `turn/completed`.
    pub fn take(&mut self, item_id: &str) -> Option<String> {
        self.buf.remove(item_id).filter(|s| !s.is_empty())
    }

    pub fn drain(&mut self) -> Vec<(String, String)> {
        self.buf.drain().filter(|(_, v)| !v.is_empty()).collect()
    }
}

/// Ceiling on any single request; thread/start may wait on MCP server startup.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

type Pending = Arc<Mutex<HashMap<String, oneshot::Sender<Result<Value, Value>>>>>;

/// A connected app-server session. Cheap to clone; all clones share one socket.
#[derive(Clone)]
pub struct Client {
    out: mpsc::Sender<String>,
    pending: Pending,
    next_id: Arc<AtomicI64>,
    active_turns: Arc<Mutex<HashMap<String, String>>>,
}

impl Client {
    /// Connect, spawn the read pump, and complete the `initialize` handshake.
    ///
    /// `experimentalApi` is always requested: `thread/queue/*`, `remoteControl/*` and
    /// `collaborationMode/*` return `-32600 requires experimentalApi capability` without
    /// it, and the capability is negotiated only here — there is no way to opt in later.
    pub async fn connect(url: &str, client_name: &str) -> Result<(Self, mpsc::Receiver<Event>)> {
        Self::connect_with_token(url, client_name, None).await
    }

    /// Connect through `zc-codexd`, which requires a bearer token on the handshake.
    ///
    /// The request is built by hand rather than handed to `connect_async` as a string because
    /// the daemon rejects any handshake carrying an `Origin`, and several WebSocket clients add
    /// one by default. Building the request explicitly means only the headers set here are sent.
    pub async fn connect_with_token(
        url: &str,
        client_name: &str,
        token: Option<&str>,
    ) -> Result<(Self, mpsc::Receiver<Event>)> {
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;
        let mut req = url
            .into_client_request()
            .with_context(|| format!("{url} is not a valid websocket url"))?;
        if let Some(t) = token {
            req.headers_mut().insert(
                "authorization",
                format!("Bearer {t}")
                    .parse()
                    .map_err(|_| anyhow!("token is not a valid header value"))?,
            );
        }
        let (ws, _) = tokio_tungstenite::connect_async(req)
            .await
            .with_context(|| format!("connecting to {url}"))?;
        let (mut sink, mut stream) = ws.split();

        let (out_tx, mut out_rx) = mpsc::channel::<String>(256);
        let (ev_tx, ev_rx) = mpsc::channel::<Event>(1024);
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let active_turns = Arc::new(Mutex::new(HashMap::new()));

        tokio::spawn(async move {
            // Pinged on a timer as well as written to, so an idle session survives the phone being
            // in another app; nothing here otherwise writes between turns.
            let mut beat = tokio::time::interval(std::time::Duration::from_secs(20));
            beat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                let msg = tokio::select! {
                    m = out_rx.recv() => match m {
                        Some(m) => tokio_tungstenite::tungstenite::Message::Text(m.into()),
                        None => break,
                    },
                    _ = beat.tick() => {
                        tokio_tungstenite::tungstenite::Message::Ping(Vec::new().into())
                    }
                };
                if sink.send(msg).await.is_err() {
                    break;
                }
            }
        });

        let pend = pending.clone();
        let evt = ev_tx.clone();
        let turns = active_turns.clone();
        tokio::spawn(async move {
            let mut coalesce_text = Coalescer::new(Duration::from_millis(900));
            let mut coalesce_cmd = Coalescer::new(Duration::from_millis(900));
            let mut coalesce_reason = Coalescer::new(Duration::from_millis(900));
            while let Some(Ok(msg)) = stream.next().await {
                let txt = match msg {
                    tokio_tungstenite::tungstenite::Message::Text(t) => t.to_string(),
                    tokio_tungstenite::tungstenite::Message::Close(_) => break,
                    _ => continue,
                };
                let Ok(v) = serde_json::from_str::<Value>(&txt) else {
                    continue;
                };
                // Records the turn id before TurnStarted reaches the presenter; only that turn's completion clears it.
                if let (Some(thread), Some(turn)) = (
                    v.pointer("/params/threadId").and_then(Value::as_str),
                    v.pointer("/params/turn/id").and_then(Value::as_str),
                ) {
                    let mut active = turns.lock().await;
                    match v["method"].as_str() {
                        Some("turn/started") => {
                            active.insert(thread.to_string(), turn.to_string());
                        }
                        Some("turn/completed")
                            if active.get(thread).map(String::as_str) == Some(turn) =>
                        {
                            active.remove(thread);
                        }
                        _ => {}
                    }
                }
                dispatch(v, &pend, &evt, &mut coalesce_text, &mut coalesce_cmd, &mut coalesce_reason).await;
            }
            let _ = evt
                .send(Event::Other {
                    method: "connection/closed".into(),
                    params: Value::Null,
                })
                .await;
        });

        let client = Self {
            out: out_tx,
            pending,
            next_id: Arc::new(AtomicI64::new(1)),
            active_turns,
        };

        client
            .request(
                "initialize",
                json!({
                    "clientInfo": { "name": client_name, "title": client_name, "version": env!("CARGO_PKG_VERSION") },
                    "capabilities": { "experimentalApi": true }
                }),
            )
            .await?;

        Ok((client, ev_rx))
    }

    /// Send a request and await its response, giving up after [`REQUEST_TIMEOUT`].
    pub async fn request(&self, method: &str, params: Value) -> Result<Value> {
        self.request_with_timeout(method, params, REQUEST_TIMEOUT).await
    }

    /// A lost response otherwise parks the caller forever and leaks the pending entry.
    pub async fn request_with_timeout(&self, method: &str, params: Value, timeout: Duration) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id.to_string(), tx);
        let msg = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        self.out.send(msg.to_string()).await.map_err(|_| anyhow!("connection closed"))?;
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(Ok(v))) => Ok(v),
            Ok(Ok(Err(e))) => Err(anyhow!("{method} failed: {e}")),
            Ok(Err(_)) => Err(anyhow!("{method}: connection closed before response")),
            Err(_) => {
                self.pending.lock().await.remove(&id.to_string());
                Err(anyhow!("{method}: no response within {}s", timeout.as_secs()))
            }
        }
    }

    /// Answer an [`Event::Ask`]. Until this lands, the turn that raised it is blocked.
    pub async fn respond(&self, request_id: &Value, result: Value) -> Result<()> {
        let msg = json!({ "jsonrpc": "2.0", "id": request_id, "result": result });
        self.out.send(msg.to_string()).await.map_err(|_| anyhow!("connection closed"))?;
        Ok(())
    }

    /// Reject an [`Event::Ask`] we cannot satisfy. Prefer a real decision where the
    /// request defines one — `decline` keeps the turn alive, an error may not.
    pub async fn respond_error(&self, request_id: &Value, code: i64, message: &str) -> Result<()> {
        let msg = json!({ "jsonrpc": "2.0", "id": request_id, "error": { "code": code, "message": message } });
        self.out.send(msg.to_string()).await.map_err(|_| anyhow!("connection closed"))?;
        Ok(())
    }

    /// `cwd` is how a remote client chooses *where* a session runs; there is no later
    /// `cd`, so a different directory means a different thread.
    pub async fn thread_start(&self, params: Value) -> Result<String> {
        let r = self.request("thread/start", params).await?;
        r.get("threadId")
            .or_else(|| r.pointer("/thread/id"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .ok_or_else(|| anyhow!("thread/start returned no thread id: {r}"))
    }

    pub async fn turn_start_with_inputs(&self, thread_id: &str, input: Vec<Value>) -> Result<Value> {
        self.turn_start_with(thread_id, input, None).await
    }

    /// Start a turn, optionally on a different model than the thread was created with.
    ///
    /// `turn/start` takes per-turn overrides, which is the only way to change model on a thread
    /// that already exists — a resumed one keeps whatever it was started with otherwise, and a
    /// picker that only relabels itself is worse than none.
    pub async fn turn_start_with(
        &self,
        thread_id: &str,
        input: Vec<Value>,
        model: Option<&str>,
    ) -> Result<Value> {
        let mut params = json!({ "threadId": thread_id, "input": input });
        if let Some(m) = model.map(str::trim).filter(|m| !m.is_empty()) {
            params["model"] = json!(m);
        }
        self.request("turn/start", params).await
    }

    pub async fn turn_start(&self, thread_id: &str, text: &str) -> Result<Value> {
        self.turn_start_with_inputs(thread_id, vec![json!({ "type": "text", "text": text })]).await
    }

    pub async fn turn_steer_with_inputs(&self, thread_id: &str, input: Vec<Value>) -> Result<Value> {
        self.turn_steer_with(thread_id, input, None).await
    }

    /// Steer an in-flight turn, carrying the same per-turn model override `turn/start` takes.
    pub async fn turn_steer_with(
        &self,
        thread_id: &str,
        input: Vec<Value>,
        model: Option<&str>,
    ) -> Result<Value> {
        let mut params = json!({ "threadId": thread_id, "input": input });
        if let Some(m) = model.map(str::trim).filter(|m| !m.is_empty()) {
            params["model"] = json!(m);
        }
        self.request("turn/steer", params).await
    }

    /// Redirect an in-flight turn. A user typing while the agent works should steer
    /// rather than open a second turn.
    pub async fn turn_steer(&self, thread_id: &str, text: &str) -> Result<Value> {
        self.turn_steer_with_inputs(thread_id, vec![json!({ "type": "text", "text": text })]).await
    }

    pub async fn turn_interrupt(&self, thread_id: &str) -> Result<Value> {
        let known_turn = self.active_turns.lock().await.get(thread_id).cloned();
        let turn_id = match known_turn {
            Some(id) => id,
            None => {
                // Reads the thread's in-progress turn when turn/started was missed.
                let snapshot = self
                    .request(
                        "thread/read",
                        json!({
                            "threadId": thread_id, "includeTurns": true,
                        }),
                    )
                    .await?;
                snapshot
                    .pointer("/thread/turns")
                    .and_then(Value::as_array)
                    .and_then(|turns| {
                        turns
                            .iter()
                            .rev()
                            .find(|turn| turn["status"] == "inProgress")
                    })
                    .and_then(|turn| turn["id"].as_str())
                    .ok_or_else(|| anyhow!("No active turn to stop. Refresh the session."))?
                    .to_string()
            }
        };
        self.request(
            "turn/interrupt",
            json!({ "threadId": thread_id, "turnId": turn_id }),
        )
        .await
    }

    pub async fn compact(&self, thread_id: &str) -> Result<Value> {
        self.request("thread/compact/start", json!({ "threadId": thread_id })).await
    }

    pub async fn resume(&self, thread_id: &str) -> Result<Value> {
        self.request("thread/resume", json!({ "threadId": thread_id })).await
    }
}

async fn dispatch(
    v: Value,
    pending: &Pending,
    ev: &mpsc::Sender<Event>,
    ctext: &mut Coalescer,
    ccmd: &mut Coalescer,
    creason: &mut Coalescer,
) {
    let has_method = v.get("method").is_some();
    let has_id = v.get("id").is_some();

    // Response to one of our requests.
    if !has_method && has_id {
        let id = v["id"].to_string().trim_matches('"').to_string();
        if let Some(tx) = pending.lock().await.remove(&id) {
            let _ = tx.send(if let Some(e) = v.get("error") {
                Err(e.clone())
            } else {
                Ok(v.get("result").cloned().unwrap_or(Value::Null))
            });
        }
        return;
    }

    let method = v["method"].as_str().unwrap_or_default().to_string();
    let params = v.get("params").cloned().unwrap_or(Value::Null);

    // Server -> client REQUEST. Has both method and id. Blocks the turn until answered.
    if has_method && has_id {
        let thread_id = params["threadId"].as_str().unwrap_or_default().to_string();
        let _ = ev
            .send(Event::Ask {
                thread_id,
                request_id: v["id"].clone(),
                method,
                params,
            })
            .await;
        return;
    }

    // Notification.
    let thread_id = params["threadId"].as_str().unwrap_or_default().to_string();
    let item_id = params["itemId"].as_str().unwrap_or_default().to_string();
    let out = match method.as_str() {
        "item/agentMessage/delta" => {
            let d = params["delta"].as_str().unwrap_or_default();
            match ctext.push(&item_id, d) {
                Some(text) => Some(Event::Text { thread_id, item_id, text, final_chunk: false }),
                None => None,
            }
        }
        // Both spellings carry the same shape; the summary stream is what a model that hides
        // its raw chain-of-thought sends instead.
        "item/reasoning/textDelta" | "item/reasoning/summaryTextDelta" => {
            let d = params["delta"].as_str().unwrap_or_default();
            match creason.push(&item_id, d) {
                Some(text) => Some(Event::Reasoning { thread_id, item_id, text, final_chunk: false }),
                None => None,
            }
        }
        "item/commandExecution/outputDelta" => {
            let d = params["delta"].as_str().unwrap_or_default();
            match ccmd.push(&item_id, d) {
                Some(text) => Some(Event::CommandOutput { thread_id, item_id, text, final_chunk: false }),
                None => None,
            }
        }
        "item/started" | "item/completed" => {
            let completed = method == "item/completed";
            let item = params["item"].clone();
            let iid = item["id"].as_str().unwrap_or(&item_id).to_string();
            // Release anything still buffered so the tail of a message is never lost.
            if completed {
                if let Some(text) = ctext.take(&iid) {
                    let _ = ev
                        .send(Event::Text { thread_id: thread_id.clone(), item_id: iid.clone(), text, final_chunk: true })
                        .await;
                }
                if let Some(text) = ccmd.take(&iid) {
                    let _ = ev
                        .send(Event::CommandOutput { thread_id: thread_id.clone(), item_id: iid.clone(), text, final_chunk: true })
                        .await;
                }
                if let Some(text) = creason.take(&iid) {
                    let _ = ev
                        .send(Event::Reasoning { thread_id: thread_id.clone(), item_id: iid.clone(), text, final_chunk: true })
                        .await;
                }
            }
            Some(Event::Item {
                thread_id,
                item_type: item["type"].as_str().unwrap_or("unknown").to_string(),
                completed,
                item,
            })
        }
        "turn/started" => Some(Event::TurnStarted { thread_id }),
        "turn/completed" => {
            for (iid, text) in ctext.drain() {
                let _ = ev
                    .send(Event::Text { thread_id: thread_id.clone(), item_id: iid, text, final_chunk: true })
                    .await;
            }
            for (iid, text) in creason.drain() {
                let _ = ev
                    .send(Event::Reasoning { thread_id: thread_id.clone(), item_id: iid, text, final_chunk: true })
                    .await;
            }
            for (iid, text) in ccmd.drain() {
                let _ = ev
                    .send(Event::CommandOutput { thread_id: thread_id.clone(), item_id: iid, text, final_chunk: true })
                    .await;
            }
            Some(Event::TurnCompleted { thread_id })
        }
        "error" => Some(Event::Error {
            thread_id,
            message: params.pointer("/error/message").and_then(|v| v.as_str()).unwrap_or("unknown error").to_string(),
            will_retry: params["willRetry"].as_bool().unwrap_or(false),
        }),
        "thread/tokenUsage/updated" => Some(Event::TokenUsage {
            thread_id,
            used: context_tokens(&params["tokenUsage"]),
            window: params.pointer("/tokenUsage/modelContextWindow").and_then(|v| v.as_u64()),
        }),
        "serverRequest/resolved" => Some(Event::AskResolved { request_id: params["requestId"].clone() }),
        _ => Some(Event::Other { method, params }),
    };
    if let Some(e) = out {
        let _ = ev.send(e).await;
    }
}

/// Tokens the last request held in the context window, less its reasoning output.
fn context_tokens(usage: &Value) -> Option<u64> {
    let last = usage.get("last")?;
    let total = last.get("totalTokens")?.as_u64()?;
    let reasoning = last.get("reasoningOutputTokens").and_then(Value::as_u64).unwrap_or(0);
    Some(total.saturating_sub(reasoning))
}

/// Decisions for `item/commandExecution/requestApproval` and
/// `item/fileChange/requestApproval`.
///
/// `Decline` and `Cancel` are not synonyms and users care about the difference: declining
/// refuses this one action and lets the agent adapt, cancelling kills the turn.
/// Replies to `mcpServer/elicitation/request`, which is how an MCP tool-call approval arrives
/// (`params._meta.codex_approval_kind == "mcp_tool_call"`). A `{decision}` body is read as a
/// rejection, so these are a separate shape from [`decision`].
pub mod elicitation {
    use serde_json::{json, Value};
    /// `content` follows `params.requestedSchema`; the approval form has no fields.
    pub fn accept() -> Value { json!({ "action": "accept", "content": {} }) }
    pub fn decline() -> Value { json!({ "action": "decline" }) }
    pub fn cancel() -> Value { json!({ "action": "cancel" }) }
}

pub mod decision {
    use serde_json::{json, Value};
    pub fn accept() -> Value { json!({ "decision": "accept" }) }
    pub fn accept_for_session() -> Value { json!({ "decision": "acceptForSession" }) }
    pub fn decline() -> Value { json!({ "decision": "decline" }) }
    pub fn cancel() -> Value { json!({ "decision": "cancel" }) }
}

#[cfg(test)]
mod tests {
    use super::context_tokens;
    use serde_json::json;

    #[test]
    fn context_tokens_reads_the_last_request() {
        let usage = json!({
            "total": { "totalTokens": 900_000, "inputTokens": 880_000, "cachedInputTokens": 0,
                       "outputTokens": 20_000, "reasoningOutputTokens": 5_000 },
            "last": { "totalTokens": 61_000, "inputTokens": 58_000, "cachedInputTokens": 50_000,
                      "outputTokens": 3_000, "reasoningOutputTokens": 1_000 },
            "modelContextWindow": 131_072
        });
        assert_eq!(context_tokens(&usage), Some(60_000));
        assert_eq!(context_tokens(&json!({ "total": { "totalTokens": 5 } })), None);
    }
}
