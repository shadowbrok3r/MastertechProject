//! Dual-key identity mapping for orders and customers.
//!
//! PrestaShop numbers orders and customers with integers; Shopify uses GIDs.
//! Rather than rewrite Mastertech's integer-keyed SurrealDB, both keys are
//! recorded side by side and this table becomes the routing authority: the
//! backend that owns an order is looked up, not inferred from the key's digit
//! shape. [`OrderKey::parse`](super::OrderKey::parse) stays as the last resort
//! for keys that were never stamped.

use super::{BackendKind, OrderKey};
use crate::{db, SurrealValue};
use serde::{Deserialize, Serialize};

pub const ORDER_IDENTITY_TABLE: &str = "order_identity";
pub const CUSTOMER_IDENTITY_TABLE: &str = "customer_identity";

/// One order's keys in both id spaces.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, SurrealValue)]
pub struct OrderIdentity {
    #[serde(default)]
    #[surreal(default)]
    pub ps_id: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub shopify_gid: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub shopify_order_number: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub everest_doc: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub reference: Option<String>,
    pub backend: String,
}

impl OrderIdentity {
    pub fn backend_kind(&self) -> Option<BackendKind> {
        match self.backend.as_str() {
            "prestashop" => Some(BackendKind::Prestashop),
            "shopify" => Some(BackendKind::Shopify),
            _ => None,
        }
    }

    /// Build the stamp for an order first seen on `backend` under `key`.
    pub fn from_key(key: &OrderKey, backend: BackendKind) -> Self {
        let mut identity = Self {
            backend: backend.as_str().to_string(),
            ..Default::default()
        };
        match key {
            OrderKey::Prestashop(id) => identity.ps_id = Some(id.clone()),
            OrderKey::Everest(doc) => identity.everest_doc = Some(doc.clone()),
            OrderKey::ShopifyOrderNumber(n) => identity.shopify_order_number = Some(n.clone()),
            OrderKey::BuildSerial(s) => identity.reference = Some(s.clone()),
        }
        identity
    }
}

/// One customer's keys in both id spaces.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, SurrealValue)]
pub struct CustomerIdentity {
    #[serde(default)]
    #[surreal(default)]
    pub ps_id: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub shopify_gid: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub cust_code: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub email: Option<String>,
    pub backend: String,
}

/// Field on `order_identity` a lookup matches against.
fn order_lookup_field(key: &OrderKey) -> (&'static str, String) {
    match key {
        OrderKey::Prestashop(id) => ("ps_id", id.clone()),
        OrderKey::Everest(doc) => ("everest_doc", doc.clone()),
        OrderKey::ShopifyOrderNumber(n) => ("shopify_order_number", n.clone()),
        OrderKey::BuildSerial(s) => ("reference", s.clone()),
    }
}

/// Look up the stamped identity for `key`, if one exists.
pub async fn find_order(key: &OrderKey) -> anyhow::Result<Option<OrderIdentity>> {
    let (field, value) = order_lookup_field(key);
    let sql = format!("SELECT * FROM {ORDER_IDENTITY_TABLE} WHERE {field} = $value LIMIT 1");
    let mut res = db().query(sql).bind(("value", value)).await?.check()?;
    Ok(res.take::<Vec<OrderIdentity>>(0)?.into_iter().next())
}

/// The backend that owns `key` according to the identity table. `None` means
/// the order was never stamped, and the caller should fall back to routing
/// mode then key shape.
pub async fn resolve_backend(key: &OrderKey) -> Option<BackendKind> {
    match find_order(key).await {
        Ok(Some(identity)) => identity.backend_kind(),
        Ok(None) => None,
        Err(e) => {
            // A lookup failure must not silently reroute an order.
            log::warn!("order_identity lookup failed for {}: {e}", key.display());
            None
        }
    }
}

/// Non-empty keys only. A `None` field must never reach `MERGE`, or stamping
/// an order by one id space would wipe the id it already had in the other.
fn present_keys<'a>(pairs: &[(&'a str, Option<&String>)]) -> Vec<(&'a str, String)> {
    pairs
        .iter()
        .filter_map(|(field, value)| {
            value
                .filter(|s| !s.trim().is_empty())
                .map(|s| (*field, s.clone()))
        })
        .collect()
}

/// Upsert one identity row, matching on the first non-empty key.
async fn stamp(table: &str, keys: Vec<(&str, String)>, backend: &str) -> anyhow::Result<()> {
    let Some((match_field, match_value)) = keys.first().cloned() else {
        anyhow::bail!("{table} needs at least one non-empty key");
    };

    let mut data = std::collections::BTreeMap::<String, String>::new();
    for (field, value) in keys {
        data.insert(field.to_string(), value);
    }
    data.insert("backend".into(), backend.to_string());

    let existing: Option<crate::schema::RecordId> = db()
        .query(format!(
            "SELECT VALUE id FROM {table} WHERE {match_field} = $value LIMIT 1"
        ))
        .bind(("value", match_value))
        .await?
        .check()?
        .take::<Vec<crate::schema::RecordId>>(0)?
        .into_iter()
        .next();

    match existing {
        Some(id) => {
            db().query("UPDATE $id MERGE $data; UPDATE $id SET updated_at = time::now();")
                .bind(("id", id))
                .bind(("data", data))
                .await?
                .check()?;
        }
        None => {
            db().query(format!("CREATE {table} CONTENT $data"))
                .bind(("data", data))
                .await?
                .check()?;
        }
    }
    Ok(())
}

