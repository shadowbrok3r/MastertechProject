//! Status gating on PrestaShop-legacy status ids (QCWizard `OrderBuilder`
//! lists). Shopify statuses carry the same ids as `legacy_id`, so one table
//! gates both backends.

use serde::{Deserialize, Serialize};

use super::OrderKind;

pub use crate::schema::prestashop::OPEN_ORDER_STATES;

/// Sales statuses a bench QC session may pull forward (QCWizard good-to-move).
pub const SALES_GOOD_TO_MOVE: &[i64] = &[73, 60, 225, 224, 70, 57, 98, 103];

/// Sales statuses that block QC outright (QCWizard refuse-to-move).
pub const SALES_REFUSE_TO_MOVE: &[i64] = &[4, 6, 58];

/// Repair statuses a bench QC session may pull forward.
pub const REPAIR_GOOD_TO_MOVE: &[i64] = &[25, 58];

/// States `advance_status` refuses to leave (QCWizard `UpdateOrderState`).
pub const UPDATE_BLOCKED_STATES: &[i64] = &[4, 6, 45, 58];

/// Sales advance target on gate pass: QC & Burn-in.
pub const SALES_QC_TARGET: i64 = 71;

/// Repair advance target on gate pass: Service Begun.
pub const REPAIR_QC_TARGET: i64 = 26;

/// Xidax bench flow status: 71 "QC & Burn-in" (live `/statuses` capture
/// 2026-06). The plan's 109 In QC / 43 Burn-in don't exist on the store —
/// 109 is absent and 43 is "Replacement Part Received - Repair Underway".
pub const XIDAX_BENCH_STATUSES: &[i64] = &[71];

/// Xidax bench advance target on verdict pass: 67 "Preparing to Ship".
/// (The plan's 76 is absent on the live store.)
pub const XIDAX_BENCH_TARGET: i64 = 67;

/// Compiled fallback names for the legacy ids this module references. The
/// live table is `status_catalog`; this covers only what was hand-maintained.
pub fn status_name(legacy_id: i64) -> &'static str {
    match legacy_id {
        2 => "Payment Accepted",
        3 => "Processing",
        4 => "Shipped",
        5 => "Delivered",
        6 => "Canceled",
        16 => "Quote",
        22 => "Completed",
        25 => "System Received",
        26 => "Service Begun",
        29 => "Check-in Shelf",
        30 => "In Repair",
        31 => "In Repair - Remote",
        40 => "Done Shelf",
        43 => "Replacement Part Received - Repair Underway (Remote)",
        45 => "Pending Payment",
        57 => "Build Pending",
        58 => "Pending Review",
        60 => "Building System",
        67 => "Preparing to Ship",
        70 => "Pre-Pulled",
        71 => "QC & Burn-in",
        73 => "Order Placed",
        80 => "Pulled",
        82 => "Ready to Pull",
        84 => "Returned",
        98 => "Online Customer Payment Received",
        103 => "X-10 Build Pending",
        104 => "Fraud Flagged",
        109 => "In QC",
        224 => "Ready to Build (On Warehouse Floor)",
        225 => "Ready to Build",
        228 => "Debuild",
        233 => "In Stock For Sale",
        234 => "Sold",
        236 => "Ship To Store",
        238 => "Delivered To Store",
        239 => "Accepted By Odoo",
        241 => "Out of Stock",
        242 => "Odoo Pending Review",
        _ => "",
    }
}

/// `live_name` (from the backend payload) is authoritative; the static map is
/// the fallback for callers that only have a legacy id (PCL gate evaluation).
pub fn status_display(legacy_id: i64, live_name: &str) -> String {
    if !live_name.is_empty() {
        return live_name.to_string();
    }
    // Runtime catalog first; the table below is a fallback for an unloaded
    // catalog, and covers 37 of the 126 statuses PrestaShop actually defines.
    if let Some(name) = super::status_catalog::name(legacy_id) {
        return name;
    }
    let name = status_name(legacy_id);
    if !name.is_empty() {
        name.to_string()
    } else {
        format!("Status {legacy_id}")
    }
}

