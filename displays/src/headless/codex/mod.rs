//! Codex broker: runs each accepted assist request as a Codex thread on the
//! zc-codexd daemon and keeps the transcript, technician turns and approvals in
//! SurrealDB, so any Mastertech instance follows a session through live queries
//! and no desktop ever holds a socket to the agent host.
//!
//! Enabled by `MTECH_CODEXD_URL`; without it the legacy dispatch paths stay in
//! charge. Tools reach Codex as client-side dynamic tools executed here, in
//! process, through the same `PluginToolProvider` the MCP server serves.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::Duration;

use database::schema::{AgentApproval, AgentThread, AssistRequest, RecordIdExt};
use tokio::sync::mpsc;

use crate::plugins::PluginManager;

mod dispatch;
mod prompt;
mod runner;
mod tools;
mod turns;
pub mod zeroclaw;

pub use dispatch::dispatch;
pub use runner::RunnerCmd;

/// Broker settings, all from the environment.
#[derive(Debug, Clone)]
pub struct Config {
    /// zc-codexd websocket, e.g. `ws://192.168.22.7:7420`.
    pub url: String,
    pub token: String,
    pub model: String,
    pub provider: String,
    /// Working directory for threads on the codex host.
    pub cwd: String,
    pub max_threads: usize,
    /// Host name stamped into `driven_by` and `agent_thread.broker_node`.
    pub node: String,
    pub agent_alias: String,
    pub approval_ttl_secs: u64,
    pub tool_output_chars: usize,
    /// Seconds a single tool call may run before the agent is told it timed out.
    pub tool_timeout_secs: u64,
    /// Days a closed thread keeps its transcript, turns and approvals.
    pub retention_days: u64,
    /// ZeroClaw's memory over its gateway, when `MTECH_ZC_GATEWAY`/`MTECH_ZC_TOKEN` are set.
    pub zeroclaw: Option<Arc<zeroclaw::ZeroclawMemory>>,
}

