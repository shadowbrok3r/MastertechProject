//! Admin-side in-memory store for `Cmd::OpenServiceCandidatesResponse`
//! payloads, keyed by `connection_string`.
//!
//! Stage 3 of the OA3-driven service-binding feature: when the admin
//! opens a Web Console session to a client, the receive loop fires a
//! `Cmd::RequestOpenServiceCandidates { refresh: false }`.  The client
//! replies with the cached PrestaShop match + open service candidates
//! + a live `SystemInformation` snapshot, and that response lands here.
//!
//! Consumers:
//!   - `tabs::tasks::client_cards` renders a "Suggested service:" chip
//!     from this store (Stage 3).
//!   - The Stage-4 confirmation modal reads the full payload to drive
//!     the live-vs-presta merge preview.
//!
//! By design this store is **not persisted**.  Per the product
//! decision, transient suggestions live in memory only — a fresh app
//! launch refetches when the admin reconnects.

use database::schema::service_match::{OpenServiceCandidate, PrestashopCustomerMatch};
use database::schema::SystemInformation;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use web_time::Instant;

/// Process-wide store keyed by `connection_string`.  Written by the
/// admin's `WebSocketClient::receive` whenever a
/// `Cmd::OpenServiceCandidatesResponse` arrives; read by the
/// connected-client card and (later) the Stage-4 confirmation modal.
///
/// Free-function access rather than threading a channel through every
/// `WebSocketClient` instance — the same pattern as
/// `mcp_bridge::resolve_pending_request`.  Wrapped in `Mutex<HashMap>`
/// so the few microseconds of contention beat plumbing a `Sender`
/// through `AdminConsole → WebSocketClient → receive_loop`.
static SUGGESTIONS: OnceLock<Mutex<HashMap<String, OpenServiceSuggestion>>> = OnceLock::new();

fn store() -> &'static Mutex<HashMap<String, OpenServiceSuggestion>> {
    SUGGESTIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Replace the suggestion for `connection_string`.
pub fn put(connection_string: &str, value: OpenServiceSuggestion) {
    if let Ok(mut g) = store().lock() {
        g.insert(connection_string.to_string(), value);
    }
}

/// Read a snapshot of the suggestion for `connection_string` (cloned).
pub fn get(connection_string: &str) -> Option<OpenServiceSuggestion> {
    store().lock().ok().and_then(|g| g.get(connection_string).cloned())
}

/// Drain the store into the caller-provided `HashMap`.  Used by the
/// `SharedContext.open_service_suggestions` field which the UI cards
/// read from each frame (so cards don't need to lock the global on
/// every paint).
pub fn snapshot_into(out: &mut HashMap<String, OpenServiceSuggestion>) {
    if let Ok(g) = store().lock() {
        for (k, v) in g.iter() {
            out.insert(k.clone(), v.clone());
        }
    }
}

/// Snapshot of one `OpenServiceCandidatesResponse` plus when it
/// arrived (used by the card to render "cached 2 min ago" hints and by
/// the modal to decide whether to nudge a refresh before showing).
#[derive(Debug, Clone)]
pub struct OpenServiceSuggestion {
    pub match_: Option<PrestashopCustomerMatch>,
    pub candidates: Vec<OpenServiceCandidate>,
    pub live_specs: Option<SystemInformation>,
    pub received_at: Instant,
}

impl OpenServiceSuggestion {
    pub fn from_cmd(
        match_: Option<PrestashopCustomerMatch>,
        candidates: Vec<OpenServiceCandidate>,
        live_specs: Option<SystemInformation>,
    ) -> Self {
        Self {
            match_,
            candidates,
            live_specs,
            received_at: Instant::now(),
        }
    }

    /// `true` when the client confirmed it has a PrestaShop customer
    /// match — but says nothing about whether open candidates exist.
    pub fn has_customer(&self) -> bool {
        self.match_.is_some()
    }

    /// Convenient short label for the card chip: "#2100121 (Sales
    /// Order)" or "no open service" or "no customer match".
    pub fn primary_chip_label(&self) -> String {
        if self.match_.is_none() {
            return "no customer match".to_string();
        }
        if self.candidates.is_empty() {
            return "no open service".to_string();
        }
        let c = &self.candidates[0];
        format!("#{} ({})", c.service_number, c.doc_alias)
    }
}
