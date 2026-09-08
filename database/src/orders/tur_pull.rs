//! Pull one order into the TUR sheet's form shape, from whichever backend and
//! whichever store holds it.
//!
//! The TUR sheet was wired straight to `get_prestashop_payload`, so a Shopify
//! order number reached `GET /orders/{n}` on PrestaShop, matched nothing, and
//! the error went nowhere. This routes the same request through both backends
//! and answers in the sheet's existing [`PrestashopPayload`] shape, so the
//! PrestaShop path is untouched and stays the fallback.
//!
//! Store matters as much as backend here. `XBM_SHOP` is compiled in, and an
//! order that lives on the other store 404s with a message that reads like
//! "no such order". Every configured store is tried before that verdict.

use anyhow::{anyhow, Context, Result};
use chrono::Utc;

use crate::schema::prestashop::{
    Address, Associations, Employee, Order, OrderRow, PrestashopPayload, ServiceOrder,
};
use crate::schema::task_creation::{fetch_prestashop_order, OrderLookup};
use crate::schema::{CustomerData, RecordId, TaskNotePayload, CUSTOMER_TABLE, TASK_NOTE_TABLE};
use crate::xbm::types::BuildDetail;
use crate::xbm::XbmClient;

use super::{BackendKind, RoutingMode};

/// One pulled order plus where it came from, so the UI can say which backend
/// and store answered rather than implying PrestaShop.
#[derive(Debug, Clone)]
pub struct PulledOrder {
    pub payload: PrestashopPayload,
    pub source: BackendKind,
    /// XBM store handle that answered; empty means the server's default store.
    /// Always empty for a PrestaShop pull.
    pub shop: String,
}

impl PulledOrder {
    /// `"PrestaShop"` / `"Shopify (pclaptops)"` — for a toast or a log line.
    pub fn origin(&self) -> String {
        match self.source {
            BackendKind::Prestashop => "PrestaShop".to_string(),
            BackendKind::Shopify if self.shop.is_empty() => "Shopify".to_string(),
            BackendKind::Shopify => format!("Shopify ({})", self.shop),
        }
    }
}

/// Store handles to try beyond the compiled `XBM_SHOP`. An empty entry is the
/// server's own default store. Two stores share one binary, so a compiled
/// default cannot be the only one consulted.
const OTHER_SHOPS: &[&str] = &["", "pclaptops"];

/// `XBM_SHOP` first, then the rest, deduplicated.
fn candidate_shops() -> Vec<String> {
    let mut shops = vec![crate::XBM_SHOP.trim().to_string()];
    for shop in OTHER_SHOPS {
        let shop = shop.trim().to_string();
        if !shops.contains(&shop) {
            shops.push(shop);
        }
    }
    shops
}

fn order_of_attempt() -> [BackendKind; 2] {
    match super::routing_mode() {
        RoutingMode::Prestashop => [BackendKind::Prestashop, BackendKind::Shopify],
        RoutingMode::Auto | RoutingMode::Shopify => [BackendKind::Shopify, BackendKind::Prestashop],
    }
}

/// Pull `lookup` from the first backend that has it.
///
/// The error names every backend that was asked and what each said, because
/// "not found" and "the backend is down" send a tech to different places.
pub async fn pull_order(lookup: OrderLookup) -> Result<PulledOrder> {
    let label = match &lookup {
        OrderLookup::ServiceNumber(n) => n.clone(),
        OrderLookup::Phone(p) => p.clone(),
    };
    if label.trim().is_empty() {
        return Err(anyhow!("Enter a service number or a phone number first"));
    }

    let mut misses: Vec<String> = Vec::new();
    for backend in order_of_attempt() {
        let attempt = match backend {
            BackendKind::Shopify => shopify_leg(&lookup).await,
            BackendKind::Prestashop => prestashop_leg(&lookup).await,
        };
        match attempt {
            Ok(Some(pulled)) => {
                log::info!("TUR pull: {label} came from {}", pulled.origin());
                return Ok(pulled);
            }
            Ok(None) => misses.push(format!("{}: no match", backend.as_str())),
            Err(e) => {
                log::warn!("TUR pull: {} leg failed for {label}: {e:#}", backend.as_str());
                misses.push(format!("{}: {e}", backend.as_str()));
            }
        }
    }
    Err(anyhow!("Could not pull {label} — {}", misses.join("; ")))
}

