use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub mod inventory;
pub mod parts;
pub mod stock;

/// Templates a product search returns at most.
const TEMPLATE_LIMIT: u32 = 5;

/// Domain matching every word in the name, or the whole query as internal reference, variant code or barcode.
pub fn template_domain(query: &str) -> Value {
    json!([parts::name_or_fields_domain(
        query,
        &["default_code", "product_variant_ids.default_code", "barcode"]
    )])
}

/// Product templates whose name has every word of `search_term`, or whose code, variant code or barcode contains it.
pub async fn search_odoo_products(search_term: &str) -> anyhow::Result<Vec<ExtraInventoryData>> {
    let rows = parts::search_read(
        "product.template",
        template_domain(search_term),
        json!([
            "product_variant_id",
            "qty_available",
            "display_name",
            "virtual_available",
            "list_price",
            "standard_price",
            "default_code",
            "name"
        ]),
        Some(TEMPLATE_LIMIT),
    )
    .await?;
    rows.into_iter()
        .map(|row| serde_json::from_value(row).map_err(|e| anyhow::anyhow!("Odoo product.template row: {e}")))
        .collect()
}

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct StockData {
    pub result: Vec<RawStockData>,
}

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct SerialData {
    pub result: Vec<SerialInfo>,
}

#[derive(Default, Debug, Serialize, Deserialize, Clone)]
pub struct LotID(pub i32, pub String);

#[derive(Default, Debug, Serialize, Deserialize, Clone)]
pub struct ProductID(pub i32, pub String);

#[derive(Default, Debug, Serialize, Deserialize, Clone)]
pub struct SerialInfo {
    pub id: u64,
    pub bs_prest_ref: BoolOrString,
    // pub bs_sale_line_id: BoolOrString,
    pub product_id: ProductID,
    pub name: String,
}

#[derive(Default, Debug, Serialize, Deserialize, Clone)]
pub struct RawStockData {
    pub available_quantity: f32,
    pub id: u64,
    pub inventory_diff_quantity: f32,
    pub inventory_quantity: f32,
    pub lot_id: LotID,
    pub product_id: ProductID,
    pub quantity: f32,
    pub reserved_quantity: f32,
    pub location_id: LotID,
}

#[derive(Default, Debug, Serialize, Deserialize, Clone)]
pub struct ExtraInventoryData {
    pub display_name: String,   // Display name is a String
    // pub id: f64,                // ID is a positive integer
    pub list_price: f64,        // Monetary value (with decimals), so f64 is appropriate
    pub qty_available: f64,     // Quantities should remain as u64 for non-negative integers
    pub standard_price: f64,    // Monetary value (with decimals), so f64 is appropriate
    pub virtual_available: f64, // Quantities should remain as u64 for non-negative integers
    pub product_variant_id: ProductID,
    #[serde(default, deserialize_with = "false_as_none")]
    pub default_code: Option<String>,
    pub name: String,
}

/// Odoo's `false` or a blank string for an empty char field becomes `None`.
fn false_as_none<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    Ok(match Option::<BoolOrString>::deserialize(deserializer)? {
        Some(BoolOrString::String(s)) if !s.trim().is_empty() => Some(s),
        _ => None,
    })
}

use serde::de::{Deserializer, MapAccess, Visitor};
use serde::ser::Serializer;
use std::fmt;

#[derive(Debug, Clone)]
pub enum BoolOrString {
    Bool(bool),
    String(String),
}

impl Default for BoolOrString {
    fn default() -> Self {
        BoolOrString::Bool(false)
    }
}

// Custom Serialize to output raw values (not tagged enum format)
// This ensures compatibility with Odoo API responses and SurrealDB round-trips
impl Serialize for BoolOrString {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            BoolOrString::Bool(b) => serializer.serialize_bool(*b),
            BoolOrString::String(s) => serializer.serialize_str(s),
        }
    }
}

impl<'de> Deserialize<'de> for BoolOrString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct BoolOrStringVisitor;

        impl<'de> Visitor<'de> for BoolOrStringVisitor {
            type Value = BoolOrString;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a bool, a string, or a tagged enum {\"Bool\": bool} / {\"String\": string}")
            }

            fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
                Ok(BoolOrString::Bool(value))
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(BoolOrString::String(value.to_string()))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(BoolOrString::String(value))
            }

            // Handle tagged enum format: {"Bool": false} or {"String": "value"}
            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                if let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "Bool" => {
                            let value: bool = map.next_value()?;
                            Ok(BoolOrString::Bool(value))
                        }
                        "String" => {
                            let value: String = map.next_value()?;
                            Ok(BoolOrString::String(value))
                        }
                        _ => Err(serde::de::Error::unknown_variant(&key, &["Bool", "String"])),
                    }
                } else {
                    Err(serde::de::Error::custom("expected a non-empty map"))
                }
            }
        }

        deserializer.deserialize_any(BoolOrStringVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// True when every `|`/`&` in a prefix-notation domain has two operands and one expression remains.
    fn well_formed(domain: &Value) -> bool {
        let mut depth = 0;
        for term in domain[0].as_array().expect("positional domain").iter().rev() {
            if matches!(term.as_str(), Some("|" | "&")) {
                if depth < 2 {
                    return false;
                }
                depth -= 1;
            } else {
                depth += 1;
            }
        }
        depth == 1
    }

    #[test]
    fn template_domain_ands_name_words_and_ors_codes() {
        assert_eq!(
            template_domain(" 1TB NVMe "),
            json!([[
                "|",
                "|",
                "|",
                "&",
                ["name", "ilike", "1TB"],
                ["name", "ilike", "NVMe"],
                ["default_code", "ilike", "1TB NVMe"],
                ["product_variant_ids.default_code", "ilike", "1TB NVMe"],
                ["barcode", "ilike", "1TB NVMe"]
            ]])
        );
        for query in ["", "SSD", "1TB NVMe", "Samsung 990 Pro 2TB"] {
            assert!(well_formed(&template_domain(query)), "malformed domain for {query:?}");
        }
    }

    #[test]
    fn default_code_false_or_blank_is_none() {
        let row = |code: Value| {
            json!({
                "display_name": "Samsung 990 Pro 1TB NVMe",
                "list_price": 129.99,
                "qty_available": 3.0,
                "standard_price": 88.5,
                "virtual_available": 2.0,
                "product_variant_id": [4411, "Samsung 990 Pro 1TB NVMe"],
                "default_code": code,
                "name": "Samsung 990 Pro 1TB NVMe"
            })
        };
        let decode = |v: Value| serde_json::from_value::<ExtraInventoryData>(v).expect("row decodes");
        assert_eq!(decode(row(json!(false))).default_code, None);
        assert_eq!(decode(row(json!(" "))).default_code, None);
        assert_eq!(decode(row(json!("MZ-V9P1T0"))).default_code.as_deref(), Some("MZ-V9P1T0"));
    }
}
