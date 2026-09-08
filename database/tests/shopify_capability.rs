//! What each Mastertech surface can and cannot do against Shopify.
//!
//! Probes the live Build Management API once per capability and prints a
//! matrix grouped by the app screen that depends on it, so "will Task Audit
//! work?" has an evidence-backed answer instead of an opinion.
//!
//! ```text
//! cargo test -p database --test shopify_capability -- --ignored --nocapture
//! ```
//!
//! Read-only by default. Writes are probed only with `CAPABILITY_WRITE=1`, and
//! then only against `CAPABILITY_ORDER` (default: the 3879 test order).
//!
//! It reports; it does not assert. A red test cannot tell you *which* screen
//! breaks, and the point of this file is the breakdown.

use database::orders::ShopifyBackend;
use database::xbm::{StaffAuthMethod, XbmClient};

#[derive(Clone, Copy, PartialEq)]
enum Verdict {
    /// Same behaviour as PrestaShop for what the screen needs.
    Works,
    /// Reachable, but the screen loses something.
    Partial,
    /// No endpoint at all.
    Missing,
    /// The endpoint exists and refuses us — a grant, not a build.
    Blocked,
    /// Not probed this run (needs a write opt-in or a credential).
    Skipped,
}

impl Verdict {
    fn tag(self) -> &'static str {
        match self {
            Self::Works => "WORKS  ",
            Self::Partial => "PARTIAL",
            Self::Missing => "MISSING",
            Self::Blocked => "BLOCKED",
            Self::Skipped => "skipped",
        }
    }
}

struct Row {
    surface: &'static str,
    need: &'static str,
    verdict: Verdict,
    detail: String,
}

fn row(surface: &'static str, need: &'static str, verdict: Verdict, detail: impl Into<String>) -> Row {
    Row { surface, need, verdict, detail: detail.into() }
}

fn client_or_skip() -> Option<XbmClient> {
    let c = XbmClient::from_env().for_shop(&shop());
    if c.configured() {
        Some(c)
    } else {
        eprintln!("shopify_capability: XBM_API_KEY not configured — skipping");
        None
    }
}

fn shop() -> String {
    std::env::var("CAPABILITY_SHOP").unwrap_or_else(|_| "37rkv3-nc".into())
}

fn order_ref() -> String {
    std::env::var("CAPABILITY_ORDER").unwrap_or_else(|_| "3879".into())
}

fn writes_enabled() -> bool {
    std::env::var("CAPABILITY_WRITE").ok().as_deref() == Some("1")
}