/// True for the pre-build intake status *names* the bench pulls from:
/// "Order Placed" (73) and "Ready to Build" (224 warehouse-floor / 225).
/// Queue rows carry status names, not legacy ids, so the match is by name.
pub fn is_build_intake_status(name: &str) -> bool {
    let trimmed = name.trim();
    trimmed.eq_ignore_ascii_case("Order Placed")
        || trimmed.to_ascii_lowercase().starts_with("ready to build")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GateOutcome {
    /// QC may proceed and the order may advance to `advance_to`.
    GoodToMove { advance_to: i64 },
    /// QC must not touch this order.
    RefuseToMove,
    /// Status outside both lists: proceed read-only, no auto-advance.
    Neutral,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateDecision {
    pub outcome: GateOutcome,
    pub status_legacy_id: i64,
    pub status_name: String,
    pub message: String,
}

impl GateDecision {
    pub fn is_refused(&self) -> bool {
        self.outcome == GateOutcome::RefuseToMove
    }

    pub fn advance_target(&self) -> Option<i64> {
        match self.outcome {
            GateOutcome::GoodToMove { advance_to } => Some(advance_to),
            _ => None,
        }
    }
}

/// Evaluate the QCWizard gate lists for a PCL (PrestaShop) order.
pub fn evaluate_prestashop(kind: OrderKind, status_legacy_id: i64, status_name_hint: &str) -> GateDecision {
    let name = status_display(status_legacy_id, status_name_hint);
    let (good, refuse, target) = match kind {
        OrderKind::Repair => (REPAIR_GOOD_TO_MOVE, &[][..], REPAIR_QC_TARGET),
        _ => (SALES_GOOD_TO_MOVE, SALES_REFUSE_TO_MOVE, SALES_QC_TARGET),
    };

    if refuse.contains(&status_legacy_id) {
        return GateDecision {
            outcome: GateOutcome::RefuseToMove,
            status_legacy_id,
            message: format!("System is in {name}! Check PrestaShop and verify."),
            status_name: name,
        };
    }
    if good.contains(&status_legacy_id) {
        let target_name = status_display(target, "");
        return GateDecision {
            outcome: GateOutcome::GoodToMove { advance_to: target },
            status_legacy_id,
            message: format!("{name} — clear for QC, advances to {target_name} ({target})."),
            status_name: name,
        };
    }
    GateDecision {
        outcome: GateOutcome::Neutral,
        status_legacy_id,
        message: format!("{name} — outside the QC gate lists; read-only, no auto-advance."),
        status_name: name,
    }
}

/// Evaluate the Xidax bench gate: bench QC operates inside In QC / Burn-in
/// and requests Preparing to Ship on pass. Refuse list matches PCL.
pub fn evaluate_shopify(status_legacy_id: i64, status_name_hint: &str) -> GateDecision {
    let name = status_display(status_legacy_id, status_name_hint);
    if SALES_REFUSE_TO_MOVE.contains(&status_legacy_id) {
        return GateDecision {
            outcome: GateOutcome::RefuseToMove,
            status_legacy_id,
            message: format!("Order is {name}! Check the build app and verify."),
            status_name: name,
        };
    }
    if XIDAX_BENCH_STATUSES.contains(&status_legacy_id) {
        let target_name = status_display(XIDAX_BENCH_TARGET, "");
        return GateDecision {
            outcome: GateOutcome::GoodToMove { advance_to: XIDAX_BENCH_TARGET },
            status_legacy_id,
            message: format!("{name} — bench QC window; pass advances to {target_name} ({XIDAX_BENCH_TARGET}) via the Worker."),
            status_name: name,
        };
    }
    GateDecision {
        outcome: GateOutcome::Neutral,
        status_legacy_id,
        message: format!("{name} — outside the bench QC window; read-only."),
        status_name: name,
    }
}

/// True when `advance_status` may move an order off `from_legacy_id`.
pub fn update_allowed(from_legacy_id: i64, to_legacy_id: i64) -> Result<(), String> {
    if UPDATE_BLOCKED_STATES.contains(&from_legacy_id) {
        return Err(format!(
            "Refusing to move order out of {} ({from_legacy_id}).",
            status_display(from_legacy_id, "")
        ));
    }
    if from_legacy_id == to_legacy_id {
        return Err("Order is already in the requested state.".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod catalog_tests {
    use super::*;
    use crate::orders::status_catalog;

    // Mutates the process-global catalog; kept in its own module so the gate
    // policy tests never observe a half-installed table.
    #[test]
    fn catalog_beats_the_compiled_table_and_covers_its_gaps() {
        status_catalog::clear();

        // 217 "Preparing to Ship" is a real PrestaShop state the compiled
        // table never knew — it used to render as "Status 217".
        assert_eq!(status_display(217, ""), "Status 217");
        // 40 is in the compiled table under an abbreviated name.
        assert_eq!(status_display(40, ""), "Done Shelf");

        status_catalog::install(std::collections::HashMap::from([
            (217, "Preparing to Ship".to_string()),
            (40, "Done Shelf (Ready for Pickup)".to_string()),
        ]));

        assert_eq!(status_display(217, ""), "Preparing to Ship");
        assert_eq!(status_display(40, "Done Shelf (Ready for Pickup)"), "Done Shelf (Ready for Pickup)");
        // An id in neither still degrades to the id, never to a wrong name.
        assert_eq!(status_display(99999, ""), "Status 99999");
        // A live name from the order itself outranks both.
        assert_eq!(status_display(40, "Whatever The Order Said"), "Whatever The Order Said");

        status_catalog::clear();
        assert_eq!(status_display(217, ""), "Status 217");
    }

    #[test]
    fn open_order_states_are_all_nameable() {
        for id in OPEN_ORDER_STATES {
            assert!(!status_name(*id).is_empty(), "no compiled name for open state {id}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sales_gate_matches_qcwizard_lists() {
        for id in SALES_GOOD_TO_MOVE {
            let d = evaluate_prestashop(OrderKind::Sales, *id, "");
            assert_eq!(d.advance_target(), Some(SALES_QC_TARGET), "id {id}");
        }
        for id in SALES_REFUSE_TO_MOVE {
            let d = evaluate_prestashop(OrderKind::Sales, *id, "");
            assert!(d.is_refused(), "id {id}");
        }
        // 71 itself is neither good nor refused: re-running QC is allowed.
        let d = evaluate_prestashop(OrderKind::Sales, 71, "");
        assert_eq!(d.outcome, GateOutcome::Neutral);
    }

    #[test]
    fn repair_gate_uses_repair_lists() {
        let d = evaluate_prestashop(OrderKind::Repair, 25, "");
        assert_eq!(d.advance_target(), Some(REPAIR_QC_TARGET));
        // 58 refuses sales orders but is good-to-move for repairs.
        let d = evaluate_prestashop(OrderKind::Repair, 58, "");
        assert_eq!(d.advance_target(), Some(REPAIR_QC_TARGET));
        let d = evaluate_prestashop(OrderKind::Sales, 58, "");
        assert!(d.is_refused());
    }

    #[test]
    fn xidax_bench_gate() {
        // 71 "QC & Burn-in" is the live bench status; pass advances to 67.
        assert_eq!(evaluate_shopify(71, "QC & Burn-in").advance_target(), Some(67));
        // 43 is a repair-remote status on the live store, NOT a bench status.
        assert_eq!(evaluate_shopify(43, "Replacement Part Received").outcome, GateOutcome::Neutral);
        assert!(evaluate_shopify(4, "Shipped").is_refused());
        assert_eq!(evaluate_shopify(73, "Order Placed").outcome, GateOutcome::Neutral);
    }

    #[test]
    fn live_name_overrides_static_map() {
        // The backend's authoritative name wins over the fallback map.
        assert_eq!(status_display(43, "Replacement Part Received - Repair Underway (Remote)"),
                   "Replacement Part Received - Repair Underway (Remote)");
        // With no live name, the map is the fallback (PCL gate path).
        assert_eq!(status_display(73, ""), "Order Placed");
        assert_eq!(status_display(99999, ""), "Status 99999");
    }

    #[test]
    fn build_intake_status_matches_order_placed_and_ready_to_build() {
        assert!(is_build_intake_status("Order Placed"));
        assert!(is_build_intake_status("order placed"));
        assert!(is_build_intake_status("Ready to Build"));
        assert!(is_build_intake_status("Ready to Build (On Warehouse Floor)"));
        // Distinct re-queued status (229) is not bare "Order Placed".
        assert!(!is_build_intake_status("Order Placed (re-queued)"));
        assert!(!is_build_intake_status("Shipped"));
        assert!(!is_build_intake_status("QC & Burn-in"));
    }

    #[test]
    fn update_block_list_matches_qcwizard() {
        assert!(update_allowed(4, 71).is_err());
        assert!(update_allowed(6, 71).is_err());
        assert!(update_allowed(45, 71).is_err());
        assert!(update_allowed(58, 71).is_err());
        assert!(update_allowed(71, 71).is_err());
        assert!(update_allowed(73, 71).is_ok());
    }
}
