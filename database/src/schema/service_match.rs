//! Shared wire types for the open-service-order auto-link flow.
//!
//! The client (Mastertech4.0) resolves a customer via the OA3 product
//! key against PrestaShop, fetches that customer's *open* service
//! orders (anything whose state is not `AcceptedByOdoo`), and caches
//! the result in process memory.  The admin (displays) pulls the
//! cached result via a `Cmd::RequestOpenServiceCandidates` over the
//! existing transport when it wants to surface the suggestion modal.
//!
//! These types live in `database` rather than next to the client-side
//! lookup so the `displays::Cmd` enum (also a workspace-shared wire
//! format) can reference them without a cyclic crate dep through
//! `Mastertech4.0`.

use crate::SurrealValue;
use serde::{Deserialize, Serialize};

/// A successful PrestaShop customer match.  The client populates this
/// after the OA3 13-digit lookup resolves the customer; the admin reads
/// it as the authoritative customer linkage suggestion.
///
/// **Not persisted on its own** — the admin-side confirmation modal is
/// what eventually writes the customer FK onto `connected_client` (via
/// the existing relink popup pattern).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, SurrealValue)]
pub struct PrestashopCustomerMatch {
    /// `"FirstName LastName - OrderID"` — backwards-compatible with
    /// the pre-existing `lookup_customer_by_serial` return value, kept
    /// as a single string so the admin card can display it without
    /// reformatting.
    pub friendly_name: String,
    /// PrestaShop customer id (e.g. `"30412"`).  Numeric in PrestaShop
    /// but stored as a string to dodge integer overflow risk on long
    /// migration histories.
    pub id_customer: String,
    /// The PrestaShop order from which we resolved the customer (the
    /// one whose `order_serial` matched the OA3 serial).  Recorded so
    /// the confirm modal can highlight it among multiple open
    /// candidates.
    pub id_order: String,
    pub first_name: String,
    pub last_name: String,
}

/// One open service order suggested for binding to a connected client.
///
/// Filtered out by the client before this struct is emitted:
/// `AcceptedByOdoo` (i.e. the order is done from the shop's point of
/// view).  Everything else surfaces to the admin for picking.
///
/// `specs` carries what the client parsed out of the PrestaShop order
/// body so the admin-side confirmation modal can show "live vs
/// PrestaShop" per-field when computer-row creation comes up for
/// approval.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, SurrealValue)]
pub struct OpenServiceCandidate {
    pub service_number: String,
    pub doc_alias: String,
    pub date_add: String,
    pub date_upd: String,
    pub checkin_notes: String,
    /// Decoded PrestaShop state name (e.g. `"In Repair"`, `"Order
    /// Placed"`).  Stored as String rather than the `OrderState` enum
    /// so the wire format stays stable even if new states get added.
    pub state_name: String,
    pub state_id: String,
    pub specs: PrestaSpecsSnapshot,
}

/// PrestaShop-parsed computer specs.  All fields are best-effort; an
/// empty string means "PrestaShop didn't have that field" and the
/// admin-side merge should prefer the live `ComputerData` value.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, SurrealValue)]
pub struct PrestaSpecsSnapshot {
    pub cpu: String,
    pub gpu: String,
    pub ram: String,
    pub device_serial: String,
    pub device_mfg: String,
    pub device_model: String,
    pub motherboard_name: String,
    pub operating_system: String,
    /// `(drive_letter_or_label, drive_type)` — same shape as
    /// `Order::extract_drives()`.
    pub drives: Vec<(String, String)>,
}
