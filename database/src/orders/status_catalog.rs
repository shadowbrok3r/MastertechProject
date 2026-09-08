//! Runtime status table, replacing three hardcoded copies as the authority.
//!
//! Order statuses were spelled out in three places that disagreed —
//! `OrderState` (20 ids), `gate::status_name` (~37) and a bare id list in
//! `schema/odoo/inventory.rs` (11) — against a live PrestaShop carrying 126.
//! Anything absent from a table rendered as the wrong status or was skipped
//! outright.
//!
//! Both backends can serve the real table, so it is fetched once and cached.
//! The compiled tables stay as an offline fallback: this is a cache, never a
//! dependency, and every lookup works with an empty catalog.

use std::collections::HashMap;
use std::sync::RwLock;

static CATALOG: RwLock<Option<HashMap<i64, String>>> = RwLock::new(None);

/// Name for a legacy status id from the loaded catalog. `None` when the
/// catalog has not been loaded or does not carry the id; callers fall back to
/// the compiled table.
pub fn name(legacy_id: i64) -> Option<String> {
    CATALOG
        .read()
        .ok()?
        .as_ref()?
        .get(&legacy_id)
        .filter(|n| !n.trim().is_empty())
        .cloned()
}

/// Number of statuses cached; 0 when nothing has been loaded.
pub fn len() -> usize {
    CATALOG.read().ok().and_then(|c| c.as_ref().map(HashMap::len)).unwrap_or(0)
}

pub fn is_loaded() -> bool {
    len() > 0
}

/// Replace the cache. Exposed for tests and for callers that already hold a
/// status list.
pub fn install(statuses: HashMap<i64, String>) {
    if let Ok(mut guard) = CATALOG.write() {
        *guard = Some(statuses);
    }
}

/// Drop the cache, so the next lookup falls back to the compiled table.
pub fn clear() {
    if let Ok(mut guard) = CATALOG.write() {
        *guard = None;
    }
}

/// Fetch the status table from whichever backends are configured and cache the
/// merged result. PrestaShop is loaded second because it carries the fuller
/// table, so it wins on any id both define.
///
/// Returns how many statuses were cached. Never errors: an unreachable backend
/// leaves the compiled fallback in place, and a status table is not worth
/// failing a bench session over.
pub async fn refresh() -> usize {
    let mut merged: HashMap<i64, String> = HashMap::new();

    let shopify = crate::xbm::XbmClient::from_env();
    if shopify.configured() {
        match shopify.statuses().await {
            Ok(payload) => {
                for status in payload.statuses {
                    if !status.name.trim().is_empty() {
                        merged.insert(status.legacy_id, status.name);
                    }
                }
            }
            Err(e) => log::warn!("status catalog: Shopify /statuses failed: {e}"),
        }
    }

    match fetch_prestashop_states().await {
        Ok(states) => merged.extend(states),
        Err(e) => log::warn!("status catalog: PrestaShop order_states failed: {e}"),
    }

    if merged.is_empty() {
        log::warn!("status catalog: no backend returned statuses; using the compiled table");
        return 0;
    }
    let count = merged.len();
    install(merged);
    log::info!("status catalog: {count} statuses cached");
    count
}

/// `GET /order_states?display=full`. Names are localised, so each arrives as a
/// list of `{id, value}` rather than a bare string.
async fn fetch_prestashop_states() -> anyhow::Result<HashMap<i64, String>> {
    if !crate::prestashop_configured() {
        return Err(crate::prestashop_unconfigured_err());
    }
    let url = format!(
        "{}/order_states?output_format=JSON&display=full",
        crate::PRESTASHOP_API_URL_WASM
    );
    let body: serde_json::Value = crate::prestashop_get(&url)
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let rows = body
        .get("order_states")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow::anyhow!("order_states missing from the response"))?;

    let mut out = HashMap::new();
    for row in rows {
        let Some(id) = row.get("id").and_then(json_i64) else { continue };
        if let Some(name) = localized_name(row.get("name")) {
            out.insert(id, name);
        }
    }
    Ok(out)
}

fn json_i64(value: &serde_json::Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|s| s.trim().parse().ok()))
}

/// A PrestaShop localised field is `[{id, value}, …]`, but a single-language
/// shop can return a bare string.
fn localized_name(value: Option<&serde_json::Value>) -> Option<String> {
    match value? {
        serde_json::Value::String(s) => Some(s.clone()).filter(|s| !s.trim().is_empty()),
        serde_json::Value::Array(items) => items
            .iter()
            .find_map(|i| i.get("value").and_then(|v| v.as_str()))
            .map(str::to_string)
            .filter(|s| !s.trim().is_empty()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // These tests mutate the process-global catalog, so they run as one.
    #[test]
    fn catalog_overrides_then_clears() {
        clear();
        assert!(!is_loaded());
        assert_eq!(name(225), None);

        install(HashMap::from([
            (225, "Ready to Build".to_string()),
            (999, "  ".to_string()),
        ]));
        assert!(is_loaded());
        assert_eq!(name(225).as_deref(), Some("Ready to Build"));
        // A blank name is not an answer; the caller should fall back.
        assert_eq!(name(999), None);
        assert_eq!(name(12345), None);

        clear();
        assert_eq!(name(225), None);
    }

    #[test]
    fn localized_name_handles_both_shapes() {
        let array = serde_json::json!([{ "id": 1, "value": "Shipped" }]);
        assert_eq!(localized_name(Some(&array)).as_deref(), Some("Shipped"));

        let bare = serde_json::json!("Delivered");
        assert_eq!(localized_name(Some(&bare)).as_deref(), Some("Delivered"));

        assert_eq!(localized_name(Some(&serde_json::json!([]))), None);
        assert_eq!(localized_name(Some(&serde_json::json!(""))), None);
        assert_eq!(localized_name(None), None);
    }

    #[test]
    fn ids_parse_from_string_or_number() {
        assert_eq!(json_i64(&serde_json::json!(224)), Some(224));
        assert_eq!(json_i64(&serde_json::json!("225")), Some(225));
        assert_eq!(json_i64(&serde_json::json!("not-a-number")), None);
    }
}
