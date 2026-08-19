//! Services parked in a PrestaShop status, with the detail a triage pass needs.
//!
//! Same query shape the Task Audit tab uses (current_state + service order
//! type, newest first), enriched per order with the device and check-in notes
//! so a caller does not have to walk PrestaShop itself.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::business_calendar::business_seconds;
use super::prestashop::{Order, OrderState, Prestashop};
use super::Store;

/// Service orders only; PrestaShop mixes sales and service in one resource.
const SERVICE_ORDER_TYPE: &str = "2";
const DEFAULT_LIMIT: usize = 15;

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct WaitingService {
    pub service_number: String,
    pub reference: String,
    pub checked_in_at: String,
    /// Open-store hours the machine has been sitting in this status.
    pub waiting_open_hours: f64,
    pub store: Option<String>,
    pub customer_id: String,
    pub device: String,
    pub device_serial: String,
    pub checkin_notes: String,
    pub intake_notes: String,
    pub physical_damage: String,
}

fn order_list_query<'a>(state: &'a str, store: Option<&'a str>) -> HashMap<&'a str, &'a str> {
    let mut query: HashMap<&str, &str> = HashMap::new();
    query.insert("filter[current_state]", state);
    query.insert("filter[id_order_type]", SERVICE_ORDER_TYPE);
    query.insert("output_format", "JSON");
    query.insert("sort", "[id_DESC]");
    if let Some(store) = store {
        query.insert("filter[id_store]", store);
    }
    query
}

fn open_hours_since(date_add: &str) -> f64 {
    let parsed = chrono::NaiveDateTime::parse_from_str(date_add, "%Y-%m-%d %H:%M:%S").ok();
    let Some(naive) = parsed else { return 0.0 };
    let from = naive.and_utc();
    business_seconds(from, chrono::Utc::now()) as f64 / 3600.0
}

impl WaitingService {
    fn from_order(order: &Order) -> Self {
        let svc = order.associations.order_service.first();
        Self {
            service_number: order.id.clone(),
            reference: order.reference.clone(),
            checked_in_at: order.date_add.clone(),
            waiting_open_hours: (open_hours_since(&order.date_add) * 10.0).round() / 10.0,
            store: matches!(order.id_store.as_str(), "7" | "8" | "10" | "12" | "14")
                .then(|| Store::from_presta_store_id(&order.id_store).as_str().to_string()),
            customer_id: order.id_customer.clone(),
            device: svc
                .map(|s| format!("{} {} {}", s.device_mfg, s.device_model, s.device_name))
                .unwrap_or_default()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" "),
            device_serial: svc.map(|s| s.device_serial.clone()).unwrap_or_default(),
            checkin_notes: svc.map(|s| s.check_in_notes.clone()).unwrap_or_default(),
            intake_notes: svc.map(|s| s.intake_notes.clone()).unwrap_or_default(),
            physical_damage: svc.map(|s| s.physical_damage.clone()).unwrap_or_default(),
        }
    }

    /// Orders sitting in `state`, newest first, enriched up to `limit`.
    pub async fn in_status(
        state: OrderState,
        store: Option<&str>,
        limit: Option<usize>,
    ) -> anyhow::Result<Vec<Self>> {
        let limit = limit.unwrap_or(DEFAULT_LIMIT).clamp(1, 100);
        let mut api = Prestashop::default();
        api.display = "[id]";
        let ids: Vec<super::prestashop::PrestashopId> = api
            .request_resources_checked("orders", order_list_query(state.to_id_str(), store))
            .await?;

        let mut out = Vec::new();
        for id in ids.iter().take(limit) {
            if id.id.is_empty() {
                continue;
            }
            let mut detail = Prestashop::default();
            detail.display = "full";
            let mut query = order_list_query(state.to_id_str(), store);
            query.insert("filter[id]", id.id.as_str());
            match detail
                .request_resources_checked::<Order>("orders", query)
                .await
            {
                Ok(orders) => {
                    if let Some(order) = orders.first() {
                        out.push(Self::from_order(order));
                    }
                }
                // One unreadable order must not sink the whole sweep.
                Err(e) => log::warn!("waiting_services: order {} detail failed: {e}", id.id),
            }
        }
        Ok(out)
    }
}
