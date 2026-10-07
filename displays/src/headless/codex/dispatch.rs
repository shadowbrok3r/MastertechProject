//! Turns a tech-confirmed assist request into an agent_thread and a runner.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use database::schema::{AgentThread, AgentTurn, AssistRequest, NewAgentThread, RecordIdExt};
use tokio::sync::OwnedMutexGuard;

use super::{config, runner, runner_for, running_count, RunnerCmd};

/// Waits for the connection's admission slot; one request at a time decides whether a session opens there.
async fn admission(connection_string: &str) -> OwnedMutexGuard<()> {
    static SLOTS: OnceLock<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> = OnceLock::new();
    let slot = {
        let mut slots = SLOTS.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
        slots.retain(|_, slot| Arc::strong_count(slot) > 1);
        slots.entry(connection_string.to_string()).or_default().clone()
    };
    slot.lock_owned().await
}

/// `another AI session (<label>) is already working on this computer for <requester>`.
pub(super) fn working_elsewhere(working: &AgentThread) -> String {
    format!(
        "another AI session ({}) is already working on this computer for {}",
        working.label(),
        working.requested_by.as_deref().unwrap_or("another technician")
    )
}

/// True for a bench confirmation of the service order the working session already has.
fn repeats_opening(req: &AssistRequest, working: &AgentThread) -> bool {
    req.trigger_source == "tur_sheet"
        && req.tech_note.is_none()
        && (working.service_number.is_none() || working.service_number == req.service_number)
}

/// Opens a session for a machine directly, with the same opening prompt a request would carry.
pub async fn start_for_connection(cfg: std::sync::Arc<super::Config>, connection_string: &str, requested_by: Option<&str>) {
    let _slot = admission(connection_string).await;
    if let Ok(Some(existing)) = AgentThread::active_for_connection(connection_string).await {
        log::info!("codex: {connection_string} already has live thread {}", existing.id.key_string());
        return;
    }
    let hostname = connection_string.split(':').next().map(str::to_string);
    let new = NewAgentThread {
        assist_request: None,
        connection_string: connection_string.to_string(),
        hostname: hostname.clone(),
        service_number: None,
        store: None,
        requested_by: requested_by.map(str::to_string),
        service_order: None,
        computer: None,
        customer: None,
        model: Some(cfg.model_for(connection_string).to_string()),
        provider: Some(cfg.provider.clone()),
        driven_by: Some(cfg.driven_by(connection_string)),
        tool_path: Some("dynamic".to_string()),
        broker_node: Some(cfg.node.clone()),
        title: hostname,
    };
    let thread_id = match AgentThread::create(&new).await {
        Ok(id) => id,
        Err(e) => {
            log::warn!("codex: could not create agent_thread for {connection_string}: {e}");
            return;
        }
    };
    let Ok(Some(thread)) = AgentThread::get(&thread_id).await else { return };
    let mut opening = format!("Check this computer: {connection_string}\ndriven_by: {}\n", cfg.agent_actor());
    if let Some(by) = requested_by {
        opening.push_str(&format!("requested_by: {by}  (the technician, not the customer)\n"));
    }
    log::info!("codex: starting operator-requested thread {} for {connection_string}", thread_id.key_string());
    runner::spawn(cfg, thread, Some(opening));
}

