use anyhow::{anyhow, Context, Result};
use database::SCAFFOLD_URL;
use reqwest::{Client, Method};
use serde::Deserialize;

// PrestaShop API endpoint
const PRESTASHOP_BASE_URL: &str = "https://pclaptops.mojo11.com";

// ============================================
// PrestaShop Models
// ============================================
#[derive(Debug, Deserialize)]
struct OrderSerialsResponse {
    order_serials: Vec<OrderSerialEntry>,
}

#[derive(Debug, Deserialize)]
struct OrderSerialEntry {
    id_order: String,
}

#[derive(Debug, Deserialize)]
struct OrderResponse {
    order: Order,
}

#[derive(Debug, Deserialize)]
struct Order {
    id_customer: String,
}

#[derive(Debug, Deserialize)]
struct CustomerResponse {
    customer: Customer,
}

#[derive(Debug, Deserialize)]
struct Customer {
    firstname: String,
    lastname: String,
}

// ============================================
// Everest Models
// ============================================
#[derive(Debug, Deserialize)]
struct EverestSerialLookupEntry {
    #[serde(rename = "DOCNUM")]
    docnum: String,
}

#[derive(Debug, Deserialize)]
struct EverestOrderHeader {
    #[serde(rename = "ACCT_NAME")]
    acct_name: Option<String>,
    #[serde(rename = "NAME")]
    name: Option<String>,
    #[serde(rename = "FIRST_NAME")]
    first_name: Option<String>,
    #[serde(rename = "LAST_NAME")]
    last_name: Option<String>,
}

// ============================================
// Public API
// ============================================

/// Lookup customer information by OA3 13-digit serial number.
/// Tries PrestaShop first, then falls back to Everest.
/// Returns formatted string "FirstName LastName - OrderID" on success.
pub async fn lookup_customer_by_serial(serial13: &str) -> Result<String> {
    // 1) Try PrestaShop first
    match request_prestashop(serial13).await {
        Ok(result) => {
            log::info!("PrestaShop customer lookup success: {}", result);
            return Ok(result);
        }
        Err(e) => {
            log::warn!("PrestaShop lookup failed: {:?} -> trying Everest fallback", e);
        }
    }

    // 2) Fallback to Everest
    match request_everest(serial13).await {
        Ok(result) => {
            log::info!("Everest customer lookup success: {}", result);
            return Ok(result);
        }
        Err(e) => {
            log::warn!("Everest fallback also failed: {:?}", e);
        }
    }

    Err(anyhow!("Could not find customer for serial: {}", serial13))
}

// ============================================
// PrestaShop Implementation
// ============================================

async fn request_prestashop(serial13: &str) -> Result<String> {
    let client = Client::new();
    let auth_token = database::ISSUE_TOKEN; // Using the existing env var from database crate

    // 1) /api/order_serial - lookup by serial number
    let url1 = format!(
        "{}/api/order_serial?output_format=JSON&display=full&filter[serial_number]=[{}]",
        PRESTASHOP_BASE_URL, serial13
    );

    let resp1 = client
        .get(&url1)
        .header("Authorization", auth_token)
        .send()
        .await?
        .error_for_status()
        .context("PrestaShop order_serial request failed")?
        .json::<OrderSerialsResponse>()
        .await
        .context("Failed to parse order_serial response")?;

    let id_order = resp1
        .order_serials
        .get(0)
        .ok_or_else(|| anyhow!("No order found for serial"))?
        .id_order
        .clone();

    // 2) /api/orders/{id} - get customer ID
    let url2 = format!("{}/api/orders/{}?output_format=JSON", PRESTASHOP_BASE_URL, id_order);

    let resp2 = client
        .get(&url2)
        .header("Authorization", auth_token)
        .send()
        .await?
        .error_for_status()
        .context("PrestaShop orders request failed")?
        .json::<OrderResponse>()
        .await
        .context("Failed to parse order response")?;

    let id_customer = resp2.order.id_customer.trim().to_string();
    if id_customer.is_empty() {
        return Err(anyhow!("Order {} had no id_customer", id_order));
    }

    // 3) /api/customers/{id_customer} - get customer name
    let url3 = format!(
        "{}/api/customers/{}?output_format=JSON",
        PRESTASHOP_BASE_URL, id_customer
    );

    let resp3 = client
        .get(&url3)
        .header("Authorization", auth_token)
        .send()
        .await?
        .error_for_status()
        .context("PrestaShop customers request failed")?
        .json::<CustomerResponse>()
        .await
        .context("Failed to parse customer response")?;

    let first = resp3.customer.firstname.trim();
    let last = resp3.customer.lastname.trim();

    Ok(format!("{} {} - {}", first, last, id_order))
}

