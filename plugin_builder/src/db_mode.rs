//! SurrealDB-backed worker mode (Slice 4).
//!
//! On startup we:
//!   1. Upsert our own `connected_client` row with
//!      `client_kind = build_worker`. Deterministic record id (derived
//!      from hostname) so reconnects land on the same row instead of
//!      orphaning ghosts.
//!   2. Drain any jobs that were already pending when we started.
//!   3. Subscribe via `listen_data_filtered` to a guarded
//!      `LIVE SELECT * FROM build_job` and process each create/update
//!      event. Claiming is race-safe (atomic UPDATE … WHERE
//!      status='pending'), so multiple workers competing for the same
//!      unassigned job all serialize through SurrealDB.
//!   4. Heartbeat by touching `last_update` on our row every 30 s;
//!      this is what `axum_server /api/build/workers` reads to decide
//!      whether to surface us to MCP callers.

use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use crossbeam::channel::{Receiver, TryRecvError};
use database::live_data::Action;
use database::schema::{
    BuildJob, ClientKind, ConnectedClient, RecordId, BUILD_JOB_TABLE, CONNECTED_CLIENT_TABLE,
};
use database::DATABASE;

use crate::compile::{compile_one, BuildArtifact, BuildFailure};
use crate::Config;

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
const LIVE_QUERY: &str = "LIVE SELECT * FROM build_job \
                           WHERE status = 'pending' \
                             AND (assigned_worker_id == NONE OR assigned_worker_id == $worker)";

pub async fn run(cfg: Config) -> Result<()> {
    database::init_database()
        .await
        .context("SurrealDB init failed (set MASTERTECH_DB_MODE=0 to force WS fallback)")?;
    log::info!(
        "plugin_builder DB mode: connected. hostname={} targets={:?}",
        cfg.hostname,
        cfg.target_triples
    );

    let worker_id = worker_record_id(&cfg.hostname);
    upsert_self(&worker_id, &cfg)
        .await
        .context("upsert connected_client row")?;

    // Drain any pending jobs queued before our LIVE subscription was active.
    drain_pending(&worker_id, &cfg).await;

    // Spawn the heartbeat task. It owns its own clone of the record id;
    // failures are logged and retried — a missed heartbeat won't crash
    // the worker.
    let hb_worker_id = worker_id.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(HEARTBEAT_INTERVAL).await;
            if let Err(e) = touch_last_update(&hb_worker_id).await {
                log::warn!("heartbeat touch failed: {e:?}");
            }
        }
    });

    // listen_data_filtered runs on a tokio runtime but pushes events
    // to a crossbeam channel (so the same helper works across the
    // codebase's UI/non-UI consumers). We pull from that channel in a
    // blocking-style loop here.
    let (tx, rx) = crossbeam::channel::unbounded::<(Action, BuildJob)>();
    let live_query = LIVE_QUERY.to_string();
    let worker_bind = serde_json::to_value(&worker_id).unwrap_or(serde_json::Value::Null);
    let bindings = vec![("worker", worker_bind)];

    // The live subscription itself is async — spawn it so the result-
    // loop below can process events synchronously without blocking the
    // tokio runtime.
    tokio::spawn(async move {
        if let Err(e) = database::live_data::listen_data_filtered::<BuildJob>(
            tx, live_query, bindings,
        )
        .await
        {
            log::error!("LIVE SELECT stream ended: {e:?}");
        }
    });

    event_loop(rx, worker_id, cfg).await
}