/// Claims the request and opens (or joins) the Codex thread for its machine.
pub async fn dispatch(req: AssistRequest) {
    let Some(cfg) = config() else { return };
    match AssistRequest::claim(&req.id).await {
        Ok(true) => {}
        Ok(false) => {
            log::info!("codex: request {} was no longer pending; not claimed", req.id.key_string());
            return;
        }
        Err(e) => {
            log::warn!("codex: claim failed for {}: {e}", req.id.key_string());
            return;
        }
    }
    let req = verified_requester(req);
    let opening = super::super::assist::compose_prompt(&req, &cfg.agent_actor());
    let _slot = admission(&req.connection_string).await;

    // A machine's working session takes every new request, fresh or not; nothing opens beside it.
    if !super::is_general(&req.connection_string) {
        match AgentThread::working_for_connection(&req.connection_string, None).await {
            Ok(Some(working)) => {
                join_working(&req, &working, &opening).await;
                return;
            }
            Ok(None) => {}
            Err(e) => log::warn!("codex: working-thread lookup failed for {}: {e}", req.connection_string),
        }
    }

    // A non-fresh request joins the machine's live thread when its requester may steer it; a busy thread queues it.
    if !req.fresh {
        match AgentThread::active_for_connection(&req.connection_string).await {
            Ok(Some(existing)) if may_join(&existing, &req).await => {
                log::info!(
                    "codex: request {} joins live thread {} for {}",
                    req.id.key_string(),
                    existing.id.key_string(),
                    req.connection_string
                );
                let _ = AssistRequest::link_thread(&req.id, &existing.id).await;
                let kind = if existing.is_busy() { "queue" } else { "start" };
                if let Err(e) = AgentTurn::ask(&existing.id, kind, &opening).await {
                    log::warn!("codex: could not queue the joining turn: {e}");
                }
                return;
            }
            Ok(Some(existing)) => {
                log::info!(
                    "codex: request {} opens its own session; its requester may not steer live thread {}",
                    req.id.key_string(),
                    existing.id.key_string()
                );
                release_idle_runners(&req.connection_string).await;
            }
            Ok(None) => {}
            Err(e) => log::warn!("codex: active-thread lookup failed for {}: {e}", req.connection_string),
        }
    } else {
        log::info!("codex: request {} asked for a fresh session", req.id.key_string());
        release_idle_runners(&req.connection_string).await;
    }

    let title = database::schema::agent_thread::session_title(&req);
    let new = NewAgentThread {
        assist_request: Some(req.id.clone()),
        connection_string: req.connection_string.clone(),
        hostname: req.hostname.clone(),
        service_number: req.service_number.clone(),
        store: req.store.clone(),
        requested_by: req.requested_by.clone(),
        service_order: req.service_order.clone(),
        computer: req.computer.clone(),
        customer: req.customer.clone(),
        model: Some(cfg.model_for(&req.connection_string).to_string()),
        provider: Some(cfg.provider.clone()),
        driven_by: Some(cfg.driven_by(&req.connection_string)),
        tool_path: Some("dynamic".to_string()),
        broker_node: Some(cfg.node.clone()),
        title,
    };
    let thread_id = match AgentThread::create(&new).await {
        Ok(id) => id,
        Err(e) => {
            log::warn!("codex: could not create agent_thread for {}: {e}", req.connection_string);
            let _ = AssistRequest::finish(&req.id, "failed", Some(e.to_string())).await;
            return;
        }
    };
    if let Err(e) = AssistRequest::link_thread(&req.id, &thread_id).await {
        log::warn!("codex: could not link request to thread: {e}");
    }
    let thread = match AgentThread::get(&thread_id).await {
        Ok(Some(t)) => t,
        _ => {
            log::warn!("codex: created thread {} but could not read it back", thread_id.key_string());
            return;
        }
    };
    if running_count() >= cfg.max_threads {
        log::info!("codex: pool busy; thread {} queued", thread_id.key_string());
        let _ = AgentThread::set_status(&thread_id, "queued", None).await;
        return;
    }
    log::info!("codex: starting thread {} for {}", thread_id.key_string(), req.connection_string);
    runner::spawn(cfg, thread, Some(opening));
}

/// Links the request to the machine's working session, or fails it when its requester may not steer that session.
async fn join_working(req: &AssistRequest, working: &AgentThread, opening: &str) {
    let key = working.id.key_string();
    if !may_join(working, req).await {
        log::info!("codex: request {} refused; thread {key} is working on {}", req.id.key_string(), req.connection_string);
        let why = format!("{}; stop or close it first", working_elsewhere(working));
        let _ = AssistRequest::finish(&req.id, "failed", Some(why)).await;
        return;
    }
    log::info!("codex: request {} joins working thread {key} for {}", req.id.key_string(), req.connection_string);
    if let Err(e) = AssistRequest::link_thread(&req.id, &working.id).await {
        log::warn!("codex: could not link request to thread: {e}");
    }
    if repeats_opening(req, working) {
        let who = req.requested_by.as_deref().unwrap_or("A technician");
        if let Some(tx) = runner_for(&key) {
            let _ = tx.send(RunnerCmd::Note(format!("{who} asked for AI help on this computer again; this session carries on."))).await;
        }
        return;
    }
    if let Err(e) = AgentTurn::ask(&working.id, "queue", opening).await {
        log::warn!("codex: could not queue the joining turn: {e}");
    }
}

/// The request with `requested_by` dropped when the database did not vouch for who filed it.
fn verified_requester(req: AssistRequest) -> AssistRequest {
    if req.requester_is_verified() {
        return req;
    }
    log::warn!(
        "codex: request {} was filed through '{}' access; ignoring its requested_by {:?}",
        req.id.key_string(),
        req.filed_access.as_deref().unwrap_or_default(),
        req.requested_by
    );
    AssistRequest { requested_by: None, ..req }
}

