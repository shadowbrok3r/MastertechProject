//! `plugin_builder` — a long-running daemon that compiles WASM
//! Mastertech plugins on behalf of admin/MCP agents running on
//! machines without Rust installed.
//!
//! Default transport (DB mode, Slice 4):
//! ```text
//!   MCP plugin_compile_remote ──▶ SurrealDB build_job (pending)
//!                                       │ LIVE SELECT
//!   plugin_builder claims, compiles, writes wasm_bytes back
//!                                       │
//!   MCP plugin_compile_status / axum_server :8082 /api/build/* read results
//! ```
//! The worker registers as a `connected_client` row (`client_kind =
//! build_worker`) and heartbeats every 30 s; `list_build_workers` and
//! `GET /api/build/workers` surface it from that row. No
//! websocket_server2 involvement.
//!
//! Fallback transport (`MASTERTECH_DB_MODE=0`): the legacy
//! websocket_server2 room relay on :8081. The admin connects with
//! `role=master`; this worker connects with `role=client`. Both sides
//! speak the bincode [`BuilderWire`](plugin_builder::BuilderWire)
//! protocol.
//!
//! Configuration (env vars, all with sensible defaults for local dev):
//!
//! | Variable                          | Default                                   |
//! |-----------------------------------|-------------------------------------------|
//! | `MASTERTECH_DB_MODE`              | on (`0`/`false`/`off` → WS fallback)      |
//! | `MASTERTECH_WS_URL`               | `ws://127.0.0.1:8081/websocket` (WS mode) |
//! | `BUILD_WORKER_HOSTNAME`           | `hostname::get()`                         |
//! | `BUILD_WORKER_ROOM`               | `build_worker_<hostname>` (WS mode)       |
//! | `BUILD_WORKER_SCRATCH_ROOT`       | `/tmp/mtech-builder`                      |
//! | `BUILD_WORKER_TARGET_CACHE_ROOT`  | `/var/cache/mtech-builder` (Docker; use a writable path for bare-metal runs) |
//! | `RUST_LOG`                        | `plugin_builder=info`                     |

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use futures::{SinkExt, StreamExt};
use plugin_builder::compile::{compile_one, sanitize, BuildArtifact, BuildFailure, Config};
use plugin_builder::BuilderWire;
use tokio_tungstenite::tungstenite::Message;
use url::Url;

const WORKER_VERSION: &str = env!("CARGO_PKG_VERSION");
const DEFAULT_WS_URL: &str = "ws://0.0.0.0:8081/websocket";
const DEFAULT_SCRATCH_ROOT: &str = "/tmp/mtech-builder";
const DEFAULT_TARGET_CACHE_ROOT: &str = "/var/cache/mtech-builder";
const RECONNECT_BACKOFF: Duration = Duration::from_secs(5);
/// Cadence for re-sending `BuilderWire::Hello`. The admin-side
/// registry uses the most recent Hello timestamp to prune stale
/// workers; anything quieter than `STALE_AFTER` (in
/// `builder_transport`) gets hidden from `list_build_workers`.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);

