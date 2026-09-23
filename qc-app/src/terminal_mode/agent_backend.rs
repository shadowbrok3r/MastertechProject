//! The terminal Diagnose tab's link to the Codex agent, over the agent tables.

use crossbeam::channel::Sender;
use database::agent_chat;
use database::schema::{general_connection, AgentEvent, AgentThread, ConnectedClient, RecordId};
use rmcp::model::CallToolRequestParams;

/// qc-app's own MCP server (raw TCP rmcp), spawned on the first app tick.
const QC_MCP_ADDR: &str = "127.0.0.1:9100";

/// Prefix marking a tool-activity line.
pub const TOOL_PREFIX: &str = "\u{00BB} ";

/// Longest local snapshot attached to a records session's first message.
const SNAPSHOT_MAX: usize = 6000;

#[derive(Clone, Debug, PartialEq)]
pub enum SentFrom {
    Me,
    Assistant,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ChatMessageType {
    Text(String),
    Error(String),
}

#[derive(Clone, Debug)]
pub struct ChatMessage {
    pub from: SentFrom,
    pub content: ChatMessageType,
}

/// Results the tab drains each frame.
pub enum BackendMsg {
    Opened { thread: RecordId, after_seq: i64, target: String },
    Rows { rows: Vec<AgentEvent>, status: Option<String> },
    Error(String),
}

/// Sends one message: this machine's Mastertech session when it has one, else the technician's records session.
pub async fn send(tech_email: Option<String>, text: String, first: bool, tx: Sender<BackendMsg>) {
    let (target, general) = match this_machine_session().await {
        Some(cs) => (cs, false),
        None => match tech_email.as_deref() {
            Some(email) => (general_connection(email), true),
            None => {
                let _ = tx.send(BackendMsg::Error(
                    "Sign in on the Order QC tab, or run Mastertech on this machine, to talk to the agent.".into(),
                ));
                return;
            }
        },
    };
    let snapshot = if first && general { local_snapshot().await } else { None };
    let body = match snapshot {
        Some(snapshot) => format!(
            "{text}\n\nLocal QC snapshot of this machine ({}), taken by qc-app:\n{snapshot}",
            hostname()
        ),
        None => text,
    };
    match agent_chat::send(&target, tech_email.as_deref(), None, None, &body).await {
        Ok(sent) => {
            let _ = tx.send(BackendMsg::Opened { thread: sent.thread, after_seq: sent.after_seq, target });
        }
        Err(e) => {
            let _ = tx.send(BackendMsg::Error(format!("could not reach the agent: {e:#}")));
        }
    }
}

/// Fetches the session's rows after `after_seq` and its current status.
pub async fn poll(thread: RecordId, after_seq: i64, tx: Sender<BackendMsg>) {
    let rows = AgentEvent::history(&thread, after_seq, 500).await.unwrap_or_default();
    let status = AgentThread::get(&thread).await.ok().flatten().map(|t| t.status);
    let _ = tx.send(BackendMsg::Rows { rows, status });
}

/// The live Mastertech client on this machine, when one is connected and cleared for diagnosis.
async fn this_machine_session() -> Option<String> {
    let prefix = format!("{}:", hostname().to_lowercase());
    let mut res = database::db()
        .query(
            "SELECT VALUE connection_string FROM connected_client WHERE connected = true \
             AND client_kind = 'machine' AND string::starts_with(string::lowercase(connection_string), $prefix) LIMIT 1",
        )
        .bind(("prefix", prefix))
        .await
        .ok()?;
    let cs: Option<String> = res.take::<Vec<String>>(0).ok()?.into_iter().next();
    let cs = cs?;
    ConnectedClient::diagnosis_block(&cs).await.is_none().then_some(cs)
}

/// Current telemetry and the last QC report, read from qc-app's own MCP server.
async fn local_snapshot() -> Option<String> {
    let stream = tokio::net::TcpStream::connect(QC_MCP_ADDR).await.ok()?;
    let (read, write) = tokio::io::split(stream);
    let client = rmcp::serve_client((), (read, write)).await.ok()?;
    let mut parts = Vec::new();
    for tool in ["get_extended_telemetry", "get_last_report"] {
        if let Ok(result) = client.call_tool(CallToolRequestParams::new(tool.to_string())).await {
            let text: String = result
                .content
                .iter()
                .filter_map(|c| c.as_text().map(|t| t.text.clone()))
                .collect::<Vec<_>>()
                .join("\n");
            if !text.trim().is_empty() && text.trim() != "null" {
                parts.push(format!("[{tool}]\n{text}"));
            }
        }
    }
    let _ = client.cancel().await;
    let joined = parts.join("\n\n");
    (!joined.is_empty()).then(|| joined.chars().take(SNAPSHOT_MAX).collect())
}

fn hostname() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "this-pc".into())
}