async fn event_loop(
    rx: Receiver<(Action, BuildJob)>,
    worker_id: RecordId,
    cfg: Config,
) -> Result<()> {
    loop {
        // Cooperative poll loop: we want to consume events as they
        // arrive but also let other tokio tasks run. `try_recv` +
        // a short sleep keeps us off the runtime when idle.
        match rx.try_recv() {
            Ok((action, job)) => {
                if !matches!(action, Action::Create | Action::Update) {
                    continue;
                }
                if job.status != "pending" {
                    continue;
                }
                let cfg_clone = cfg.clone();
                let worker_clone = worker_id.clone();
                // Each claim+compile is its own task so a long compile
                // doesn't block other jobs from being claimed. The
                // worker happily processes multiple builds in parallel
                // if the host has the CPU for it.
                tokio::spawn(async move {
                    process_job(job, worker_clone, cfg_clone).await;
                });
            }
            Err(TryRecvError::Empty) => {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            Err(TryRecvError::Disconnected) => {
                anyhow::bail!("LIVE SELECT producer dropped; restarting db_mode");
            }
        }
    }
}

async fn process_job(job: BuildJob, worker_id: RecordId, cfg: Config) {
    let job_id = job.id.clone();
    // Atomic claim. If we lost the race, just bail — another worker
    // took it. This is also how we deduplicate Create+Update events
    // emitted by SurrealDB for the same logical row transition.
    match BuildJob::claim(&job_id, &worker_id).await {
        Ok(Some(claimed)) => {
            log::info!(
                "[{}] claimed by {} — plugin={} target={}",
                claimed.id.key_string_safe(),
                cfg.hostname,
                claimed.plugin_id,
                claimed.target
            );
            run_and_record(claimed, cfg).await;
        }
        Ok(None) => {
            log::debug!(
                "[{}] not claimable (raced by another worker or already terminal)",
                job_id.key_string_safe()
            );
        }
        Err(e) => log::warn!("[{}] claim error: {e:?}", job_id.key_string_safe()),
    }
}

async fn run_and_record(job: BuildJob, cfg: Config) {
    let id_for_log = job.id.key_string_safe();
    let start = Instant::now();
    let outcome = compile_one(
        &cfg,
        &id_for_log,
        &job.plugin_id,
        &job.cargo_toml,
        &job.lib_rs,
        &job.target,
        &job.profile,
    )
    .await;
    match outcome {
        Ok(BuildArtifact {
            wasm_bytes,
            stdout,
            stderr,
        }) => {
            let dur = start.elapsed().as_millis() as u64;
            if let Err(e) = BuildJob::finish_success(
                &job.id, wasm_bytes, stdout, stderr, dur,
            )
            .await
            {
                log::error!("[{id_for_log}] finish_success write failed: {e:?}");
            }
        }
        Err(BuildFailure::Cargo { stdout, stderr, dur }) => {
            if let Err(e) = BuildJob::finish_failure(
                &job.id,
                stdout,
                stderr,
                dur.as_millis() as u64,
            )
            .await
            {
                log::error!("[{id_for_log}] finish_failure write failed: {e:?}");
            }
        }
        Err(BuildFailure::Setup(e)) => {
            if let Err(write_err) = BuildJob::finish_failure(
                &job.id,
                String::new(),
                format!("worker setup error: {e:#}"),
                0,
            )
            .await
            {
                log::error!("[{id_for_log}] finish_failure write failed: {write_err:?}");
            }
        }
    }
}

// ── Self-registration ─────────────────────────────────────────────

/// Deterministic record id keyed off hostname. Two `plugin_builder`
/// processes on the same host will land on the same row, which is
/// the desired behavior (the row represents the host, not the PID).
fn worker_record_id(hostname: &str) -> RecordId {
    let key = sanitize(&format!("build_worker_{hostname}"));
    RecordId::new(CONNECTED_CLIENT_TABLE, key)
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

async fn upsert_self(worker_id: &RecordId, cfg: &Config) -> Result<()> {
    let now: database::schema::Datetime = chrono::Utc::now().into();
    let row = ConnectedClient {
        id: worker_id.clone(),
        connection_string: format!("build_worker_{}", cfg.hostname),
        client_hash: String::new(),
        friendly_name: Some(cfg.hostname.clone()),
        connected: true,
        client_kind: ClientKind::BuildWorker,
        last_update: Some(now.clone()),
        created_at: Some(now),
        ..ConnectedClient::default()
    };
    let _: Option<ConnectedClient> = DATABASE
        .upsert(worker_id.clone())
        .content(row)
        .await
        .context("upsert build_worker row")?;
    Ok(())
}

async fn touch_last_update(worker_id: &RecordId) -> Result<()> {
    let _: Option<ConnectedClient> = DATABASE
        .query("UPDATE $id SET last_update = time::now(), connected = true RETURN AFTER")
        .bind(("id", worker_id.clone()))
        .await
        .context("touch last_update")?
        .take(0)
        .context("decode touch response")?;
    Ok(())
}

async fn drain_pending(worker_id: &RecordId, cfg: &Config) {
    match BuildJob::pending_for_worker(worker_id).await {
        Ok(jobs) if !jobs.is_empty() => {
            log::info!("draining {} pending job(s) on startup", jobs.len());
            for job in jobs {
                let worker_clone = worker_id.clone();
                let cfg_clone = cfg.clone();
                tokio::spawn(async move {
                    process_job(job, worker_clone, cfg_clone).await;
                });
            }
        }
        Ok(_) => log::info!("no pending jobs at startup"),
        Err(e) => log::warn!("drain_pending failed: {e:?}"),
    }
}

// Small extension so we can log a record id without pulling RecordIdExt
// (which is fine to import, but this keeps the imports tight).
trait KeyStringSafe {
    fn key_string_safe(&self) -> String;
}

impl KeyStringSafe for RecordId {
    fn key_string_safe(&self) -> String {
        use database::schema::RecordIdExt;
        self.key_string()
    }
}