fn config_from_env() -> Result<Config> {
    let hostname = std::env::var("BUILD_WORKER_HOSTNAME").ok().unwrap_or_else(|| {
        hostname::get()
            .map(|h| h.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "unknown".to_string())
    });

    let room_id = std::env::var("BUILD_WORKER_ROOM")
        .unwrap_or_else(|_| format!("build_worker_{}", sanitize(&hostname)));

    let base = std::env::var("MASTERTECH_WS_URL").unwrap_or_else(|_| DEFAULT_WS_URL.into());
    let mut url = Url::parse(&base).context("MASTERTECH_WS_URL is not a valid URL")?;
    // Append query params; preserve any caller-supplied ones.
    url.query_pairs_mut()
        .append_pair("room_id", &room_id)
        .append_pair("role", "client");

    let scratch_root = std::env::var("BUILD_WORKER_SCRATCH_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_SCRATCH_ROOT));
    let target_cache_root = std::env::var("BUILD_WORKER_TARGET_CACHE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| writable_target_cache_root());

    Ok(Config {
        ws_url: url,
        hostname,
        target_triples: detect_target_triples(),
        scratch_root,
        target_cache_root,
    })
}

/// `/var/cache/mtech-builder` when creatable (Docker), else the user
/// cache dir so bare-metal `cargo run` works without env overrides.
fn writable_target_cache_root() -> PathBuf {
    let preferred = PathBuf::from(DEFAULT_TARGET_CACHE_ROOT);
    if std::fs::create_dir_all(&preferred).is_ok() {
        return preferred;
    }
    let fallback = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("mtech-builder");
    log::warn!(
        "{} not writable; using target cache {} (override with BUILD_WORKER_TARGET_CACHE_ROOT)",
        preferred.display(),
        fallback.display()
    );
    fallback
}

/// Returns true if the worker should use SurrealDB live queries
/// (Slice 4) instead of the WS room model (Slice 1-3). Default ON;
/// set `MASTERTECH_DB_MODE=0` to force the WS path.
fn use_db_mode() -> bool {
    match std::env::var("MASTERTECH_DB_MODE").ok().as_deref() {
        Some("0") | Some("false") | Some("off") => false,
        _ => true,
    }
}

/// Best-effort: ask `rustup target list --installed`. If rustup is not
/// available (e.g. the image installs rust via apt), assume the worker
/// can at least produce its own host triple plus `wasm32-wasip1` so the
/// admin gets something to filter on; the actual `cargo build` will be
/// the source of truth.
fn detect_target_triples() -> Vec<String> {
    let out = std::process::Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output();
    if let Ok(out) = out {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout);
            let v: Vec<String> = s
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect();
            if !v.is_empty() {
                return v;
            }
        }
    }
    vec!["wasm32-wasip1".to_string()]
}

#[tokio::main]
async fn main() -> Result<()> {
    if std::env::var("RUST_LOG").is_err() {
        unsafe { std::env::set_var("RUST_LOG", "plugin_builder=info") };
    }
    env_logger::init();

    let cfg = config_from_env()?;
    log::info!(
        "plugin_builder {} starting; hostname={} url={} targets={:?}",
        WORKER_VERSION,
        cfg.hostname,
        cfg.ws_url,
        cfg.target_triples,
    );

    tokio::fs::create_dir_all(&cfg.scratch_root)
        .await
        .with_context(|| format!("create scratch root {}", cfg.scratch_root.display()))?;
    tokio::fs::create_dir_all(&cfg.target_cache_root)
        .await
        .with_context(|| format!("create target cache root {}", cfg.target_cache_root.display()))?;

    if use_db_mode() {
        log::info!("transport: SurrealDB live-query (set MASTERTECH_DB_MODE=0 to use WS fallback)");
        loop {
            match plugin_builder::db_mode::run(cfg.clone()).await {
                Ok(()) => log::warn!("db_mode loop ended cleanly; restarting"),
                Err(e) => log::error!("db_mode failed: {e:?}; restarting after backoff"),
            }
            tokio::time::sleep(RECONNECT_BACKOFF).await;
        }
    }

    log::info!("transport: WS fallback (MASTERTECH_DB_MODE=0)");
    loop {
        match run_once(&cfg).await {
            Ok(()) => log::warn!("WebSocket session ended cleanly; reconnecting"),
            Err(e) => log::error!("WebSocket session failed: {e:?}; reconnecting"),
        }
        tokio::time::sleep(RECONNECT_BACKOFF).await;
    }
}