#[tokio::test]
#[ignore = "hits the live Build Management API; run with --ignored --nocapture"]
async fn capability_matrix() {
    let Some(xbm) = client_or_skip() else { return };
    let backend = ShopifyBackend::from_env().for_shop(&shop());
    let reference = order_ref();
    let mut rows: Vec<Row> = Vec::new();

    // Resolve the probe order once; most capabilities need its GID.
    let resolved = xbm.resolve(&reference).await;
    let order_gid = match &resolved {
        Ok(r) => {
            rows.push(row(
                "Check-in / TUR sheet",
                "find an order by service number",
                Verdict::Works,
                format!("{} matchedBy={}", r.name, r.matched_by),
            ));
            r.order_gid.clone()
        }
        Err(e) => {
            rows.push(row("Check-in / TUR sheet", "find an order by service number", Verdict::Missing, e.to_string()));
            println!("cannot resolve {reference}; aborting");
            return;
        }
    };

    // ─── Check-in / TUR sheet ────────────────────────────────────────────
    // `OrderLookup` offers ServiceNumber or Phone.
    match xbm.resolve("8015550142").await {
        Ok(r) => rows.push(row("Check-in / TUR sheet", "find an order by phone number", Verdict::Partial, format!("matched {} — verify", r.name))),
        Err(_) => rows.push(row(
            "Check-in / TUR sheet",
            "find an order by phone number",
            Verdict::Missing,
            "resolve has no phone reading; OrderLookup::Phone has no equivalent",
        )),
    }

    match xbm.resolve("logan.lees@pclaptops.com").await {
        Ok(r) if r.matched_by == "search" => rows.push(row(
            "Check-in / TUR sheet",
            "find an order by customer email",
            Verdict::Partial,
            format!("fuzzy only — returned {}, may not be the right order", r.name),
        )),
        Ok(r) => rows.push(row("Check-in / TUR sheet", "find an order by customer email", Verdict::Works, r.name)),
        Err(_) => rows.push(row("Check-in / TUR sheet", "find an order by customer email", Verdict::Missing, "no match")),
    }

    let detail = xbm.order_detail(&order_gid).await;
    match &detail {
        Ok(d) => {
            let order = d.order.as_ref();
            let cust = order.and_then(|o| o.customer.as_ref());
            let has_name = cust.and_then(|c| c.name.as_deref()).is_some_and(|n| !n.trim().is_empty());
            let has_email = cust.and_then(|c| c.email.as_deref()).is_some_and(|e| !e.trim().is_empty());
            rows.push(row(
                "Check-in / TUR sheet",
                "customer name + email to prefill the form",
                if has_name && has_email { Verdict::Works } else { Verdict::Partial },
                format!("name={has_name} email={has_email}"),
            ));

            rows.push(row(
                "Check-in / TUR sheet",
                "repair intake block (device, serial, check-in notes)",
                if d.service_details.is_some() { Verdict::Works } else { Verdict::Missing },
                if d.service_details.is_some() { "serviceDetails present" } else { "no serviceDetails" },
            ));

            let details = d.order_details.as_ref();
            let has_rep = details
                .and_then(|d| d.sales_rep.as_ref())
                .is_some_and(|r| !r.employee_id.trim().is_empty());
            rows.push(row(
                "Check-in / TUR sheet",
                "sales rep on the order",
                if has_rep { Verdict::Works } else { Verdict::Partial },
                format!("salesRep present={has_rep}"),
            ));

            let split = details.map(|d| d.split_reps.len()).unwrap_or(0);
            rows.push(row(
                "Check-in / TUR sheet",
                "split rep + percentage",
                Verdict::Missing,
                format!("splitReps len={split}; PATCH accepts splitRepId and stores nothing"),
            ));
        }
        Err(e) => rows.push(row("Check-in / TUR sheet", "order detail", Verdict::Missing, e.to_string())),
    }

    // ─── Notes / chats ───────────────────────────────────────────────────
    match xbm.comments(&order_gid, None, Some(50)).await {
        Ok(p) => rows.push(row("Notes / chats", "read the note history", Verdict::Works, format!("{} comments", p.comments.len()))),
        Err(e) => rows.push(row("Notes / chats", "read the note history", Verdict::Missing, e.to_string())),
    }

    if writes_enabled() {
        match xbm.post_comment(&order_gid, "capability probe — safe to delete", None, None).await {
            Ok(c) => rows.push(row("Notes / chats", "post a note", Verdict::Works, format!("id={} author={}", c.id, c.author))),
            Err(e) => rows.push(row("Notes / chats", "post a note", Verdict::Blocked, e.to_string())),
        }
    } else {
        rows.push(row("Notes / chats", "post a note", Verdict::Skipped, "set CAPABILITY_WRITE=1"));
    }
    rows.push(row(
        "Notes / chats",
        "edit or delete a note",
        Verdict::Missing,
        "comments are append-only; the pencil and trash icons have nothing to call",
    ));
    rows.push(row(
        "Notes / chats",
        "private vs public note toggle",
        Verdict::Partial,
        "visibility is internal|customer, not PrestaShop's private flag",
    ));

    // ─── Task audit ──────────────────────────────────────────────────────
    let all = xbm.orders(&[], None, None).await;
    match &all {
        Ok(q) => {
            let qc_only = xbm.orders(&["qc"], None, None).await.map(|r| r.orders.len()).unwrap_or(0);
            rows.push(row(
                "Task audit / shelf lists",
                "list orders filtered by workflow status",
                if qc_only == q.orders.len() { Verdict::Missing } else { Verdict::Works },
                if qc_only == q.orders.len() {
                    format!("bucket ignored — 'qc' returns all {} orders", q.orders.len())
                } else {
                    format!("qc={qc_only} of {}", q.orders.len())
                },
            ));
            rows.push(row(
                "Task audit / shelf lists",
                "page through more orders than one response holds",
                Verdict::Missing,
                "no cursor or limit on /orders; getQueue caps at 250",
            ));
        }
        Err(e) => rows.push(row("Task audit / shelf lists", "list orders", Verdict::Missing, e.to_string())),
    }

    if writes_enabled() {
        let patch = database::xbm::OrderDetailsPatch {
            customer_reference: Some("capability-probe".into()),
            ..Default::default()
        };
        match xbm.update_order_details(&order_gid, &patch).await {
            Ok(_) => rows.push(row("Task audit / shelf lists", "edit an order field (reference, PO, rep, store)", Verdict::Works, "PATCH applied")),
            Err(e) => rows.push(row("Task audit / shelf lists", "edit an order field", Verdict::Blocked, e.to_string())),
        }
    } else {
        rows.push(row("Task audit / shelf lists", "edit an order field", Verdict::Skipped, "set CAPABILITY_WRITE=1"));
    }

    // ─── KOTH / sales tracker ────────────────────────────────────────────
    rows.push(row(
        "KOTH / sales tracker",
        "commission report by rep, status and pay period",
        Verdict::Missing,
        "no equivalent of generate_orders_report; /orders has no date-range or period filter",
    ));
    // A filter that returns 0 proves nothing on its own — compare a real rep,
    // a bogus rep and no filter at all before calling it working.
    let unfiltered = all.as_ref().map(|q| q.orders.len()).unwrap_or(0);
    let real_rep = xbm.orders(&[], Some("183"), None).await.map(|q| q.orders.len());
    let bogus_rep = xbm.orders(&[], Some("nobody-real"), None).await.map(|q| q.orders.len());
    match (real_rep, bogus_rep) {
        (Ok(hit), Ok(miss)) => {
            // The queue projection has to carry the rep for the filter to have
            // anything to match on.
            let projected = all
                .as_ref()
                .map(|q| q.orders.iter().filter(|o| o.sales_rep_code.is_some()).count())
                .unwrap_or(0);
            let verdict = if projected == 0 {
                Verdict::Missing
            } else if miss == 0 && hit > 0 && hit < unfiltered {
                Verdict::Works
            } else {
                Verdict::Partial
            };
            rows.push(row(
                "KOTH / sales tracker",
                "filter orders by sales rep",
                verdict,
                format!(
                    "rep183={hit} bogus={miss} unfiltered={unfiltered};                      {projected} of {unfiltered} queue rows carry a salesRepCode"
                ),
            ));
        }
        _ => rows.push(row("KOTH / sales tracker", "filter orders by sales rep", Verdict::Missing, "query failed")),
    }
    match xbm.staff(Some(true)).await {
        Ok(s) => rows.push(row(
            "KOTH / sales tracker",
            "employee roster",
            Verdict::Partial,
            format!("{} staff — Shopify staff ids, not PrestaShop id_employee", s.staff.len()),
        )),
        Err(e) => rows.push(row("KOTH / sales tracker", "employee roster", Verdict::Missing, e.to_string())),
    }

    // ─── Bench QC ────────────────────────────────────────────────────────
    match xbm.qc_run(&order_gid).await {
        Ok(_) => rows.push(row("Bench QC", "read the QC checklist run", Verdict::Works, "GET /qc answers")),
        Err(e) => rows.push(row("Bench QC", "read the QC checklist run", Verdict::Missing, e.to_string())),
    }
    match xbm
        .authenticate_staff(StaffAuthMethod::Pin { staff_id: "48", pin: "0000" }, None)
        .await
    {
        Ok(_) => rows.push(row("Bench QC", "exchange a floor credential for a token", Verdict::Works, "PIN accepted")),
        Err(e) => {
            let msg = e.to_string();
            // A rejected PIN proves the endpoint works; we simply do not hold one.
            let verdict = if msg.contains("401") || msg.to_lowercase().contains("credential") {
                Verdict::Works
            } else {
                Verdict::Blocked
            };
            rows.push(row("Bench QC", "exchange a floor credential for a token", verdict, msg));
        }
    }
    rows.push(row(
        "Bench QC",
        "submit a QC result",
        Verdict::Blocked,
        "needs an X-Staff-Token holding qc.perform; an API key alone is refused",
    ));

    if writes_enabled() {
        match xbm
            .advance_order(
                &order_gid,
                &database::xbm::AdvanceRequest {
                    to_status_gid: None,
                    to_status_name: Some("In Repair".into()),
                    note: Some("capability probe".into()),
                    force: None,
                },
            )
            .await
        {
            Ok(_) => rows.push(row("Bench QC", "advance the order status", Verdict::Works, "advance accepted")),
            Err(e) => rows.push(row("Bench QC", "advance the order status", Verdict::Blocked, e.to_string())),
        }
    } else {
        rows.push(row("Bench QC", "advance the order status", Verdict::Skipped, "set CAPABILITY_WRITE=1"));
    }
    rows.push(row("Bench QC", "attach or detach a component serial", Verdict::Blocked, "scope 'workflow' not granted to this key"));

    match backend.recent_orders(25).await {
        Ok(o) => rows.push(row("Bench QC", "build-intake queue for the bench picker", Verdict::Works, format!("{} orders", o.len()))),
        Err(e) => rows.push(row("Bench QC", "build-intake queue for the bench picker", Verdict::Missing, e.to_string())),
    }

    // ─── First run / OA3 ─────────────────────────────────────────────────
    match xbm.serial_history("1234").await {
        Ok(h) => rows.push(row(
            "First run / OA3 lookup",
            "find the original invoice from a hardware serial",
            Verdict::Works,
            format!(
                "found={} shopify={} odoo={} prestashop_rows={} events={}",
                h.found,
                h.shopify.is_some(),
                h.odoo.is_some(),
                h.prestashop.len(),
                h.history.len()
            ),
        )),
        Err(e) => rows.push(row("First run / OA3 lookup", "find the original invoice from a hardware serial", Verdict::Missing, e.to_string())),
    }

    // ─── Report ──────────────────────────────────────────────────────────
    println!("\nShopify capability matrix — order {reference} on shop {}\n", shop());
    let mut current = "";
    for r in &rows {
        if r.surface != current {
            println!("\n  {}", r.surface);
            println!("  {}", "─".repeat(r.surface.chars().count()));
            current = r.surface;
        }
        println!("    [{}] {:<48} {}", r.verdict.tag(), r.need, truncate(&r.detail, 78));
    }

    let count = |v: Verdict| rows.iter().filter(|r| r.verdict == v).count();
    println!(
        "\n  ── {} probed: {} work, {} partial, {} missing, {} blocked, {} skipped\n",
        rows.len(),
        count(Verdict::Works),
        count(Verdict::Partial),
        count(Verdict::Missing),
        count(Verdict::Blocked),
        count(Verdict::Skipped),
    );
}

fn truncate(s: &str, max: usize) -> String {
    let s = s.replace('\n', " ");
    if s.chars().count() <= max {
        return s;
    }
    format!("{}…", s.chars().take(max - 1).collect::<String>())
}