async fn prestashop_leg(lookup: &OrderLookup) -> Result<Option<PulledOrder>> {
    if !crate::prestashop_configured() {
        return Ok(None);
    }
    match fetch_prestashop_order(lookup.clone()).await {
        Ok(payload) => Ok(Some(PulledOrder {
            payload,
            source: BackendKind::Prestashop,
            shop: String::new(),
        })),
        Err(e) => Err(e),
    }
}

/// Resolve the reference on every configured store, then map the build detail
/// into the sheet's shape. Phone lookups have no Shopify equivalent.
async fn shopify_leg(lookup: &OrderLookup) -> Result<Option<PulledOrder>> {
    let OrderLookup::ServiceNumber(reference) = lookup else {
        return Ok(None);
    };
    let base = XbmClient::from_env();
    if !base.configured() {
        return Ok(None);
    }

    for shop in candidate_shops() {
        let client = XbmClient::from_env().for_shop(&shop);
        let resolved = match client.resolve(reference).await {
            Ok(r) => r,
            Err(e) if e.is_not_found() => continue,
            Err(e) => return Err(anyhow!(e)).context("Build Management resolve failed"),
        };

        let detail = client
            .order_detail(&resolved.order_gid)
            .await
            .map_err(|e| anyhow!(e))
            .context("Build Management order detail failed")?;

        // A comment fetch failure loses the notes, not the order — the sheet is
        // still worth filling without them.
        let notes = match client.comments(&resolved.order_gid, None, Some(100)).await {
            Ok(payload) => notes_from_comments(payload.comments, reference).await,
            Err(e) => {
                log::warn!("TUR pull: comments for {reference} failed: {e}");
                Vec::new()
            }
        };

        let payload = payload_from_detail(&detail, &resolved.name, notes).await;
        return Ok(Some(PulledOrder { payload, source: BackendKind::Shopify, shop }));
    }
    Ok(None)
}

/// Convert the comment stream into task notes.
///
/// `user` is a real `user` foreign key that gets persisted on submit, so a
/// comment whose author does not resolve to one is dropped and logged — the
/// same thing `CustomerMessage::into_task_note` does on the PrestaShop side.
/// Shopify identifies an author by display name and an XBM staff id, and
/// neither is an email, so the name is what there is to match on.
async fn notes_from_comments(
    comments: Vec<crate::xbm::types::XbmComment>,
    reference: &str,
) -> Vec<TaskNotePayload> {
    let mut notes = Vec::new();
    for comment in comments {
        if comment.body.trim().is_empty() {
            continue;
        }
        let user = match crate::schema::User::query_user_from_name(&comment.author).await {
            Ok(user) => user,
            Err(e) => {
                log::info!(
                    "TUR pull: dropping comment {} on {reference} — author {:?} unresolved: {e}",
                    comment.id,
                    comment.author
                );
                continue;
            }
        };
        notes.push(TaskNotePayload {
            id: RecordId::new(TASK_NOTE_TABLE, comment.id),
            task_id: None,
            created_at: comment
                .created_at
                .as_deref()
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|d| d.with_timezone(&Utc).into())
                .unwrap_or_else(|| Utc::now().into()),
            note: comment.body,
            username: user.get_username().to_string(),
            user: user.get_id(),
            id_customer_thread: None,
            id_customer_message: None,
            id_employee: comment.author_staff_id,
            service_number: Some(reference.trim_start_matches('#').to_string()),
            private: comment.visibility.eq_ignore_ascii_case("internal"),
        });
    }
    notes
}

