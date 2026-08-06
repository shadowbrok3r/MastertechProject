//! Customer resolution for the link-customer flow.
//!
//! Two independent paths can name a customer for a connected client:
//!
//!  - the service order number the operator typed, resolved through
//!    `service_order.customer` (falling back to the PrestaShop order's
//!    `id_customer`);
//!  - the OA3 / motherboard serial cached on the client, which resolves
//!    through `order_serial` to whichever order first shipped that
//!    hardware.
//!
//! On a resold used machine those disagree: the serial still points at the
//! machine's previous owner. The typed order is the authority, and a
//! disagreement is surfaced for the operator to settle rather than resolved
//! silently.

use super::{
    utilities::get_prestashop_payload, CustomerData, RecordId, RecordIdExt, CUSTOMER_TABLE,
    TICKET_TABLE,
};
use crate::db;
use serde::{Deserialize, Serialize};

/// Which lookup produced a candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CustomerSource {
    /// `service_order.customer` for the typed order number.
    ServiceOrder,
    /// `id_customer` on the PrestaShop order for the typed order number.
    PrestashopOrder,
    /// OA3 / motherboard-serial lookup cached on the connected client.
    Serial,
}

impl CustomerSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::ServiceOrder => "service order",
            Self::PrestashopOrder => "PrestaShop order",
            Self::Serial => "machine serial (OA3)",
        }
    }

    /// True for the sources derived from an order number the operator typed.
    pub fn is_order(self) -> bool {
        matches!(self, Self::ServiceOrder | Self::PrestashopOrder)
    }
}

/// One customer a lookup path arrived at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomerCandidate {
    /// `customer` record key, e.g. `"201989"`.
    pub customer_key: String,
    pub name: String,
    /// Order number this candidate came from; the `friendly_name` suffix.
    pub order_number: String,
    pub source: CustomerSource,
}

impl CustomerCandidate {
    pub fn new(
        customer_key: impl Into<String>,
        name: impl Into<String>,
        order_number: impl Into<String>,
        source: CustomerSource,
    ) -> Self {
        Self {
            customer_key: customer_key.into().trim().to_string(),
            name: name.into().trim().to_string(),
            order_number: order_number.into().trim().to_string(),
            source,
        }
    }

    pub fn record_id(&self) -> RecordId {
        RecordId::new(CUSTOMER_TABLE, self.customer_key.as_str())
    }

    /// `"First Last - Order"`, or the bare name when no order number is known.
    pub fn friendly_name(&self) -> String {
        let name = self.name.trim();
        let order = self.order_number.trim();
        if name.is_empty() {
            String::new()
        } else if order.is_empty() {
            name.to_string()
        } else {
            format!("{name} - {order}")
        }
    }

    /// One-line operator-facing summary.
    pub fn describe(&self) -> String {
        let name = if self.name.trim().is_empty() {
            "(no name)"
        } else {
            self.name.trim()
        };
        let order = self.order_number.trim();
        if order.is_empty() {
            format!("{name} — customer:{}", self.customer_key)
        } else {
            format!("{name} — customer:{} via order {order}", self.customer_key)
        }
    }
}

/// Outcome of comparing the order-derived and serial-derived candidates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CustomerResolution {
    /// Neither path produced a customer.
    None,
    /// One path produced a customer, or both agreed on the same one.
    Agreed(CustomerCandidate),
    /// The typed order and the serial name different customers.
    Conflict {
        from_order: CustomerCandidate,
        from_serial: CustomerCandidate,
    },
}

impl CustomerResolution {
    /// The candidate a commit uses unless the operator picks the other one.
    /// The typed order wins every conflict.
    pub fn default_choice(&self) -> Option<&CustomerCandidate> {
        match self {
            Self::None => None,
            Self::Agreed(c) => Some(c),
            Self::Conflict { from_order, .. } => Some(from_order),
        }
    }

    pub fn is_conflict(&self) -> bool {
        matches!(self, Self::Conflict { .. })
    }
}

/// Compare the two lookup paths. The typed order is authoritative; a
/// disagreement is reported rather than resolved.
pub fn resolve_customer(
    from_order: Option<CustomerCandidate>,
    from_serial: Option<CustomerCandidate>,
) -> CustomerResolution {
    match (from_order, from_serial) {
        (Some(order), Some(serial)) => {
            if customer_keys_match(&order.customer_key, &serial.customer_key) {
                CustomerResolution::Agreed(order)
            } else {
                CustomerResolution::Conflict {
                    from_order: order,
                    from_serial: serial,
                }
            }
        }
        (Some(order), None) => CustomerResolution::Agreed(order),
        (None, Some(serial)) => CustomerResolution::Agreed(serial),
        (None, None) => CustomerResolution::None,
    }
}