/// Whether the request's requester is the live thread's assignee or an active Root.
async fn may_join(thread: &AgentThread, req: &AssistRequest) -> bool {
    match AgentThread::may_steer(&thread.id, req.requested_by.as_deref()).await {
        Ok(allowed) => allowed,
        Err(e) => {
            log::warn!("codex: steer check failed for {}: {e}", thread.id.key_string());
            false
        }
    }
}

/// Asks the runners of a machine's open threads to free their pool slots while idle.
async fn release_idle_runners(connection_string: &str) {
    let threads = match AgentThread::open_for_connection(connection_string).await {
        Ok(threads) => threads,
        Err(e) => {
            log::warn!("codex: open-thread lookup failed for {connection_string}: {e}");
            return;
        }
    };
    for thread in threads {
        if let Some(tx) = runner_for(&thread.id.key_string()) {
            let _ = tx.send(RunnerCmd::Release).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use database::schema::{RecordId, AGENT_THREAD_TABLE};

    use super::*;

    fn request(trigger_source: &str, service_number: Option<&str>, note: Option<&str>) -> AssistRequest {
        AssistRequest {
            id: RecordId::new("assist_request", "r"),
            status: "dispatched".into(),
            trigger_source: trigger_source.into(),
            machine_confirmed: trigger_source == "tur_sheet",
            connection_string: "JeffsComputer:663a3fd40".into(),
            hostname: Some("JeffsComputer".into()),
            service_number: service_number.map(str::to_string),
            service_order: None,
            computer: None,
            customer: None,
            requested_by: Some("joshua.adams@pclaptops.com".into()),
            store: None,
            tech_note: note.map(str::to_string),
            agent: None,
            dispatch_error: None,
            agent_thread: None,
            fresh: true,
            filed_access: Some("user".into()),
        }
    }

    fn working(service_number: Option<&str>) -> AgentThread {
        AgentThread {
            id: RecordId::new(AGENT_THREAD_TABLE, "t"),
            status: "running".into(),
            connection_string: "JeffsComputer:663a3fd40".into(),
            hostname: Some("JeffsComputer".into()),
            service_number: service_number.map(str::to_string),
            store: None,
            requested_by: Some("joshua.adams@pclaptops.com".into()),
            assignee: None,
            assist_request: None,
            service_order: None,
            computer: None,
            customer: None,
            diagnostic_session: None,
            codex_thread_id: None,
            tools_hash: None,
            model: None,
            provider: None,
            driven_by: None,
            tool_path: None,
            title: None,
            error: None,
            broker_node: None,
            allow_box_shell: false,
            approve_all: None,
            tokens_used: None,
            tokens_window: None,
            last_seq: None,
            activity: None,
            created_at: None,
            updated_at: None,
            last_event_at: None,
            closed_at: None,
        }
    }

    #[test]
    fn a_repeated_bench_confirmation_adds_no_turn() {
        let confirm = request("tur_sheet", Some("2155485"), None);
        assert!(repeats_opening(&confirm, &working(Some("2155485"))));
        assert!(repeats_opening(&confirm, &working(None)));
    }

    #[test]
    fn a_new_order_or_a_typed_message_still_reaches_the_working_session() {
        let other_order = request("tur_sheet", Some("2155999"), None);
        assert!(!repeats_opening(&other_order, &working(Some("2155485"))));
        let typed = request("chat", Some("2155485"), Some("check the fans"));
        assert!(!repeats_opening(&typed, &working(Some("2155485"))));
    }

    #[test]
    fn the_refusal_names_the_working_session_and_its_technician() {
        assert_eq!(
            working_elsewhere(&working(Some("2155485"))),
            "another AI session (#2155485 JeffsComputer) is already working on this computer for joshua.adams@pclaptops.com"
        );
    }

    #[tokio::test]
    async fn one_request_per_machine_is_admitted_at_a_time() {
        let first = admission("A:1").await;
        let wait = Duration::from_millis(50);
        assert!(tokio::time::timeout(wait, admission("A:1")).await.is_err(), "a second request waits");
        assert!(tokio::time::timeout(wait, admission("B:2")).await.is_ok(), "another machine does not wait");
        drop(first);
        assert!(tokio::time::timeout(wait, admission("A:1")).await.is_ok(), "the slot frees with its guard");
    }
}