/// Map an XBM build detail onto the sheet's PrestaShop shape.
///
/// Fields Shopify has no counterpart for are left empty rather than filled
/// with a stand-in: a blank salesman prompts the tech, a wrong one does not.
async fn payload_from_detail(
    detail: &BuildDetail,
    order_name: &str,
    task_notes: Vec<TaskNotePayload>,
) -> PrestashopPayload {
    let order = detail.order.as_ref();
    let customer = order.and_then(|o| o.customer.as_ref());

    // `#3881` is the display form; the sheet keys the ticket off the bare number.
    let order_number = order_name.trim_start_matches('#').to_string();

    let order_rows = detail
        .line_items
        .iter()
        .map(|li| OrderRow {
            id: crate::xbm::XbmClient::order_path_id(&li.id).to_string(),
            id_order_config: String::new(),
            product_id: li.product_handle.clone().unwrap_or_default(),
            product_quantity: li.qty.to_string(),
            product_name: li.title.clone(),
            product_price: String::new(),
            product_reference: li.sku.clone().unwrap_or_default(),
        })
        .collect();

    let order_service = service_order_from(detail);

    let (sales_rep, split_rep) = reps_from(detail).await;

    let phone = customer.and_then(|c| c.phone.clone()).unwrap_or_default();
    let email = customer.and_then(|c| c.email.clone()).unwrap_or_default();
    let name = customer.and_then(|c| c.name.clone()).unwrap_or_default();

    // Shopify customer ids are GIDs and the sheet's CustomerData keys on a
    // PrestaShop numeric. Keying on the order number keeps the record
    // addressable without inventing a customer id that means nothing.
    let customer_key = order_number.clone();

    PrestashopPayload {
        customer: CustomerData {
            id: RecordId::new(CUSTOMER_TABLE, customer_key.clone()),
            cust_code: customer_key,
            name,
            email,
            phone_number: phone.clone(),
            ..Default::default()
        },
        order: Order {
            id: order_number,
            current_state: detail
                .current_status
                .as_ref()
                .map(|s| s.legacy_id.to_string())
                .unwrap_or_default(),
            reference: detail.order_reference.clone().unwrap_or_default(),
            order_type: detail
                .order_type
                .as_ref()
                .map(|t| t.name.clone())
                .unwrap_or_default(),
            payment: order.and_then(|o| o.financial_status.clone()).unwrap_or_default(),
            shipping_number: String::new(),
            associations: Associations {
                order_rows,
                order_service,
                order_serial: Vec::new(),
            },
            ..Default::default()
        },
        sales_rep,
        split_rep,
        address: Address { phone, ..Default::default() },
        customer_threads: Vec::new(),
        customer_messages: Vec::new(),
        task_notes,
    }
}

/// `service_details` is a free-form JSON block; the sheet wants one
/// `ServiceOrder`. An all-blank block is the API's empty shell, not an intake.
fn service_order_from(detail: &BuildDetail) -> Vec<ServiceOrder> {
    let Some(obj) = detail.service_details.as_ref().and_then(|v| v.as_object()) else {
        return Vec::new();
    };
    let field = |key: &str| {
        obj.get(key).and_then(|v| v.as_str()).unwrap_or_default().trim().to_string()
    };
    let service = ServiceOrder {
        id_order_service: field("id_status_service"),
        device_name: field("device_name"),
        device_mfg: field("device_mfg"),
        device_model: field("device_model"),
        device_serial: field("device_serial"),
        device_password: field("device_password"),
        device_power_supply: field("device_power_supply"),
        other_hardware_software: field("other_hardware_software"),
        physical_damage: field("physical_damage"),
        check_in_notes: field("check_in_notes"),
        intake_notes: field("intake_notes"),
    };
    let blank = [
        &service.device_name,
        &service.device_mfg,
        &service.device_model,
        &service.device_serial,
        &service.device_password,
        &service.device_power_supply,
        &service.other_hardware_software,
        &service.physical_damage,
        &service.check_in_notes,
        &service.intake_notes,
    ]
    .iter()
    .all(|v| v.is_empty());
    if blank {
        Vec::new()
    } else {
        vec![service]
    }
}