/// Keys match on the trimmed string, or when both parse to the same integer
/// (string-keyed and number-keyed rows for the same PrestaShop customer).
pub fn customer_keys_match(a: &str, b: &str) -> bool {
    let (a, b) = (a.trim(), b.trim());
    if a.is_empty() || b.is_empty() {
        return false;
    }
    if a == b {
        return true;
    }
    matches!((a.parse::<i64>(), b.parse::<i64>()), (Ok(x), Ok(y)) if x == y)
}

/// Record key out of a SurrealDB JSON value, accepting the `{ tb, id }`
/// object form, the stringified `table:key` form, and a bare number.
pub fn record_key_from_json(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => match s.split_once(':') {
            Some((_, key)) => super::entity_link::strip_surreal_key_quotes(key),
            None => s.trim().to_string(),
        },
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Object(obj) => obj
            .get("id")
            .map(|inner| match inner {
                serde_json::Value::String(s) => s.trim().to_string(),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Object(o) => o
                    .get("String")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string())
                    .or_else(|| o.get("Number").and_then(|x| x.as_i64()).map(|n| n.to_string()))
                    .unwrap_or_default(),
                _ => String::new(),
            })
            .unwrap_or_default(),
        _ => String::new(),
    }
}

/// What a typed order number resolved to.
#[derive(Debug, Clone, Default)]
pub struct OrderCustomerLookup {
    /// The customer the order names, `None` when neither path found one.
    pub candidate: Option<CustomerCandidate>,
    /// Contact fields off the PrestaShop order, for backfilling a form.
    pub contact: Option<CustomerData>,
    /// Why the PrestaShop fetch produced nothing, when it failed.
    pub prestashop_error: Option<String>,
}

/// Resolve the customer for an order number the operator typed.
/// `service_order.customer` is the authority; the PrestaShop order's
/// `id_customer` is the fallback when no local row carries the link.
pub async fn lookup_order_customer(order_number: &str) -> Result<OrderCustomerLookup, String> {
    let num = order_number.trim();
    if num.is_empty() {
        return Err("order number is empty".to_string());
    }

    let mut out = OrderCustomerLookup::default();
    match service_order_customer(num).await {
        Ok(found) => out.candidate = found,
        Err(e) => log::warn!("service_order customer lookup failed for {num}: {e}"),
    }

    match get_prestashop_payload(num).await {
        Ok(payload) => {
            let key = payload.customer.id.key_string();
            if out.candidate.is_none() && !key.is_empty() {
                out.candidate = Some(CustomerCandidate::new(
                    key,
                    payload.customer.name.clone(),
                    num,
                    CustomerSource::PrestashopOrder,
                ));
            }
            out.contact = Some(payload.customer);
        }
        Err(e) => {
            let msg = format!("PrestaShop order {num} lookup failed: {e}");
            if out.candidate.is_none() {
                return Err(msg);
            }
            out.prestashop_error = Some(msg);
        }
    }

    Ok(out)
}

/// `service_order.customer` for an order number, matched against the record
/// key in both its string and number forms and against `service_number`.
async fn service_order_customer(order_number: &str) -> Result<Option<CustomerCandidate>, String> {
    let mut ids = vec![RecordId::new(TICKET_TABLE, order_number)];
    if let Ok(n) = order_number.parse::<i64>() {
        if n.to_string() == order_number {
            ids.push(RecordId::new(TICKET_TABLE, n));
        }
    }

    let rows: Vec<serde_json::Value> = db()
        .query(
            "SELECT customer, service_number FROM service_order \
             WHERE id IN $ids OR service_number == $num LIMIT 1",
        )
        .bind(("ids", ids))
        .bind(("num", order_number.to_string()))
        .await
        .map_err(|e| e.to_string())?
        .take(0)
        .map_err(|e| e.to_string())?;

    let Some(row) = rows.first() else {
        return Ok(None);
    };
    let key = row
        .get("customer")
        .map(record_key_from_json)
        .unwrap_or_default();
    if key.is_empty() {
        return Ok(None);
    }
    let order = row
        .get("service_number")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(order_number)
        .to_string();

    Ok(Some(CustomerCandidate::new(
        key.clone(),
        customer_name(&key).await,
        order,
        CustomerSource::ServiceOrder,
    )))
}

