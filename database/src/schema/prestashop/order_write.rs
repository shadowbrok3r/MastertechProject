use super::xml::{element_text, has_element, modify_xml, remove_xml_tag};
use super::Prestashop;
use futures::lock::Mutex;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Arc;

/// Held across a whole GET -> modify -> PUT cycle, one lock per order id.
static ORDER_LOCKS: Lazy<Mutex<HashMap<String, Arc<Mutex<()>>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

async fn order_lock(order_id: &str) -> Arc<Mutex<()>> {
    let mut locks = ORDER_LOCKS.lock().await;
    locks
        .entry(order_id.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

/// Writes one order field. See [`set_order_fields`].
pub async fn set_order_field(
    order_id: &str,
    field: &str,
    value: &str,
) -> anyhow::Result<String, anyhow::Error> {
    set_order_fields(order_id, &[(field, value)]).await
}

/// Applies field updates to an order in one serialized GET -> modify -> PUT cycle.
///
/// The PUT replaces the whole resource, so pass every field being changed in a single call.
pub async fn set_order_fields(
    order_id: &str,
    fields: &[(&str, &str)],
) -> anyhow::Result<String, anyhow::Error> {
    if fields.is_empty() {
        anyhow::bail!("set_order_fields called with no fields for order {order_id}");
    }

    let lock = order_lock(order_id).await;
    let _guard = lock.lock().await;

    let api = Prestashop::default();
    let mut payload = api.request_raw_resource_by_id("orders", order_id).await?;

    for (field, value) in fields {
        // modify_xml is a no-op when the tag is absent.
        if !has_element(&payload, field) {
            anyhow::bail!("order {order_id} has no <{field}> field to update");
        }
        payload = modify_xml(&payload, field, value)?;
    }

    let payload = remove_xml_tag(&payload, "tax_exempt")?;
    let write = api.modify_prestashop_order(&payload).await;

    let applied = fields
        .iter()
        .map(|(field, value)| format!("{field}={value}"))
        .collect::<Vec<_>>()
        .join(", ");

    // Prestashop returns HTTP 500 for current_state writes that nonetheless commit — the
    // column is updated before the state-change hooks throw. Re-read before believing a
    // failed response, otherwise a write that took effect is reported as rejected.
    let response = match write {
        Ok(response) => response,
        Err(write_error) => {
            let verified = api.request_raw_resource_by_id("orders", order_id).await;
            match verified {
                Ok(current) if fields_match(&current, fields) => {
                    log::warn!(
                        "Order {order_id} write of {applied} returned an error but the values \
                         are present on re-read, treating as applied: {write_error}"
                    );
                    current
                }
                Ok(_) => return Err(write_error),
                Err(read_error) => {
                    return Err(write_error.context(format!(
                        "could not re-read order {order_id} to confirm the write: {read_error}"
                    )))
                }
            }
        }
    };

    log::info!("Wrote {applied} to order {order_id}");

    Ok(response)
}

/// True when every field in the order XML already holds the value we wrote.
fn fields_match(order_xml: &str, fields: &[(&str, &str)]) -> bool {
    fields.iter().all(|(field, expected)| {
        element_text(order_xml, field).is_some_and(|actual| actual.trim() == *expected)
    })
}
