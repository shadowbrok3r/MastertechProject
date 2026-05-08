use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{ODOO_API_KEY, ODOO_DB, ODOO_JSONRPC_URL, ODOO_UID};

pub mod inventory;

pub async fn search_odoo_products(search_term: &str) -> anyhow::Result<JsonRpcResponse, anyhow::Error> {
    let client = Client::new();
    let url = ODOO_JSONRPC_URL;
    let db = ODOO_DB;
    let uid: u32 = ODOO_UID.parse().map_err(|_| anyhow::anyhow!("ODOO_UID must be a decimal u32"))?;
    let api_key = ODOO_API_KEY;
    let model = "product.template";
    let method = "search_read";
    let domain = vec![
        vec![
            json!(["default_code", "ilike", search_term]),
            json!(["product_variant_ids.default_code", "ilike", search_term]),
            // json!(["name", "ilike", search_term])
        ]
    ];

    let fields = vec![
        "product_variant_id",
        "qty_available",
        "display_name",
        "virtual_available",
        "list_price",
        "standard_price",
        "default_code",
        "name"
    ];
    let limit = 5; // Replace with desired limit

    let result = call_odoo_api(
        &client,
        url,
        db,
        uid,
        api_key,
        model,
        method,
        domain,
        fields,
        limit,
    )
    .await?;

    Ok(result)

}

pub async fn call_odoo_api(
    client: &Client,
    url: &str,
    db: &str,
    uid: u32,
    api_key: &str,
    model: &str,
    method: &str,
    domain: Vec<Vec<serde_json::Value>>,
    fields: Vec<&str>,
    limit: u32,
) -> Result<JsonRpcResponse, reqwest::Error> {
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "call".to_string(),
        params: Params {
            service: "object".to_string(),
            method: "execute_kw".to_string(),
            args: vec![
                json!(db),
                json!(uid),
                json!(api_key),
                json!(model),
                json!(method),
                json!(domain),
                json!({
                    "fields": fields,
                    "limit": limit
                }),
            ],
        },
        id: 1,
    };

    let response = client
        .post(url)
        .json(&request)
        .send()
        .await?
        .json::<JsonRpcResponse>()
        .await?;

    Ok(response)
}


#[derive(Serialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    method: String,
    params: Params,
    id: u32,
}

#[derive(Serialize)]
struct Params {
    service: String,
    method: String,
    args: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub result: Vec<ExtraInventoryData>
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
    pub default_code: String,
    pub name: String,
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
