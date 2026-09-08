//! Find the customer who owns a hardware serial, through whichever backend
//! holds the order.
//!
//! This is the general form of the OA3 first-run lookup in
//! `Mastertech4.0::filesystem::customer_lookup`, which walks PrestaShop only
//! and answers with a display string. A tech scanning a serial at the counter
//! needs the same question answered against Shopify too, and needs the ids
//! back rather than a formatted name.
//!
//! Both legs are tried in the order [`super::routing_mode`] implies, and a hit
//! from either is returned as [`SerialCustomerMatch`]. An unconfigured backend
//! is skipped, not failed — a shop running one backend must not see errors
//! about the other.

use crate::schema::service_match::{SerialCustomerMatch, SerialOrderRef};
use crate::xbm::XbmClient;
use anyhow::Result;

use super::{BackendKind, RoutingMode};

/// Resolve `serial` to its customer. `Ok(None)` means every configured backend
/// answered and none knew the serial; `Err` means no backend could answer.
pub async fn lookup(serial: &str) -> Result<Option<SerialCustomerMatch>> {
    let serial = serial.trim();
    if serial.is_empty() {
        return Ok(None);
    }

    let mut errors: Vec<String> = Vec::new();
    for backend in order_of_attempt() {
        let attempt = match backend {
            BackendKind::Shopify => shopify_leg(serial).await,
            BackendKind::Prestashop => prestashop_leg(serial).await,
        };
        match attempt {
            Ok(Some(hit)) => return Ok(Some(hit)),
            Ok(None) => {}
            Err(e) => {
                log::warn!("serial lookup: {} leg failed for {serial}: {e:#}", backend.as_str());
                errors.push(format!("{}: {e}", backend.as_str()));
            }
        }
    }

    // Every leg erroring is a different answer from every leg saying "no such
    // serial", and the caller has to be able to tell them apart.
    if !errors.is_empty() {
        anyhow::bail!("no backend could answer for serial {serial} ({})", errors.join("; "));
    }
    Ok(None)
}

/// Preferred backend first. `Auto` tries Shopify first because a serial
/// installed today exists only there.
fn order_of_attempt() -> [BackendKind; 2] {
    match super::routing_mode() {
        RoutingMode::Prestashop => [BackendKind::Prestashop, BackendKind::Shopify],
        RoutingMode::Auto | RoutingMode::Shopify => [BackendKind::Shopify, BackendKind::Prestashop],
    }
}

/// `/serials/{serial}` carries the customer directly; `/orders/resolve?ref=`
/// is the fallback for a serial the install record does not cover but the
/// order search does.
async fn shopify_leg(serial: &str) -> Result<Option<SerialCustomerMatch>> {
    let client = XbmClient::from_env();
    if !client.configured() {
        return Ok(None);
    }

    let mut hit = SerialCustomerMatch {
        serial: serial.to_string(),
        source: BackendKind::Shopify.as_str().to_string(),
        ..Default::default()
    };

    let history = client.serial_history(serial).await?;
    if history.found {
        if let Some(order) = history.shopify.as_ref().and_then(|s| s.order.as_ref()) {
            hit.name = order.customer.clone().unwrap_or_default();
            hit.email = order.email.clone().unwrap_or_default();
            hit.orders.push(SerialOrderRef {
                reference: order.name.clone().unwrap_or_else(|| order.gid.clone()),
                date: order.created_at.clone().unwrap_or_default(),
                matched_by: "serial".into(),
                ..Default::default()
            });
        }
        // Legacy rows name an order, never a customer — they widen the order
        // list without ever answering who owns the serial.
        for legacy in history.prestashop.iter() {
            let Some(id_order) =
                legacy.id_order.as_deref().map(str::trim).filter(|s| !s.is_empty())
            else {
                continue;
            };
            hit.orders.push(SerialOrderRef {
                reference: id_order.to_string(),
                legacy_order_id: id_order.to_string(),
                date: legacy.date_created.clone().unwrap_or_default(),
                matched_by: "serial".into(),
                ..Default::default()
            });
        }
    }

    // Fill the customer from the order detail when the install record had no
    // order, or had one carrying no customer.
    if hit.name.trim().is_empty() || hit.email.trim().is_empty() {
        match client.resolve(serial).await {
            Ok(resolved) => {
                let detail = client.order_detail(&resolved.order_gid).await?;
                if let Some(customer) = detail.order.as_ref().and_then(|o| o.customer.as_ref()) {
                    if hit.name.trim().is_empty() {
                        hit.name = customer.name.clone().unwrap_or_default();
                    }
                    if hit.email.trim().is_empty() {
                        hit.email = customer.email.clone().unwrap_or_default();
                    }
                    hit.phone = customer.phone.clone().unwrap_or_default();
                }
                if !hit.orders.iter().any(|o| o.reference == resolved.name) {
                    hit.orders.push(SerialOrderRef {
                        reference: resolved.name.clone(),
                        legacy_order_id: resolved.legacy_order_id.clone().unwrap_or_default(),
                        matched_by: resolved.matched_by.clone(),
                        status_name: detail
                            .current_status
                            .as_ref()
                            .map(|s| s.name.clone())
                            .unwrap_or_default(),
                        status_id: detail.current_status.as_ref().map(|s| s.legacy_id).unwrap_or(0),
                        ..Default::default()
                    });
                }
            }
            Err(e) if e.is_not_found() => {}
            Err(e) => return Err(e.into()),
        }
    }

    if hit.name.trim().is_empty() && hit.email.trim().is_empty() && hit.orders.is_empty() {
        return Ok(None);
    }
    Ok(Some(hit))
}

async fn prestashop_leg(serial: &str) -> Result<Option<SerialCustomerMatch>> {
    if !crate::prestashop_configured() {
        return Ok(None);
    }
    let pairs = crate::schema::prestashop::Customer::find_customer_by_serial(serial).await?;
    let Some((customer, address)) = pairs.into_iter().next() else {
        return Ok(None);
    };

    let phone = if address.phone.trim().is_empty() {
        address.phone_mobile.clone()
    } else {
        address.phone.clone()
    };

    Ok(Some(SerialCustomerMatch {
        serial: serial.to_string(),
        source: BackendKind::Prestashop.as_str().to_string(),
        name: format!("{} {}", customer.firstname.trim(), customer.lastname.trim())
            .trim()
            .to_string(),
        email: customer.email.clone(),
        phone,
        customer_gid: String::new(),
        id_customer: customer.id.clone(),
        orders: Vec::new(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

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
    async fn blank_serial_is_not_a_lookup() {
        assert!(lookup("   ").await.unwrap().is_none());
    }

    #[test]
    fn a_match_without_either_id_cannot_be_attached() {
        let display_only =
            SerialCustomerMatch { name: "Jane Doe".into(), ..Default::default() };
        assert!(!display_only.has_customer_id());

        let shopify = SerialCustomerMatch {
            customer_gid: "gid://shopify/Customer/1".into(),
            ..Default::default()
        };
        assert!(shopify.has_customer_id());

        let presta =
            SerialCustomerMatch { id_customer: "30412".into(), ..Default::default() };
        assert!(presta.has_customer_id());
    }
}
