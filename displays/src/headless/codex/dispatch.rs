//! Turns a tech-confirmed assist request into an agent_thread and a runner.

use database::schema::{AgentThread, AgentTurn, AssistRequest, NewAgentThread, RecordIdExt};

use super::{config, runner, running_count};

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
    let mut opening = format!("Check this computer: {connection_string}\n");
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
    let opening = super::super::assist::compose_prompt(&req);

    // One live thread per machine: a second request joins it as a new turn.
    match AgentThread::active_for_connection(&req.connection_string).await {
        Ok(Some(existing)) => {
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
        Ok(None) => {}
        Err(e) => log::warn!("codex: active-thread lookup failed for {}: {e}", req.connection_string),
    }

    let title = match (&req.service_number, &req.hostname) {
        (Some(sn), Some(host)) => Some(format!("#{sn} {host}")),
        (Some(sn), None) => Some(format!("#{sn}")),
        (None, Some(host)) => Some(host.clone()),
        (None, None) => None,
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