/// The sheet fills salesman / tech / check-in rep from an employee **email**,
/// which XBM does not carry. `orderDetails.salesRep.employeeId` is the
/// PrestaShop `id_employee`, so the email comes from PrestaShop — the one
/// place both id spaces meet. Unresolvable means an empty rep, not a guess.
async fn reps_from(detail: &BuildDetail) -> (Option<Employee>, Option<Employee>) {
    let Some(details) = detail.order_details.as_ref() else {
        return (None, None);
    };
    let sales = match details.sales_rep.as_ref() {
        Some(rep) => employee_for(&rep.employee_id, &rep.name).await,
        None => None,
    };
    let split = match details.split_reps.first() {
        Some(rep) => employee_for(&rep.employee_id, &rep.name).await,
        None => None,
    };
    (sales, split)
}

async fn employee_for(employee_id: &str, name: &str) -> Option<Employee> {
    let employee_id = employee_id.trim();
    if employee_id.is_empty() || employee_id == "0" {
        return None;
    }
    if crate::prestashop_configured() {
        use crate::schema::helper_traits::EmployeeHelper;
        match Employee::default().get_employee_from_id(employee_id).await {
            Ok(employee) if !employee.email.trim().is_empty() => return Some(employee),
            Ok(_) => log::debug!("TUR pull: employee {employee_id} has no email on PrestaShop"),
            Err(e) => log::debug!("TUR pull: employee {employee_id} lookup failed: {e}"),
        }
    }
    // Name only. The sheet's rep fields stay blank rather than showing a value
    // its autocomplete cannot match back to a user.
    let (firstname, lastname) = match name.trim().split_once(' ') {
        Some((first, last)) => (first.to_string(), last.to_string()),
        None => (name.trim().to_string(), String::new()),
    };
    (!firstname.is_empty()).then(|| Employee {
        id: employee_id.to_string(),
        firstname,
        lastname,
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_store_is_a_candidate_and_none_twice() {
        let shops = candidate_shops();
        assert_eq!(shops[0], crate::XBM_SHOP.trim(), "the compiled store is tried first");
        for shop in OTHER_SHOPS {
            assert!(shops.iter().any(|s| s == shop.trim()), "{shop:?} must be reachable");
        }
        let mut sorted = shops.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), shops.len(), "a store must not be asked twice");
    }

    #[test]
    fn attempt_order_follows_routing_mode() {
        super::super::set_routing_mode(RoutingMode::Prestashop);
        assert_eq!(order_of_attempt()[0], BackendKind::Prestashop);
        super::super::set_routing_mode(RoutingMode::Shopify);
        assert_eq!(order_of_attempt()[0], BackendKind::Shopify);
        super::super::set_routing_mode(RoutingMode::Auto);
        assert_eq!(order_of_attempt()[0], BackendKind::Shopify);
    }

    #[tokio::test]
    async fn a_blank_reference_is_not_a_lookup() {
        let err = pull_order(OrderLookup::ServiceNumber("  ".into())).await.unwrap_err();
        assert!(err.to_string().contains("Enter a service number"), "{err}");
    }

    #[test]
    fn origin_names_the_store() {
        let mut pulled = PulledOrder {
            payload: PrestashopPayload::default(),
            source: BackendKind::Shopify,
            shop: "pclaptops".into(),
        };
        assert_eq!(pulled.origin(), "Shopify (pclaptops)");
        pulled.shop = String::new();
        assert_eq!(pulled.origin(), "Shopify");
        pulled.source = BackendKind::Prestashop;
        assert_eq!(pulled.origin(), "PrestaShop");
    }

    #[test]
    fn an_empty_service_block_is_not_an_intake_record() {
        let mut detail = BuildDetail::default();
        detail.service_details = Some(serde_json::json!({
            "device_name": "", "device_mfg": "  ", "check_in_notes": ""
        }));
        assert!(service_order_from(&detail).is_empty());

        detail.service_details = Some(serde_json::json!({
            "device_name": "Aurora", "device_mfg": "Dell", "check_in_notes": "won't post"
        }));
        let rows = service_order_from(&detail);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].device_name, "Aurora");
        assert_eq!(rows[0].check_in_notes, "won't post");
    }
}
