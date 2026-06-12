//! Status gating on PrestaShop-legacy status ids (QCWizard `OrderBuilder`
//! lists). Shopify statuses carry the same ids as `legacy_id`, so one table
//! gates both backends.

use serde::{Deserialize, Serialize};

use super::OrderKind;

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

/// Xidax bench flow statuses: planned W7 ids (109 In QC / 43 Burn-in) plus
/// 71 "QC & Burn-in" — the id the live store actually carries (2026-06
/// `/statuses` capture; 109 absent there and 43 is a repair status).
pub const XIDAX_BENCH_STATUSES: &[i64] = &[109, 43, 71];

/// Xidax bench advance target on verdict pass: Preparing to Ship.
pub const XIDAX_BENCH_TARGET: i64 = 76;

/// Display names for the legacy ids this module references.
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
        43 => "Burn-in",
        45 => "Pending Payment",
        57 => "Build Pending",
        58 => "Pending Review",
        60 => "Building System",
        67 => "Preparing to Ship",
        70 => "Pre-Pulled",
        71 => "QC & Burn-in",
        73 => "Order Placed",
        76 => "Preparing to Ship",
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

pub fn status_display(legacy_id: i64, fallback: &str) -> String {
    let name = status_name(legacy_id);
    if !name.is_empty() {
        name.to_string()
    } else if !fallback.is_empty() {
        fallback.to_string()
    } else {
        format!("Status {legacy_id}")
    }
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
        assert_eq!(evaluate_shopify(109, "In QC").advance_target(), Some(76));
        assert_eq!(evaluate_shopify(43, "Burn-in").advance_target(), Some(76));
        assert!(evaluate_shopify(4, "Shipped").is_refused());
        assert_eq!(evaluate_shopify(73, "Order Placed").outcome, GateOutcome::Neutral);
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
