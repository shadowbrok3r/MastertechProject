//! Turns a tech-confirmed assist request into an agent_thread and a runner.

use database::schema::{AgentThread, AgentTurn, AssistRequest, NewAgentThread, RecordIdExt};

use super::{config, runner, runner_for, running_count, RunnerCmd};

/// Opens a session for a machine directly, with the same opening prompt a request would carry.
pub async fn start_for_connection(cfg: std::sync::Arc<super::Config>, connection_string: &str, requested_by: Option<&str>) {
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
        model: Some(cfg.model.clone()),
        provider: Some(cfg.provider.clone()),
        driven_by: Some(cfg.driven_by()),
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
        Ok(false) => return,
        Err(e) => {
            log::warn!("codex: claim failed for {}: {e}", req.id.key_string());
            return;
        }
    }
    let req = verified_requester(req);
    let opening = super::super::assist::compose_prompt(&req, &cfg.agent_actor());

    // A request that is not fresh joins the machine's live thread when its requester may steer it.
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
                if let Err(e) = AgentTurn::ask(&existing.id, "start", &opening).await {
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

    let title = if super::is_general(&req.connection_string) {
        Some(format!("General \u{00b7} {}", req.requested_by.clone().unwrap_or_else(|| "technician".into())))
    } else {
        match (&req.service_number, &req.hostname) {
            (Some(sn), Some(host)) => Some(format!("#{sn} {host}")),
            (Some(sn), None) => Some(format!("#{sn}")),
            (None, Some(host)) => Some(host.clone()),
            (None, None) => None,
        }
    };
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
        model: Some(cfg.model.clone()),
        provider: Some(cfg.provider.clone()),
        driven_by: Some(cfg.driven_by()),
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
