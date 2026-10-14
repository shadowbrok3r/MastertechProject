//! A staff member's full PrestaShop employee record, credential fields removed.

use reqwest::header::ACCEPT;
use serde_json::Value;

use super::{EmployeeDirectory, EmployeeRecord, employee_directory};
use crate::schema::Store;
use crate::schema::prestashop::{Employee, localized_text, truncate_body};

/// Keys dropped from every employee record before it is kept or shown.
const HIDDEN_KEYS: [&str; 3] = ["passwd", "reset_password_token", "reset_password_validity"];

/// An employee as PrestaShop stores it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmployeeProfile {
    pub record: EmployeeRecord,
    /// PrestaShop profile (permission group) name, such as "SuperAdmin".
    pub profile_name: Option<String>,
    /// Every remaining field as key and display text, sorted by key.
    pub fields: Vec<(String, String)>,
}

impl EmployeeProfile {
    pub fn store(&self) -> Option<Store> {
        self.record.store()
    }

    /// Display text of field `key`, if PrestaShop sent it.
    pub fn field(&self, key: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

/// Loads employee `id`; by-id reads are never scoped to one shop.
pub async fn employee_profile(id: u64) -> anyhow::Result<EmployeeProfile> {
    if !crate::prestashop_configured() {
        return Err(crate::prestashop_unconfigured_err());
    }
    let mut body = get_json(&format!("employees/{id}")).await?;
    let employee = body
        .get_mut("employee")
        .map(Value::take)
        .ok_or_else(|| anyhow::anyhow!("PrestaShop has no employee {id}"))?;
    let profile_id = employee
        .get("id_profile")
        .and_then(display_text)
        .filter(|p| p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty());
    let profile_name = match profile_id {
        Some(pid) => match get_json(&format!("profiles/{pid}")).await {
            Ok(profile) => localized_text(profile.get("profile").and_then(|p| p.get("name"))),
            Err(e) => {
                log::warn!("PrestaShop profile {pid} for employee {id} failed to load: {e:#}");
                None
            }
        },
        None => None,
    };
    profile_from_json(employee, profile_name)
}

/// Finds an employee by email, then loads the full record; `Ok(None)` is a miss.
pub async fn employee_profile_by_email(email: &str) -> anyhow::Result<Option<EmployeeProfile>> {
    match employee_directory().by_email(email).await? {
        Some(record) => employee_profile(record.id).await.map(Some),
        None => Ok(None),
    }
}

/// GET `path` under the PrestaShop API as JSON; errors name the path, never the base URL.
async fn get_json(path: &str) -> anyhow::Result<Value> {
    let separator = if path.contains('?') { '&' } else { '?' };
    let url = format!(
        "{}/{path}{separator}output_format=JSON",
        crate::PRESTASHOP_API_URL_WASM
    );
    let response = crate::prestashop_get(url)
        .header(ACCEPT, "application/json")
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("GET {path} failed: {}", e.without_url()))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| anyhow::anyhow!("GET {path} failed: {}", e.without_url()))?;
    if !status.is_success() {
        anyhow::bail!("GET {path} -> HTTP {status}: {}", truncate_body(&body));
    }
    serde_json::from_str(&body)
        .map_err(|e| anyhow::anyhow!("GET {path} -> not JSON ({e}): {}", truncate_body(&body)))
}

/// Builds a profile from an `employee` JSON object, dropping [`HIDDEN_KEYS`].
pub fn profile_from_json(
    employee: Value,
    profile_name: Option<String>,
) -> anyhow::Result<EmployeeProfile> {
    let Value::Object(mut map) = employee else {
        anyhow::bail!("the employee record is not an object");
    };
    for key in HIDDEN_KEYS {
        map.remove(key);
    }
    let core: Employee = serde_json::from_value(Value::Object(map.clone()))
        .map_err(|e| anyhow::anyhow!("the employee record is missing a core field: {e}"))?;
    let record = EmployeeRecord::try_from(core)?;
    let mut fields: Vec<(String, String)> = map
        .iter()
        .map(|(key, value)| (key.clone(), display_text(value).unwrap_or_default()))
        .collect();
    fields.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(EmployeeProfile {
        record,
        profile_name,
        fields,
    })
}

/// A JSON value as one line of text; localised arrays give their first value.
fn display_text(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Array(_) => localized_text(Some(value)).or_else(|| Some(value.to_string())),
        Value::Object(_) => Some(value.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn employee() -> Value {
        json!({
            "id": 48,
            "id_lang": "1",
            "lastname": "Lees ",
            "firstname": "Logan",
            "email": "Logan.Lees@pclaptops.com",
            "passwd": "$2y$10$secret",
            "reset_password_token": "abc",
            "reset_password_validity": "2026-01-01 00:00:00",
            "active": "1",
            "id_profile": "1",
            "id_store": "7",
            "last_connection_date": "2026-10-13",
            "bo_theme": ["default"],
            "associations": { "shops": [] }
        })
    }

    #[test]
    fn credential_fields_are_dropped() {
        let profile = profile_from_json(employee(), Some("SuperAdmin".into())).expect("profile");
        for key in HIDDEN_KEYS {
            assert!(profile.field(key).is_none(), "{key} survived");
        }
        let rendered = format!("{profile:?}");
        assert!(
            !rendered.contains("secret"),
            "the hash leaked into the profile"
        );
    }

    #[test]
    fn the_core_record_is_parsed_and_normalized() {
        let profile = profile_from_json(employee(), None).expect("profile");
        assert_eq!(profile.record.id, 48);
        assert_eq!(profile.record.email, "logan.lees@pclaptops.com");
        assert_eq!(profile.record.name(), "Logan Lees");
        assert!(profile.record.active);
        assert_eq!(profile.store(), Some(Store::RIV));
    }

    #[test]
    fn every_other_field_is_listed_as_text_sorted_by_key() {
        let profile = profile_from_json(employee(), None).expect("profile");
        assert_eq!(profile.field("id"), Some("48"));
        assert_eq!(profile.field("last_connection_date"), Some("2026-10-13"));
        assert_eq!(profile.field("associations"), Some(r#"{"shops":[]}"#));
        let keys: Vec<&str> = profile.fields.iter().map(|(k, _)| k.as_str()).collect();
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        assert_eq!(keys, sorted);
    }

    #[test]
    fn a_record_without_core_fields_is_an_error() {
        assert!(profile_from_json(json!({ "id": 1 }), None).is_err());
        assert!(profile_from_json(json!([1, 2]), None).is_err());
    }
}