/// `customer.name` for a key, empty when the row is missing or unreadable.
async fn customer_name(key: &str) -> String {
    let rid = RecordId::new(CUSTOMER_TABLE, key);
    let rows: Vec<serde_json::Value> = match db().query("SELECT name FROM $id").bind(("id", rid)).await
    {
        Ok(mut r) => r.take(0).unwrap_or_default(),
        Err(e) => {
            log::warn!("customer name lookup failed for {key}: {e}");
            return String::new();
        }
    };
    rows.first()
        .and_then(|v| v.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order_candidate() -> CustomerCandidate {
        CustomerCandidate::new("201989", "Seth Grover", "2152279", CustomerSource::ServiceOrder)
    }

    fn serial_candidate() -> CustomerCandidate {
        CustomerCandidate::new("2095832", "ron zuiderweg", "2095832", CustomerSource::Serial)
    }

    #[test]
    fn typed_order_wins_over_disagreeing_serial() {
        let resolution = resolve_customer(Some(order_candidate()), Some(serial_candidate()));

        assert!(
            resolution.is_conflict(),
            "a resold machine must surface both customers, not pick one silently"
        );
        let chosen = resolution.default_choice().expect("conflict has a default");
        assert_eq!(chosen.customer_key, "201989");
        assert_eq!(chosen.order_number, "2152279");
        assert_eq!(chosen.friendly_name(), "Seth Grover - 2152279");

        let CustomerResolution::Conflict { from_serial, .. } = &resolution else {
            unreachable!("checked above");
        };
        assert_eq!(from_serial.customer_key, "2095832");
    }

    #[test]
    fn prestashop_fallback_still_beats_the_serial() {
        let from_order =
            CustomerCandidate::new("201989", "Seth Grover", "2152279", CustomerSource::PrestashopOrder);
        let resolution = resolve_customer(Some(from_order), Some(serial_candidate()));

        assert!(resolution.is_conflict());
        assert_eq!(resolution.default_choice().unwrap().customer_key, "201989");
    }

    #[test]
    fn agreeing_paths_collapse_to_the_order() {
        let serial =
            CustomerCandidate::new("201989", "Seth Grover", "2095832", CustomerSource::Serial);
        let resolution = resolve_customer(Some(order_candidate()), Some(serial));

        assert!(!resolution.is_conflict());
        let chosen = resolution.default_choice().unwrap();
        assert_eq!(chosen.source, CustomerSource::ServiceOrder);
        assert_eq!(chosen.order_number, "2152279");
    }

    #[test]
    fn serial_only_still_resolves() {
        let resolution = resolve_customer(None, Some(serial_candidate()));
        assert!(!resolution.is_conflict());
        assert_eq!(resolution.default_choice().unwrap().customer_key, "2095832");

        assert_eq!(resolve_customer(None, None), CustomerResolution::None);
        assert!(CustomerResolution::None.default_choice().is_none());
    }

    #[test]
    fn keys_match_across_string_and_number_forms() {
        assert!(customer_keys_match("201989", " 201989 "));
        assert!(customer_keys_match("0201989", "201989"));
        assert!(!customer_keys_match("201989", "2095832"));
        assert!(!customer_keys_match("", "201989"));
    }

    #[test]
    fn friendly_name_pairs_the_chosen_customer_with_its_order() {
        assert_eq!(order_candidate().friendly_name(), "Seth Grover - 2152279");
        assert_eq!(
            CustomerCandidate::new("201989", "Seth Grover", "", CustomerSource::ServiceOrder)
                .friendly_name(),
            "Seth Grover"
        );
        assert!(
            CustomerCandidate::new("201989", "", "2152279", CustomerSource::ServiceOrder)
                .friendly_name()
                .is_empty()
        );
    }

    #[test]
    fn record_key_from_json_accepts_every_id_shape() {
        let obj = serde_json::json!({ "tb": "customer", "id": "201989" });
        assert_eq!(record_key_from_json(&obj), "201989");

        let nested = serde_json::json!({ "tb": "customer", "id": { "String": "201989" } });
        assert_eq!(record_key_from_json(&nested), "201989");

        let numeric = serde_json::json!({ "tb": "customer", "id": { "Number": 201989 } });
        assert_eq!(record_key_from_json(&numeric), "201989");

        let stringified = serde_json::json!("customer:`201989`");
        assert_eq!(record_key_from_json(&stringified), "201989");

        assert_eq!(record_key_from_json(&serde_json::Value::Null), "");
    }
}