fn env_trimmed(key: &str) -> Option<String> {
    std::env::var(key).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

fn env_parse<T: std::str::FromStr>(key: &str, default: T) -> T {
    env_trimmed(key).and_then(|v| v.parse().ok()).unwrap_or(default)
}

impl Config {
    /// `None` when `MTECH_CODEXD_URL` is unset.
    pub fn from_env() -> Option<Self> {
        let url = env_trimmed("MTECH_CODEXD_URL")?;
        let token = env_trimmed("MTECH_CODEXD_TOKEN").unwrap_or_default();
        if token.is_empty() {
            log::warn!("codex: MTECH_CODEXD_TOKEN unset; the daemon will refuse the handshake");
        }
        let node = env_trimmed("MTECH_AGENT_NODE")
            .or_else(|| env_trimmed("HOSTNAME"))
            .unwrap_or_else(|| "admin-agent".to_string());
        let zeroclaw = zeroclaw::ZeroclawMemory::from_env().map(Arc::new);
        match &zeroclaw {
            Some(m) => log::info!("codex: ZeroClaw memory via {}", m.base()),
            None => log::warn!("codex: MTECH_ZC_GATEWAY/MTECH_ZC_TOKEN unset; sessions run without ZeroClaw memory"),
        }
        Some(Self {
            url,
            token,
            model: env_trimmed("MTECH_CODEX_MODEL").unwrap_or_else(|| "zc-heavy".to_string()),
            provider: env_trimmed("MTECH_CODEX_PROVIDER").unwrap_or_else(|| "zcpool".to_string()),
            cwd: env_trimmed("MTECH_CODEX_CWD").unwrap_or_else(|| "/home/shadowbroker/zc-sessions".to_string()),
            max_threads: env_parse("MTECH_CODEX_MAX_THREADS", 2usize).max(1),
            node,
            agent_alias: env_trimmed("MTECH_CODEX_AGENT").unwrap_or_else(|| "diagnostician".to_string()),
            approval_ttl_secs: env_parse("MTECH_APPROVAL_TTL_SECS", 600u64).max(30),
            tool_output_chars: env_parse("MTECH_CODEX_TOOL_OUTPUT_CHARS", 24_000usize).max(1_000),
            tool_timeout_secs: env_parse("MTECH_CODEX_TOOL_TIMEOUT_SECS", 320u64).max(10),
            retention_days: env_parse("MTECH_AGENT_EVENT_RETENTION_DAYS", 30u64).max(1),
            zeroclaw,
        })
    }

    /// `codex/<alias>@<model>#<node>`, in the provenance grammar the schema asserts.
    pub fn driven_by(&self) -> String {
        format!(
            "codex/{}@{}#{}",
            provenance_slug(&self.agent_alias, false),
            provenance_slug(&self.model, true),
            provenance_slug(&self.node, false)
        )
    }
}

/// Connection string of a technician's session with no machine in scope.
pub fn general_connection(email: &str) -> String {
    format!("general:{}", email.trim().to_lowercase())
}

/// A session with no machine in scope: records-only tools, no remote actions.
pub fn is_general(connection_string: &str) -> bool {
    connection_string.starts_with("general:")
}

/// Lowercases and replaces anything outside the grammar's segment alphabet.
fn provenance_slug(raw: &str, allow_colon: bool) -> String {
    let cleaned: String = raw
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') || (allow_colon && c == ':') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches(|c: char| !c.is_ascii_alphanumeric());
    if trimmed.is_empty() { "unknown".to_string() } else { trimmed.to_string() }
}

static CONFIG: OnceLock<Option<Arc<Config>>> = OnceLock::new();

pub fn config() -> Option<Arc<Config>> {
    CONFIG.get_or_init(|| Config::from_env().map(Arc::new)).clone()
}

pub fn enabled() -> bool {
    config().is_some()
}

type Runners = Mutex<HashMap<String, mpsc::Sender<RunnerCmd>>>;
static RUNNERS: OnceLock<Runners> = OnceLock::new();

fn runners() -> &'static Runners {
    RUNNERS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(super) fn runner_for(thread_key: &str) -> Option<mpsc::Sender<RunnerCmd>> {
    runners().lock().ok().and_then(|m| m.get(thread_key).cloned())
}

/// Claims the thread for a runner; `false` when another runner already holds it.
pub(super) fn register_runner(thread_key: &str, tx: mpsc::Sender<RunnerCmd>) -> bool {
    let Ok(mut m) = runners().lock() else { return false };
    match m.entry(thread_key.to_string()) {
        std::collections::hash_map::Entry::Occupied(_) => false,
        std::collections::hash_map::Entry::Vacant(slot) => {
            slot.insert(tx);
            true
        }
    }
}

pub(super) fn unregister_runner(thread_key: &str) {
    if let Ok(mut m) = runners().lock() {
        m.remove(thread_key);
    }
}

pub(super) fn running_count() -> usize {
    runners().lock().map(|m| m.len()).unwrap_or(0)
}

static MANAGER: OnceLock<Arc<RwLock<PluginManager>>> = OnceLock::new();

pub(super) fn manager() -> Option<Arc<RwLock<PluginManager>>> {
    MANAGER.get().cloned()
}

/// Starts the broker tasks: resume of open threads, the turn watcher, the approval reaper and the queue pump.
pub fn spawn_codex_broker(manager: Arc<RwLock<PluginManager>>) {
    let Some(cfg) = config() else {
        log::info!("codex: MTECH_CODEXD_URL unset; broker disabled");
        return;
    };
    let _ = MANAGER.set(manager);
    log::info!(
        "codex: broker -> {} model {} provider {} (max {} threads, approvals expire after {}s)",
        cfg.url, cfg.model, cfg.provider, cfg.max_threads, cfg.approval_ttl_secs
    );
    tokio::spawn(resume_open_threads(cfg.clone()));
    turns::spawn_turn_watcher(cfg.clone());
    tokio::spawn(approval_reaper());
    tokio::spawn(retention_sweeper(cfg.clone()));
    // Operator hook: start one session for a machine without an assist_request.
    if let Some(cs) = env_trimmed("MTECH_CODEX_START_CS") {
        let by = env_trimmed("MTECH_CODEX_START_BY");
        let cfg = cfg.clone();
        tokio::spawn(async move { dispatch::start_for_connection(cfg, &cs, by.as_deref()).await });
    }
    tokio::spawn(queue_pump(cfg));
}

/// Reattaches to every thread that was live when the broker last stopped.
async fn resume_open_threads(cfg: Arc<Config>) {
    let threads = match AgentThread::open_threads().await {
        Ok(t) => t,
        Err(e) => {
            log::warn!("codex: could not list open threads: {e}");
            return;
        }
    };
    for thread in threads {
        if thread.status == "queued" || runner_for(&thread.id.key_string()).is_some() {
            continue;
        }
        if running_count() >= cfg.max_threads {
            let _ = AgentThread::set_status(&thread.id, "queued", None).await;
            continue;
        }
        log::info!("codex: resuming thread {} for {}", thread.id.key_string(), thread.connection_string);
        runner::spawn(cfg.clone(), thread, None);
    }
}

async fn approval_reaper() {
    loop {
        tokio::time::sleep(Duration::from_secs(30)).await;
        if let Err(e) = AgentApproval::expire_stale().await {
            log::warn!("codex: approval reaper: {e}");
        }
    }
}

/// Drops the transcripts of long-closed threads every six hours; the thread rows stay.
async fn retention_sweeper(cfg: Arc<Config>) {
    let retention = format!("{}d", cfg.retention_days);
    tokio::time::sleep(Duration::from_secs(90)).await;
    loop {
        match AgentThread::purge_closed_before(&retention).await {
            Ok(0) => {}
            Ok(n) => log::info!("codex: purged the transcripts of {n} threads closed over {retention} ago"),
            Err(e) => log::warn!("codex: retention sweep failed: {e}"),
        }
        tokio::time::sleep(Duration::from_secs(6 * 3600)).await;
    }
}

/// Starts queued threads as capacity frees up.
async fn queue_pump(cfg: Arc<Config>) {
    loop {
        tokio::time::sleep(Duration::from_secs(20)).await;
        if running_count() >= cfg.max_threads {
            continue;
        }
        let queued = match AgentThread::open_threads().await {
            Ok(t) => t.into_iter().filter(|t| t.status == "queued").collect::<Vec<_>>(),
            Err(_) => continue,
        };
        for thread in queued {
            if running_count() >= cfg.max_threads {
                break;
            }
            if runner_for(&thread.id.key_string()).is_some() {
                continue;
            }
            // A queued thread never spoke to codex; its opening prompt is rebuilt from the request.
            let opening = match &thread.assist_request {
                Some(req_id) => match AssistRequest::get(req_id).await {
                    Ok(Some(req)) => Some(super::assist::compose_prompt(&req, &cfg.driven_by())),
                    _ => None,
                },
                None => None,
            };
            let _ = AgentThread::set_status(&thread.id, "starting", None).await;
            runner::spawn(cfg.clone(), thread, opening);
        }
    }
}
