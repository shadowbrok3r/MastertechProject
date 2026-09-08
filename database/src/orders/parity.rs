//! Shadow comparison between the PrestaShop and Shopify backends.
//!
//! During the overlap period the same order exists in both systems. This
//! module fetches it from each and diffs the fields Mastertech actually
//! renders, so differences are found by running the comparison rather than by
//! a tech noticing a wrong number on the bench.
//!
//! Comparison is deliberately field-by-field and string-based: the point is to
//! see *what* differs, not to assert the two structs are equal. Values are
//! normalised (case, whitespace, money, `#` prefixes) so cosmetic formatting
//! does not drown the real differences.

use super::{BuildSpec, OrderBackend, OrderKey, PrestashopBackend, QcOrder, ShopifyBackend};
use serde::{Deserialize, Serialize};

/// How a field compared across the two backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Verdict {
    /// Both sides agree after normalisation.
    Match,
    /// Both sides have a value and they differ.
    Differs,
    /// Exactly one side has a value.
    OnlyOneSide,
    /// Neither side has a value; nothing to compare.
    BothEmpty,
}

impl Verdict {
    /// True for anything a human needs to look at.
    pub fn needs_review(self) -> bool {
        matches!(self, Self::Differs | Self::OnlyOneSide)
    }
}

/// One field's comparison.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldDiff {
    pub field: String,
    pub prestashop: String,
    pub shopify: String,
    pub verdict: Verdict,
}

/// Full comparison for one order.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ParityReport {
    pub key: String,
    pub ps_id: String,
    pub shopify_gid: String,
    pub fields: Vec<FieldDiff>,
    /// Populated when one side could not be fetched at all.
    pub fetch_errors: Vec<String>,
}

impl ParityReport {
    pub fn mismatches(&self) -> impl Iterator<Item = &FieldDiff> {
        self.fields.iter().filter(|f| f.verdict.needs_review())
    }

    /// True when both sides were fetched and every field agrees.
    pub fn is_clean(&self) -> bool {
        self.fetch_errors.is_empty() && self.mismatches().next().is_none()
    }

