//! Routes technician turns (agent_turn rows) to the runner that owns the thread.

use std::sync::Arc;
use std::time::Duration;

use database::live_data::Action;
use database::schema::{AgentThread, AgentTurn, RecordIdExt};

use super::{runner, runner_for, running_count, Config, RunnerCmd};

const LIVE_QUERY: &str = "LIVE SELECT * FROM agent_turn WHERE status = 'pending'";

async fn route(cfg: Arc<Config>, turn: AgentTurn) {
    match AgentTurn::claim(&turn.id).await {
        Ok(true) => {}
        Ok(false) => return,
        Err(e) => {
            log::warn!("codex: turn claim failed for {}: {e}", turn.id.key_string());
            return;
        }
    }
    let key = turn.thread.key_string();
    if let Some(tx) = runner_for(&key) {
        if tx.send(RunnerCmd::Turn(turn.clone())).await.is_ok() {
            return;
        }
    }
    // No runner holds this thread (broker restarted, or the thread was queued): bring one up.
    let thread = match AgentThread::get(&turn.thread).await {
        Ok(Some(t)) if t.is_open() || t.status == "queued" => t,
        Ok(Some(t)) => {
            let _ = AgentTurn::mark_failed(&turn.id, &format!("thread is {}", t.status)).await;
            return;
        }
        _ => {
            let _ = AgentTurn::mark_failed(&turn.id, "thread not found").await;
            return;
        }
    };
    if running_count() >= cfg.max_threads {
        let _ = AgentTurn::mark_failed(&turn.id, "agent pool is full; try again shortly").await;
        return;
    }
    let tx = runner::spawn(cfg, thread, None);
    if tx.send(RunnerCmd::Turn(turn.clone())).await.is_err() {
        let _ = AgentTurn::mark_failed(&turn.id, "runner did not accept the turn").await;
    }
}

/// Watches the turn queue for the life of the process, restarting on stream loss.
pub fn spawn_turn_watcher(cfg: Arc<Config>) {
    tokio::spawn(async move {
        loop {
            for turn in AgentTurn::pending(50).await.unwrap_or_default() {
                route(cfg.clone(), turn).await;
            }
            let (tx, rx) = crossbeam::channel::unbounded::<(Action, AgentTurn)>();
            let listener = tokio::spawn(database::live_data::listen_data_filtered::<AgentTurn>(
                tx,
                LIVE_QUERY.to_string(),
                Vec::new(),
                None,
            ));
            log::info!("codex: watching {LIVE_QUERY}");
            loop {
                match rx.try_recv() {
                    Ok((Action::Create | Action::Update, turn)) => {
                        if turn.status == "pending" {
                            tokio::spawn(route(cfg.clone(), turn));
                        }
                    }
                    Ok(_) => {}
                    Err(crossbeam::channel::TryRecvError::Empty) => {
                        if listener.is_finished() {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(250)).await;
                    }
                    Err(crossbeam::channel::TryRecvError::Disconnected) => break,
                }
            }
            log::warn!("codex: turn stream ended; retrying in 10s");
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    });
}