// ============================================
// Everest Implementation
// ============================================

/// Perform Everest fallback flow:
/// 1) getDocnumBySerialNumber -> obtain docnum
/// 2) getOrder(docnum) -> obtain order/customer info
/// Returns formatted string "NameOrAcct - DOCNUM"
async fn request_everest(serial13: &str) -> Result<String> {
    let client = Client::builder().build()?;

    let user_email = database::SCAFFOLD_USER;
    let user_password = database::SCAFFOLD_PASS;

    // 1) Lookup DOCNUM by serial
    let docnum = lookup_docnum(&client, user_email, user_password, serial13)
        .await
        .context("Everest docnum lookup failed")?;

    // 2) Fetch order header by DOCNUM
    let header = get_order(&client, user_email, user_password, &docnum)
        .await
        .context("Everest order fetch failed")?;

    let name = header
        .first_name
        .as_ref()
        .and_then(|f| {
            header
                .last_name
                .as_ref()
                .map(|l| format!("{} {}", f.trim(), l.trim()))
        })
        .filter(|s| !s.trim().is_empty())
        .or_else(|| header.name.clone().filter(|s| !s.trim().is_empty()))
        .or_else(|| header.acct_name.clone().filter(|s| !s.trim().is_empty()))
        .unwrap_or_else(|| "Unknown Customer".into());

    Ok(format!("{} - {}", name.trim(), docnum))
}

async fn lookup_docnum(client: &Client, email: &str, password: &str, serial: &str) -> Result<String> {
    let payload = serde_json::json!({
        "action": "everest_call",
        "application": "everest",
        "arg1": serial,
        "call": "getDocnumBySerialNumber",
        "user_email": email,
        "user_password": password,
    });

    let resp = client
        .request(Method::POST, SCAFFOLD_URL)
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await?;

    let status = resp.status();
    let body_txt = resp.text().await?;
    if !status.is_success() {
        return Err(anyhow!("Everest serial lookup HTTP {}: {}", status, body_txt));
    }

    let parsed: serde_json::Value =
        serde_json::from_str(&body_txt).context("Parsing Everest serial lookup JSON")?;
    let arr = parsed
        .as_array()
        .ok_or_else(|| anyhow!("Everest serial lookup unexpected JSON shape"))?;
    let first = arr
        .get(0)
        .ok_or_else(|| anyhow!("No entries returned for serial"))?;
    let entry: EverestSerialLookupEntry = serde_json::from_value(first.clone())?;
    if entry.docnum.trim().is_empty() {
        return Err(anyhow!("Empty DOCNUM returned"));
    }
    Ok(entry.docnum)
}

async fn get_order(client: &Client, email: &str, password: &str, docnum: &str) -> Result<EverestOrderHeader> {
    let payload = serde_json::json!({
        "action": "everest_call",
        "application": "everest",
        "arg1": docnum,
        "arg2": true,
        "call": "getOrder",
        "user_email": email,
        "user_password": password,
    });

    let resp = client
        .request(Method::POST, SCAFFOLD_URL)
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await?;
    let status = resp.status();
    let body_txt = resp.text().await?;
    if !status.is_success() {
        return Err(anyhow!("Everest order fetch HTTP {}: {}", status, body_txt));
    }

    let parsed: serde_json::Value =
        serde_json::from_str(&body_txt).context("Parsing Everest order JSON")?;
    let header_val = parsed
        .get("header")
        .ok_or_else(|| anyhow!("Missing 'header' in order response"))?
        .clone();
    let header: EverestOrderHeader = serde_json::from_value(header_val)?;
    Ok(header)
}