    /// One line per field needing review, for a terminal or a log.
    pub fn summary(&self) -> String {
        if !self.fetch_errors.is_empty() {
            return format!("{}: {}", self.key, self.fetch_errors.join("; "));
        }
        let mismatches: Vec<&FieldDiff> = self.mismatches().collect();
        if mismatches.is_empty() {
            return format!("{}: clean ({} fields compared)", self.key, self.fields.len());
        }
        let mut out = format!("{}: {} of {} fields differ\n", self.key, mismatches.len(), self.fields.len());
        for f in mismatches {
            out.push_str(&format!(
                "  {:<18} ps={:<30} shopify={}\n",
                f.field,
                truncate(&f.prestashop, 30),
                truncate(&f.shopify, 40)
            ));
        }
        out
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    format!("{}…", s.chars().take(max.saturating_sub(1)).collect::<String>())
}

/// Lowercase, collapse internal whitespace, drop a leading `#`.
fn norm(value: &str) -> String {
    value
        .trim()
        .trim_start_matches('#')
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// `"1234.5600"` and `"1,234.56"` are the same amount. Non-numeric input
/// falls back to [`norm`].
fn norm_money(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    match cleaned.parse::<f64>() {
        Ok(n) => format!("{n:.2}"),
        Err(_) => norm(value),
    }
}

fn diff_with(field: &str, ps: &str, shopify: &str, normalise: fn(&str) -> String) -> FieldDiff {
    let (a, b) = (normalise(ps), normalise(shopify));
    let verdict = match (a.is_empty(), b.is_empty()) {
        (true, true) => Verdict::BothEmpty,
        (false, false) if a == b => Verdict::Match,
        (false, false) => Verdict::Differs,
        _ => Verdict::OnlyOneSide,
    };
    FieldDiff {
        field: field.to_string(),
        prestashop: ps.to_string(),
        shopify: shopify.to_string(),
        verdict,
    }
}

fn diff(field: &str, ps: &str, shopify: &str) -> FieldDiff {
    diff_with(field, ps, shopify, norm)
}

/// Compare the fields Mastertech renders on an order.
pub fn compare_orders(ps: &QcOrder, shopify: &QcOrder) -> Vec<FieldDiff> {
    let mut fields = vec![
        diff("reference", &ps.reference, &shopify.reference),
        diff("customer_name", &ps.customer_name, &shopify.customer_name),
        diff("kind", ps.kind.as_str(), shopify.kind.as_str()),
        diff(
            "status_legacy_id",
            &ps.status.legacy_id.to_string(),
            &shopify.status.legacy_id.to_string(),
        ),
        diff("status_name", &ps.status.name, &shopify.status.name),
        diff_with("total_paid", &ps.total_paid, &shopify.total_paid, norm_money),
        diff(
            "build_serial",
            ps.build_serial.as_deref().unwrap_or_default(),
            shopify.build_serial.as_deref().unwrap_or_default(),
        ),
        diff("note", ps.note.as_deref().unwrap_or_default(), shopify.note.as_deref().unwrap_or_default()),
        diff(
            "item_count",
            &ps.items.len().to_string(),
            &shopify.items.len().to_string(),
        ),
    ];

    // Line items are matched on SKU, not position: the two backends have no
    // reason to return them in the same order.
    let mut skus: Vec<String> = ps
        .items
        .iter()
        .chain(shopify.items.iter())
        .map(|i| norm(&i.reference))
        .filter(|s| !s.is_empty())
        .collect();
    skus.sort();
    skus.dedup();

    for sku in skus {
        let ps_item = ps.items.iter().find(|i| norm(&i.reference) == sku);
        let shop_item = shopify.items.iter().find(|i| norm(&i.reference) == sku);
        fields.push(diff(
            &format!("item[{sku}].qty"),
            &ps_item.map(|i| i.quantity.to_string()).unwrap_or_default(),
            &shop_item.map(|i| i.quantity.to_string()).unwrap_or_default(),
        ));
        fields.push(diff(
            &format!("item[{sku}].serials"),
            &joined_serials(ps_item),
            &joined_serials(shop_item),
        ));
    }
    fields
}

fn joined_serials(item: Option<&super::QcOrderItem>) -> String {
    let Some(item) = item else { return String::new() };
    let mut serials: Vec<String> = item.serials.iter().map(|s| norm(s)).filter(|s| !s.is_empty()).collect();
    serials.sort();
    serials.join(",")
}

/// Compare the build spec QC checks a machine against.
pub fn compare_specs(ps: &BuildSpec, shopify: &BuildSpec) -> Vec<FieldDiff> {
    let mut fields = vec![
        diff("spec.model", &ps.model, &shopify.model),
        diff("spec.cpu", &ps.cpu, &shopify.cpu),
        diff("spec.gpu", &ps.gpu, &shopify.gpu),
        diff("spec.ram", &ps.ram, &shopify.ram),
        diff(
            "spec.motherboard",
            ps.motherboard.as_deref().unwrap_or_default(),
            shopify.motherboard.as_deref().unwrap_or_default(),
        ),
        diff("spec.os", ps.os.as_deref().unwrap_or_default(), shopify.os.as_deref().unwrap_or_default()),
        diff(
            "spec.drive_count",
            &ps.drives.len().to_string(),
            &shopify.drives.len().to_string(),
        ),
    ];
    // Drives compare as a sorted set: order carries no meaning.
    fields.push(diff("spec.drives", &joined_drives(ps), &joined_drives(shopify)));
    fields
}

fn joined_drives(spec: &BuildSpec) -> String {
    let mut drives: Vec<String> = spec
        .drives
        .iter()
        .map(|d| norm(&format!("{} {}", d.kind, d.name)))
        .filter(|s| !s.trim().is_empty())
        .collect();
    drives.sort();
    drives.join(" | ")
}

/// Fetch one order from both backends and diff it.
///
/// `ps_key` and `shopify_key` name the same order in each id space — take them
/// from `order_identity`, or from the Shopify order's `parent_order_id`, which
/// carries the PrestaShop `id_order`.
pub async fn compare(ps_key: &OrderKey, shopify_key: &OrderKey) -> ParityReport {
    let mut report = ParityReport {
        key: format!("{} / {}", ps_key.display(), shopify_key.display()),
        ..Default::default()
    };

    let ps_backend = PrestashopBackend::new();
    let shopify_backend = ShopifyBackend::from_env();

    let (ps_order, shopify_order) = futures::join!(
        ps_backend.find_order(ps_key),
        shopify_backend.find_order(shopify_key)
    );

    let ps_order = match ps_order {
        Ok(o) => o,
        Err(e) => {
            report.fetch_errors.push(format!("prestashop fetch failed: {e}"));
            return report;
        }
    };
    let shopify_order = match shopify_order {
        Ok(o) => o,
        Err(e) => {
            report.fetch_errors.push(format!("shopify fetch failed: {e}"));
            return report;
        }
    };

    report.ps_id = ps_order.id.clone();
    report.shopify_gid = shopify_order.gid.clone().unwrap_or_default();
    report.fields = compare_orders(&ps_order, &shopify_order);

    // A spec that fails to build is a finding, not a reason to abandon the
    // report — the order-level diff above is still worth having.
    match futures::join!(
        ps_backend.build_spec(&ps_order),
        shopify_backend.build_spec(&shopify_order)
    ) {
        (Ok(ps_spec), Ok(shopify_spec)) => {
            report.fields.extend(compare_specs(&ps_spec, &shopify_spec));
        }
        (ps_spec, shopify_spec) => {
            if let Err(e) = ps_spec {
                report.fetch_errors.push(format!("prestashop build_spec failed: {e}"));
            }
            if let Err(e) = shopify_spec {
                report.fetch_errors.push(format!("shopify build_spec failed: {e}"));
            }
        }
    }
    report
}

/// Compare a Shopify order against the PrestaShop order it was migrated from,
/// using the `legacyPs` id the Shopify order carries. `None` when the order
/// has no PrestaShop ancestor, which is the normal case for a Shopify-native
/// order and not an error.
pub async fn compare_against_legacy(shopify_key: &OrderKey) -> anyhow::Result<Option<ParityReport>> {
    let shopify_backend = ShopifyBackend::from_env();
    let order = shopify_backend.find_order(shopify_key).await?;
    let Some(legacy) = order
        .parent_order_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return Ok(None);
    };
    let ps_key = OrderKey::Prestashop(legacy.to_string());
    Ok(Some(compare(&ps_key, shopify_key).await))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orders::{BackendKind, OrderKind, QcOrderItem, StatusInfo};

    fn order(reference: &str, customer: &str, total: &str) -> QcOrder {
        QcOrder {
            backend: Some(BackendKind::Prestashop),
            reference: reference.into(),
            customer_name: customer.into(),
            total_paid: total.into(),
            kind: OrderKind::Sales,
            status: StatusInfo { legacy_id: 71, name: "QC & Burn-in".into() },
            ..Default::default()
        }
    }

    fn item(sku: &str, qty: f64, serials: &[&str]) -> QcOrderItem {
        QcOrderItem {
            reference: sku.into(),
            quantity: qty,
            serials: serials.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn cosmetic_formatting_is_not_a_mismatch() {
        // `#1042` vs `1042`, case, and padded money must all compare equal.
        let ps = order("1042", "Jane Doe", "1234.5600");
        let shopify = order("#1042", "  jane   doe ", "1,234.56");
        let fields = compare_orders(&ps, &shopify);
        let bad: Vec<&str> = fields
            .iter()
            .filter(|f| f.verdict.needs_review())
            .map(|f| f.field.as_str())
            .collect();
        assert!(bad.is_empty(), "unexpected mismatches: {bad:?}");
    }

    #[test]
    fn a_real_difference_is_caught() {
        let ps = order("1042", "Jane Doe", "1234.56");
        let mut shopify = order("#1042", "Jane Doe", "1299.99");
        shopify.status = StatusInfo { legacy_id: 67, name: "Preparing to Ship".into() };
        let fields = compare_orders(&ps, &shopify);
        let bad: Vec<&str> = fields
            .iter()
            .filter(|f| f.verdict.needs_review())
            .map(|f| f.field.as_str())
            .collect();
        assert!(bad.contains(&"total_paid"), "{bad:?}");
        assert!(bad.contains(&"status_legacy_id"), "{bad:?}");
        assert!(bad.contains(&"status_name"), "{bad:?}");
    }

    #[test]
    fn line_items_match_on_sku_not_position() {
        let mut ps = order("1042", "Jane Doe", "0");
        ps.items = vec![item("CPU-9950X", 1.0, &["SN-A"]), item("GPU-5080", 1.0, &["SN-B"])];
        let mut shopify = order("1042", "Jane Doe", "0");
        shopify.items = vec![item("GPU-5080", 1.0, &["SN-B"]), item("CPU-9950X", 1.0, &["SN-A"])];

        let fields = compare_orders(&ps, &shopify);
        let bad: Vec<&str> = fields
            .iter()
            .filter(|f| f.verdict.needs_review())
            .map(|f| f.field.as_str())
            .collect();
        assert!(bad.is_empty(), "reordered items should match: {bad:?}");
    }

    #[test]
    fn a_serial_on_one_side_only_is_flagged() {
        let mut ps = order("1042", "Jane Doe", "0");
        ps.items = vec![item("CPU-9950X", 1.0, &["SN-A"])];
        let mut shopify = order("1042", "Jane Doe", "0");
        shopify.items = vec![item("CPU-9950X", 1.0, &[])];

        let fields = compare_orders(&ps, &shopify);
        let serial = fields
            .iter()
            .find(|f| f.field == "item[cpu-9950x].serials")
            .expect("serial field compared");
        assert_eq!(serial.verdict, Verdict::OnlyOneSide);
    }

    #[test]
    fn an_item_missing_from_one_side_is_flagged() {
        let mut ps = order("1042", "Jane Doe", "0");
        ps.items = vec![item("CPU-9950X", 1.0, &[]), item("RAM-64", 2.0, &[])];
        let mut shopify = order("1042", "Jane Doe", "0");
        shopify.items = vec![item("CPU-9950X", 1.0, &[])];

        let fields = compare_orders(&ps, &shopify);
        let qty = fields
            .iter()
            .find(|f| f.field == "item[ram-64].qty")
            .expect("missing item compared");
        assert_eq!(qty.verdict, Verdict::OnlyOneSide);
        let count = fields.iter().find(|f| f.field == "item_count").unwrap();
        assert_eq!(count.verdict, Verdict::Differs);
    }

    #[test]
    fn drives_compare_as_a_set() {
        let ps = BuildSpec {
            drives: vec![
                super::super::DriveSpec { name: "2TB NVMe".into(), kind: "ssd".into(), ..Default::default() },
                super::super::DriveSpec { name: "4TB HDD".into(), kind: "hdd".into(), ..Default::default() },
            ],
            ..Default::default()
        };
        let shopify = BuildSpec {
            drives: vec![
                super::super::DriveSpec { name: "4TB HDD".into(), kind: "hdd".into(), ..Default::default() },
                super::super::DriveSpec { name: "2TB NVMe".into(), kind: "ssd".into(), ..Default::default() },
            ],
            ..Default::default()
        };
        let fields = compare_specs(&ps, &shopify);
        assert!(
            fields.iter().all(|f| !f.verdict.needs_review()),
            "reordered drives should match"
        );
    }

    #[test]
    fn both_empty_is_not_a_mismatch() {
        let ps = order("1042", "Jane Doe", "0");
        let shopify = order("1042", "Jane Doe", "0");
        let fields = compare_orders(&ps, &shopify);
        let build_serial = fields.iter().find(|f| f.field == "build_serial").unwrap();
        assert_eq!(build_serial.verdict, Verdict::BothEmpty);
        assert!(!build_serial.verdict.needs_review());
    }

    #[test]
    fn summary_names_only_the_fields_that_differ() {
        let ps = order("1042", "Jane Doe", "1234.56");
        let shopify = order("1042", "Jane Doe", "1299.99");
        let report = ParityReport {
            key: "212345 / #1042".into(),
            fields: compare_orders(&ps, &shopify),
            ..Default::default()
        };
        assert!(!report.is_clean());
        let summary = report.summary();
        assert!(summary.contains("total_paid"), "{summary}");
        assert!(!summary.contains("customer_name"), "{summary}");
    }
}
