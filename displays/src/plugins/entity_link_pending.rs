//! Blocking entity-link resolution for MCP tools when the displays UI is active.

use database::schema::entity_link::LinkValidationIssue;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tokio::sync::oneshot;

static UI_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Call each frame from `SharedContext::receive` so MCP knows modals can run.
pub fn set_entity_link_ui_active(active: bool) {
    UI_ACTIVE.store(active, Ordering::Relaxed);
}

pub fn entity_link_ui_active() -> bool {
    UI_ACTIVE.load(Ordering::Relaxed)
}

type PendingMap = Mutex<HashMap<String, oneshot::Sender<EntityLinkOutcome>>>;

static PENDING: Lazy<PendingMap> = Lazy::new(|| Mutex::new(HashMap::new()));

static CHANNEL: Lazy<(
    crossbeam::channel::Sender<EntityLinkRequest>,
    crossbeam::channel::Receiver<EntityLinkRequest>,
)> = Lazy::new(crossbeam::channel::unbounded);

#[derive(Debug, Clone)]
pub struct EntityLinkRequest {
    pub request_id: String,
    pub connection_string: Option<String>,
    pub customer_id: String,
    pub computer_id: String,
    pub issues: Vec<LinkValidationIssue>,
}

#[derive(Debug, Clone)]
pub enum EntityLinkOutcome {
    Resolved {
        customer_id: String,
        computer_id: String,
    },
    Cancelled {
        reason: String,
    },
}

pub fn register_entity_link_resolution(
    request: EntityLinkRequest,
) -> oneshot::Receiver<EntityLinkOutcome> {
    let (tx, rx) = oneshot::channel();
    if let Ok(mut map) = PENDING.lock() {
        map.insert(request.request_id.clone(), tx);
    }
    let _ = CHANNEL.0.send(request);
    rx
}

pub fn resolve_entity_link_request(request_id: &str, outcome: EntityLinkOutcome) {
    if let Ok(mut map) = PENDING.lock() {
        if let Some(tx) = map.remove(request_id) {
            let _ = tx.send(outcome);
        }
    }
}

pub fn unregister_entity_link_request(request_id: &str) {
    if let Ok(mut map) = PENDING.lock() {
        map.remove(request_id);
    }
}

pub fn entity_link_request_receiver() -> &'static crossbeam::channel::Receiver<EntityLinkRequest> {
    &CHANNEL.1
}

/// Admin-console manual link (no MCP oneshot).
pub fn submit_manual_entity_link_request(mut request: EntityLinkRequest) {
    request.request_id = format!("manual-{}", uuid::Uuid::new_v4());
    let _ = CHANNEL.0.send(request);
}