/// Record which backend owns an order, keyed on whichever id the caller has.
/// Re-stamping an order updates the row rather than adding a second one, and
/// never clears an id space the row already carried.
pub async fn stamp_order(identity: &OrderIdentity) -> anyhow::Result<()> {
    let keys = present_keys(&[
        ("ps_id", identity.ps_id.as_ref()),
        ("shopify_gid", identity.shopify_gid.as_ref()),
        ("shopify_order_number", identity.shopify_order_number.as_ref()),
        ("everest_doc", identity.everest_doc.as_ref()),
        ("reference", identity.reference.as_ref()),
    ]);
    stamp(ORDER_IDENTITY_TABLE, keys, &identity.backend).await
}

/// Record a customer's keys across both id spaces.
pub async fn stamp_customer(identity: &CustomerIdentity) -> anyhow::Result<()> {
    let keys = present_keys(&[
        ("ps_id", identity.ps_id.as_ref()),
        ("shopify_gid", identity.shopify_gid.as_ref()),
        ("cust_code", identity.cust_code.as_ref()),
        ("email", identity.email.as_ref()),
    ]);
    stamp(CUSTOMER_IDENTITY_TABLE, keys, &identity.backend).await
}

/// Stamp the identity of an order a backend just returned. Called after a
/// successful lookup, which is the point at which the owning backend stops
/// being a guess. Failures are logged, never propagated — an identity write
/// must not fail an order the tech is already looking at.
pub async fn stamp_from_order(key: &OrderKey, order: &super::QcOrder) {
    let Some(backend) = order.backend else {
        return;
    };
    let mut identity = OrderIdentity::from_key(key, backend);

    match backend {
        BackendKind::Shopify => {
            identity.shopify_gid = order.gid.clone();
            if identity.shopify_order_number.is_none() {
                identity.shopify_order_number =
                    Some(order.reference.trim_start_matches('#').to_string())
                        .filter(|s| !s.is_empty());
            }
            // `legacyPs` on the Shopify order is the PrestaShop id_order, so a
            // single lookup fills both id spaces.
            identity.ps_id = order
                .parent_order_id
                .clone()
                .filter(|s| !s.trim().is_empty());
        }
        BackendKind::Prestashop => {
            if identity.ps_id.is_none() {
                identity.ps_id = Some(order.id.clone()).filter(|s| !s.is_empty());
            }
        }
    }
    if identity.reference.is_none() && !order.reference.trim().is_empty() {
        identity.reference = Some(order.reference.clone());
    }

    if let Err(e) = stamp_order(&identity).await {
        log::warn!("order_identity stamp failed for {}: {e}", key.display());
    }
}

/// Look up a customer by PrestaShop `id_customer` (Mastertech's `cust_code`).
pub async fn find_customer_by_ps_id(ps_id: &str) -> anyhow::Result<Option<CustomerIdentity>> {
    let sql =
        format!("SELECT * FROM {CUSTOMER_IDENTITY_TABLE} WHERE ps_id = $value OR cust_code = $value LIMIT 1");
    let mut res = db().query(sql).bind(("value", ps_id.to_string())).await?.check()?;
    Ok(res.take::<Vec<CustomerIdentity>>(0)?.into_iter().next())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_key_stamps_the_matching_id_space() {
        let ps = OrderIdentity::from_key(&OrderKey::Prestashop("212345".into()), BackendKind::Prestashop);
        assert_eq!(ps.ps_id.as_deref(), Some("212345"));
        assert_eq!(ps.shopify_order_number, None);
        assert_eq!(ps.backend_kind(), Some(BackendKind::Prestashop));

        let shop = OrderIdentity::from_key(
            &OrderKey::ShopifyOrderNumber("1042".into()),
            BackendKind::Shopify,
        );
        assert_eq!(shop.shopify_order_number.as_deref(), Some("1042"));
        assert_eq!(shop.ps_id, None);
        assert_eq!(shop.backend_kind(), Some(BackendKind::Shopify));

        let everest = OrderIdentity::from_key(&OrderKey::Everest("51234567".into()), BackendKind::Prestashop);
        assert_eq!(everest.everest_doc.as_deref(), Some("51234567"));
    }

    #[test]
    fn lookup_field_matches_the_key_variant() {
        assert_eq!(order_lookup_field(&OrderKey::Prestashop("1".into())).0, "ps_id");
        assert_eq!(order_lookup_field(&OrderKey::Everest("5".into())).0, "everest_doc");
        assert_eq!(
            order_lookup_field(&OrderKey::ShopifyOrderNumber("2".into())).0,
            "shopify_order_number"
        );
        assert_eq!(order_lookup_field(&OrderKey::BuildSerial("XBS-1".into())).0, "reference");
    }

    #[test]
    fn present_keys_drops_empty_and_missing() {
        let blank = String::new();
        let spaces = "   ".to_string();
        let real = "212345".to_string();
        let keys = present_keys(&[
            ("ps_id", Some(&real)),
            ("shopify_gid", None),
            ("reference", Some(&blank)),
            ("everest_doc", Some(&spaces)),
        ]);
        assert_eq!(keys, vec![("ps_id", "212345".to_string())]);
    }

    #[test]
    fn unknown_backend_string_does_not_route() {
        let identity = OrderIdentity {
            backend: "odoo".into(),
            ..Default::default()
        };
        assert_eq!(identity.backend_kind(), None);
    }
}