async fn run_once(cfg: &Config) -> Result<()> {
    let (ws_stream, response) =
        tokio_tungstenite::connect_async(cfg.ws_url.as_str())
            .await
            .with_context(|| format!("connect {}", cfg.ws_url))?;
    log::info!(
        "connected to {} (HTTP {})",
        cfg.ws_url,
        response.status().as_u16()
    );

    let (mut tx, mut rx) = ws_stream.split();

    // Greet the admin/MCP side so a `list_build_workers` call can see
    // our targets before the first compile request is dispatched.
    send_hello(cfg, &mut tx).await?;
    log::debug!("sent Hello");

    // Heartbeat: re-send Hello on a fixed cadence. The admin uses the
    // most recent Hello timestamp to prune stale workers, so this is
    // both a liveness signal and a registry refresh.
    let mut heartbeat = tokio::time::interval(HEARTBEAT_INTERVAL);
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    heartbeat.tick().await; // consume the immediate first tick

    loop {
        tokio::select! {
            biased;
            msg = rx.next() => {
                let Some(msg) = msg else {
                    log::info!("ws stream ended");
                    return Ok(());
                };
                let msg = msg.context("ws recv")?;
                match msg {
                    Message::Binary(bytes) => {
                        let wire = match BuilderWire::decode_tagged(&bytes) {
                            Ok(Some(w)) => w,
                            Ok(None) => {
                                log::debug!(
                                    "ignoring untagged binary frame ({} bytes); first byte 0x{:02x}",
                                    bytes.len(),
                                    bytes.first().copied().unwrap_or(0)
                                );
                                continue;
                            }
                            Err(e) => {
                                log::warn!("ignoring undecodable BuilderWire frame ({} bytes): {e}", bytes.len());
                                continue;
                            }
                        };
                        handle_message(cfg, wire, &mut tx).await?;
                    }
                    Message::Text(t) => {
                        // Chat-server status pings (`MASTER_CONNECTED`, etc.)
                        // are delivered as text; log + ignore.
                        log::debug!("text frame: {t}");
                    }
                    Message::Ping(p) => tx.send(Message::Pong(p)).await?,
                    Message::Pong(_) => {}
                    Message::Close(_) => {
                        log::info!("peer closed connection");
                        return Ok(());
                    }
                    Message::Frame(_) => {}
                }
            }
            _ = heartbeat.tick() => {
                if let Err(e) = send_hello(cfg, &mut tx).await {
                    log::warn!("heartbeat send failed: {e}; reconnect");
                    return Err(e);
                }
            }
        }
    }
}

async fn send_hello(
    cfg: &Config,
    tx: &mut (impl futures::Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin),
) -> Result<()> {
    let hello = BuilderWire::Hello {
        hostname: cfg.hostname.clone(),
        target_triples: cfg.target_triples.clone(),
        capabilities: vec!["wasm-plugin".to_string()],
        worker_version: WORKER_VERSION.to_string(),
    };
    tx.send(Message::Binary(hello.encode_tagged()?.into())).await?;
    Ok(())
}

async fn handle_message(
    cfg: &Config,
    wire: BuilderWire,
    tx: &mut (impl futures::Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin),
) -> Result<()> {
    match wire {
        BuilderWire::CompileRequest {
            job_id,
            plugin_id,
            cargo_toml,
            lib_rs,
            target,
            profile,
        } => {
            log::info!("[{job_id}] compile request: plugin={plugin_id} target={target} profile={profile}");
            // Acknowledge before we kick off cargo so the admin sees motion.
            let _ = tx
                .send(Message::Binary(
                    BuilderWire::CompileProgress {
                        job_id: job_id.clone(),
                        stage: "queued".to_string(),
                        message: format!("worker {} accepted job", cfg.hostname),
                    }
                    .encode_tagged()?
                    .into(),
                ))
                .await;

            let start = std::time::Instant::now();
            let result = compile_one(cfg, &job_id, &plugin_id, &cargo_toml, &lib_rs, &target, &profile).await;
            let dur_ms = start.elapsed().as_millis() as u64;
            let payload = match result {
                Ok(BuildArtifact { wasm_bytes, stdout, stderr }) => BuilderWire::CompileResult {
                    job_id,
                    success: true,
                    wasm_bytes: Some(wasm_bytes),
                    stdout,
                    stderr,
                    duration_ms: dur_ms,
                },
                Err(BuildFailure::Cargo { dur, stdout, stderr }) => BuilderWire::CompileResult {
                    job_id,
                    success: false,
                    wasm_bytes: None,
                    stdout,
                    stderr,
                    duration_ms: dur.as_millis() as u64,
                },
                Err(BuildFailure::Setup(e)) => BuilderWire::CompileResult {
                    job_id,
                    success: false,
                    wasm_bytes: None,
                    stdout: String::new(),
                    stderr: format!("worker setup error: {e:#}"),
                    duration_ms: 0,
                },
            };
            tx.send(Message::Binary(payload.encode_tagged()?.into())).await?;
        }
        BuilderWire::Hello { .. }
        | BuilderWire::CompileProgress { .. }
        | BuilderWire::CompileResult { .. } => {
            log::warn!("ignoring inbound message of a worker→admin variant");
        }
    }
    Ok(())
}
