//! Routes technician turns (agent_turn rows) to the runner that owns the thread.

use std::sync::Arc;
use std::time::Duration;

use database::live_data::Action;
use database::schema::agent_thread::AgentThreadState;
use database::schema::agent_turn::APPROVALS_PROMPT;
use database::schema::{clean_title, AgentEvent, AgentThread, AgentTurn, RecordIdExt};
use serde_json::json;
use zc_codex_client::Client;

use super::{runner, runner_for, running_count, Config, RunnerCmd};

const LIVE_QUERY: &str = "LIVE SELECT * FROM agent_turn WHERE status = 'pending'";
/// Ceiling on the one-shot `thread/name/set` a rename makes.
const RENAME_TIMEOUT: Duration = Duration::from_secs(20);

async fn route(cfg: Arc<Config>, turn: AgentTurn) {
    match AgentTurn::claim(&turn.id).await {
        Ok(true) => {}
        Ok(false) => return,
        Err(e) => {
            log::warn!("codex: turn claim failed for {}: {e}", turn.id.key_string());
            return;
        }
    }
    if turn.kind == "rename" {
        rename(&cfg, &turn).await;
        return;
    }
    let key = turn.thread.key_string();
    if let Some(tx) = runner_for(&key) {
        if tx.send(RunnerCmd::Turn(turn.clone())).await.is_ok() {
            return;
        }
    }
    if turn.kind == "approvals" {
        approvals_without_runner(&turn).await;
        return;
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

/// Turns approve-all off on the thread row, then hands the turn to a runner that came up meanwhile.
async fn approvals_without_runner(turn: &AgentTurn) {
    if turn.text.trim() != APPROVALS_PROMPT {
        let _ = AgentTurn::mark_failed(&turn.id, "unknown approvals setting").await;
        return;
    }
    let thread = match AgentThread::get(&turn.thread).await {
        Ok(Some(t)) => t,
        Ok(None) => {
            let _ = AgentTurn::mark_failed(&turn.id, "thread not found").await;
            return;
        }
        Err(e) => {
            let _ = AgentTurn::mark_failed(&turn.id, &e.to_string()).await;
            return;
        }
    };
    if let Err(e) = AgentThread::set_approve_all(&turn.thread, false).await {
        log::warn!("codex: could not turn approve-all off on {}: {e}", turn.thread.key_string());
        let _ = AgentTurn::mark_failed(&turn.id, &e.to_string()).await;
        return;
    }
    if let Some(tx) = runner_for(&turn.thread.key_string()) {
        let _ = tx.send(RunnerCmd::Turn(turn.clone())).await;
        return;
    }
    if !thread.approves_all() {
        return;
    }
    let recorded = database::agent_chat::last_seq(&turn.thread).await.unwrap_or(0);
    let seq = recorded.max(thread.last_seq.unwrap_or(0)) + 1;
    if let Err(e) = AgentEvent::marker(&turn.thread, seq, None, "approval", runner::PROMPTS_ON, None).await {
        log::warn!("codex: approvals marker write failed: {e}");
        return;
    }
    let state = AgentThreadState { last_seq: Some(seq), ..Default::default() };
    if let Err(e) = AgentThread::save_state(&turn.thread, &state).await {
        log::warn!("codex: thread write failed: {e}");
    }
}

/// Stores the turn's text as the thread's title, then names the codex thread the same, best-effort.
pub(super) async fn rename(cfg: &Config, turn: &AgentTurn) {
    let Some(title) = clean_title(&turn.text) else {
        let _ = AgentTurn::mark_failed(&turn.id, "empty title").await;
        return;
    };
    if let Err(e) = AgentThread::set_title(&turn.thread, &title).await {
        log::warn!("codex: rename of {} failed: {e}", turn.thread.key_string());
        let _ = AgentTurn::mark_failed(&turn.id, &e.to_string()).await;
        return;
    }
    let codex_thread = match AgentThread::get(&turn.thread).await {
        Ok(Some(t)) => t.codex_thread_id,
        _ => None,
    };
    let Some(codex_thread) = codex_thread else {
        return;
    };
    let token = (!cfg.token.is_empty()).then_some(cfg.token.as_str());
    let named = match Client::connect_with_token(&cfg.url, "mastertech-broker", token).await {
        Ok((client, _events)) => client
            .request_with_timeout(
                "thread/name/set",
                json!({ "threadId": codex_thread, "name": title }),
                RENAME_TIMEOUT,
            )
            .await
            .map(|_| ()),
        Err(e) => Err(e),
    };
    if let Err(e) = named {
        log::info!("codex: thread/name/set for {codex_thread} failed (the title is saved): {e}");
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
