use anyhow::{anyhow, Context, Error, Result};
use crossbeam::channel::Sender;
use database::{ODOO_API_KEY, ODOO_JSONRPC_URL, SCAFFOLD_PASS, SCAFFOLD_URL, SCAFFOLD_USER};
use eframe::egui::{Button, Color32, Response, RichText, Ui, Widget};
use egui_data_table::{
    viewer::{DecodeErrorBehavior, RowCodec},
    RowViewer,
};
use egui_extras::Column as TableColumnConfig;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashMap;

/* ------------------------------------ API types ------------------------------------ */

#[derive(Debug, Deserialize, Clone, Default)]
pub struct EverestSerialLookupEntry {
    #[serde(rename = "REFERENCE", default)] pub reference: String,
    #[serde(rename = "DOCNUM", default)] pub docnum: String,
    #[serde(rename = "ITEM_NO", default)] pub item_no: String,
    #[serde(rename = "MFG_SER", default)] pub mfg_ser: String,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct EverestHeader {
    #[serde(rename = "DOC_NO", default)] pub doc_no: String,
    #[serde(rename = "ORDER_DATE", default)] pub order_date: Option<String>,
    #[serde(rename = "DEP", default)] pub dep: String,
    #[serde(rename = "SALES_REP", default)] pub sales_rep: String,
    #[serde(rename = "DOC_ALIAS", default)] pub doc_alias: String,
    #[serde(rename = "TERMS", default)] pub terms: String,
    #[serde(rename = "INV_AMOUNT", default)] pub inv_amount: String,
    #[serde(rename = "PAID", default)] pub paid: String,
    #[serde(rename = "COG", default)] pub cog: String,
    #[serde(rename = "MISC_COG", default)] pub misc_cog: String,
    #[serde(rename = "ACCT_NAME", default)] pub acct_name: String,
    #[serde(rename = "NAME", default)] pub name: String,
    #[serde(rename = "FIRST_NAME", default)] pub first_name: String,
    #[serde(rename = "LAST_NAME", default)] pub last_name: String,
    #[serde(rename = "TEL1", default)] pub tel1: String,
    #[serde(rename = "TEL2", default)] pub tel2: String,
    #[serde(rename = "EMAIL", default)] pub email: String,
    #[serde(rename = "CUST_CODE", default)] pub cust_code: String,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct EverestCustomer {
    #[serde(rename = "CUST_CODE", default)] pub cust_code: String,
    #[serde(rename = "NAME", default)] pub name: String,
    #[serde(rename = "DEP_CODE", default)] pub dep_code: String,
    #[serde(rename = "NUM_INV", default)] pub num_inv: String,
    #[serde(rename = "INV_LIFE", default)] pub inv_life: String,
    #[serde(rename = "CREAT_DATE", default)] pub creat_date: Option<String>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct EverestAddress {
    #[serde(rename = "NAME", default)] pub name: String,
    #[serde(rename = "STREET_ADDRESS", default)] pub street_address: String,
    #[serde(rename = "CITY", default)] pub city: String,
    #[serde(rename = "STATE", default)] pub state: String,
    #[serde(rename = "ZIP", default)] pub zip: String,
    #[serde(rename = "COUNTRY", default)] pub country: String,
    #[serde(rename = "TEL1", default)] pub tel1: String,
    #[serde(rename = "MOBILE_PHONE", default)] pub mobile_phone: String,
    #[serde(rename = "EMAIL", default)] pub email: String,
    #[serde(rename = "FULL_ADDRESS", default)] pub full_address: String,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct EverestItemSerial {
    #[serde(rename = "SERIAL_NO", default)] pub serial_no: Option<String>,
    #[serde(rename = "MFG_SER", default)] pub mfg_ser: Option<String>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct EverestItem {
    #[serde(rename = "SEQUENCE", default)] pub sequence: Option<String>,
    #[serde(rename = "ITEM_CODE", default)] pub item_code: Option<String>,
    #[serde(rename = "ITEM_QTY", default)] pub item_qty: Option<String>,
    #[serde(rename = "QTY_SHIP", default)] pub qty_ship: Option<String>,
    #[serde(rename = "ITEM_PRICE", default)] pub item_price: Option<String>,
    #[serde(rename = "AMOUNT", default)] pub amount: Option<String>,
    #[serde(rename = "NOTE", default)] pub note: Option<String>,
    #[serde(rename = "KIT_CODE", default)] pub kit_code: Option<String>,
    #[serde(default)] pub serials: Vec<EverestItemSerial>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct EverestOrder {
    #[serde(default)] pub header: EverestHeader,
    #[serde(default)] pub customer: EverestCustomer,
    #[serde(default)] pub addresses: Vec<EverestAddress>,
    #[serde(default)] pub items: Vec<EverestItem>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct OdooMoveLine {
    #[serde(default)] pub id: Option<i64>,
    #[serde(default)] pub date: Option<String>,
    #[serde(default)] pub state: Option<String>,
    #[serde(default)] pub qty_done: Option<f64>,
    #[serde(default)] pub reserved_qty: Option<f64>,
    /// May be `false` or a string.
    #[serde(default)] pub reference: Value,
    /// Many2one fields come back as `[id, "Display Name"]` or `false`.
    #[serde(default)] pub location_id: Value,
    #[serde(default)] pub location_dest_id: Value,
    #[serde(default)] pub picking_id: Value,
}

impl OdooMoveLine {
    pub fn reference_str(&self) -> String { value_as_str(&self.reference) }
    pub fn location_name(&self) -> String { many2one_name(&self.location_id) }
    pub fn dest_name(&self) -> String { many2one_name(&self.location_dest_id) }
    pub fn picking_name(&self) -> String { many2one_name(&self.picking_id) }
}

fn value_as_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Bool(false) => String::new(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn many2one_name(v: &Value) -> String {
    // Odoo Many2one: [id, "Display Name"] or false
    if let Some(arr) = v.as_array() {
        if let Some(name) = arr.get(1).and_then(|x| x.as_str()) {
            return name.to_string();
        }
    }
    value_as_str(v)
}

/* ------------------------------------ Async ops ------------------------------------ */

/// Result delivered back to the UI after the scan → DOCNUM → order chain runs.
#[derive(Debug, Clone)]
pub struct EverestLookupResult {
    pub serial: String,
    pub lookup_entries: Vec<EverestSerialLookupEntry>,
    pub order: Option<EverestOrder>,
    pub error: Option<String>,
}

/// Combined scan-a-serial flow: getEverestOrderBySN → getEverestOrder.
pub async fn lookup_everest_order(
    serial: String,
    tx: Sender<EverestLookupResult>,
) -> Result<(), Error> {
    let client = Client::builder().build()?;

    let entries = match get_docnums_by_serial(&client, &serial).await {
        Ok(e) => e,
        Err(e) => {
            let _ = tx.try_send(EverestLookupResult {
                serial,
                lookup_entries: Vec::new(),
                order: None,
                error: Some(format!("Serial lookup failed: {e}")),
            });
            return Ok(());
        }
    };

    let Some(first) = entries.iter().find(|e| !e.docnum.trim().is_empty()) else {
        let _ = tx.try_send(EverestLookupResult {
            serial,
            lookup_entries: entries,
            order: None,
            error: Some("No DOCNUM returned for that serial.".to_string()),
        });
        return Ok(());
    };

    let docnum = first.docnum.clone();
    match get_order_by_docnum(&client, &docnum).await {
        Ok(order) => {
            let _ = tx.try_send(EverestLookupResult {
                serial,
                lookup_entries: entries,
                order: Some(order),
                error: None,
            });
        }
        Err(e) => {
            let _ = tx.try_send(EverestLookupResult {
                serial,
                lookup_entries: entries,
                order: None,
                error: Some(format!("Order fetch failed for DOCNUM {docnum}: {e}")),
            });
        }
    }
    Ok(())
}

async fn get_docnums_by_serial(client: &Client, serial: &str) -> Result<Vec<EverestSerialLookupEntry>> {
    let payload = json!({
        "action": "everest_call",
        "application": "everest",
        "arg1": serial,
        "call": "getDocnumBySerialNumber",
        "user_email": SCAFFOLD_USER,
        "user_password": SCAFFOLD_PASS,
    });

    let resp = client
        .post(SCAFFOLD_URL)
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await?;

    let status = resp.status();
    let body = resp.text().await?;
    if !status.is_success() {
        return Err(anyhow!("HTTP {status}: {body}"));
    }

    let entries: Vec<EverestSerialLookupEntry> = serde_json::from_str(&body)
        .with_context(|| format!("Parsing serial lookup response: {body}"))?;
    Ok(entries)
}

async fn get_order_by_docnum(client: &Client, docnum: &str) -> Result<EverestOrder> {
    let payload = json!({
        "action": "everest_call",
        "application": "everest",
        "arg1": docnum,
        "arg2": true,
        "call": "getOrder",
        "user_email": SCAFFOLD_USER,
        "user_password": SCAFFOLD_PASS,
    });

    let resp = client
        .post(SCAFFOLD_URL)
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await?;

    let status = resp.status();
    let body = resp.text().await?;
    if !status.is_success() {
        return Err(anyhow!("HTTP {status}: {body}"));
    }

    let order: EverestOrder = serde_json::from_str(&body)
        .with_context(|| format!("Parsing order response: body starts with: {}", &body.chars().take(200).collect::<String>()))?;
    Ok(order)
}

/// Result of an Odoo movement history lookup for a single serial.
#[derive(Debug, Clone)]
pub struct OdooSerialHistory {
    pub serial: String,
    pub lot_id: Option<i64>,
    pub product_name: Option<String>,
    pub moves: Vec<OdooMoveLine>,
    pub error: Option<String>,
}

/// Look up the Odoo stock.lot for `serial`, then pull recent stock.move.line history.
pub async fn fetch_serial_movement(
    serial: String,
    tx: Sender<OdooSerialHistory>,
) -> Result<(), Error> {
    let client = Client::builder().build()?;

    let lot = match find_stock_lot(&client, &serial).await {
        Ok(l) => l,
        Err(e) => {
            let _ = tx.try_send(OdooSerialHistory {
                serial,
                lot_id: None,
                product_name: None,
                moves: Vec::new(),
                error: Some(format!("stock.lot lookup failed: {e}")),
            });
            return Ok(());
        }
    };

    let Some((lot_id, product_name)) = lot else {
        let _ = tx.try_send(OdooSerialHistory {
            serial,
            lot_id: None,
            product_name: None,
            moves: Vec::new(),
            error: Some("No matching stock.lot in Odoo for that serial.".to_string()),
        });
        return Ok(());
    };

    let moves = match fetch_move_lines(&client, lot_id).await {
        Ok(m) => m,
        Err(e) => {
            let _ = tx.try_send(OdooSerialHistory {
                serial,
                lot_id: Some(lot_id),
                product_name: Some(product_name),
                moves: Vec::new(),
                error: Some(format!("stock.move.line fetch failed: {e}")),
            });
            return Ok(());
        }
    };

    let _ = tx.try_send(OdooSerialHistory {
        serial,
        lot_id: Some(lot_id),
        product_name: Some(product_name),
        moves,
        error: None,
    });
    Ok(())
}

async fn find_stock_lot(client: &Client, serial: &str) -> Result<Option<(i64, String)>> {
    let payload = json!({
        "jsonrpc": "2.0",
        "method": "call",
        "id": 1,
        "params": {
            "service": "object",
            "method": "execute_kw",
            "args": [
                "pcl_live", 374, ODOO_API_KEY,
                "stock.lot", "search_read",
                [[ ["name", "=", serial] ]],
                { "fields": ["id", "name", "product_id"], "limit": 1 }
            ]
        }
    });
    let resp = client.post(ODOO_JSONRPC_URL).json(&payload).send().await?;
    let v: Value = resp.json().await?;
    let arr = v.get("result").and_then(|r| r.as_array()).cloned().unwrap_or_default();
    let Some(first) = arr.into_iter().next() else { return Ok(None); };
    let id = first.get("id").and_then(|i| i.as_i64()).ok_or_else(|| anyhow!("stock.lot missing id"))?;
    let product_name = many2one_name(first.get("product_id").unwrap_or(&Value::Null));
    Ok(Some((id, product_name)))
}

async fn fetch_move_lines(client: &Client, lot_id: i64) -> Result<Vec<OdooMoveLine>> {
    let payload = json!({
        "jsonrpc": "2.0",
        "method": "call",
        "id": 2,
        "params": {
            "service": "object",
            "method": "execute_kw",
            "args": [
                "pcl_live", 374, ODOO_API_KEY,
                "stock.move.line", "search_read",
                [[ ["lot_id", "=", lot_id] ]],
                {
                    "fields": [
                        "id", "date", "state", "qty_done", "reserved_qty",
                        "reference", "location_id", "location_dest_id", "picking_id"
                    ],
                    "limit": 50,
                    "order": "date desc"
                }
            ]
        }
    });
    let resp = client.post(ODOO_JSONRPC_URL).json(&payload).send().await?;
    let v: Value = resp.json().await?;
    let result = v.get("result").cloned().unwrap_or(Value::Null);
    let moves: Vec<OdooMoveLine> = serde_json::from_value(result)?;
    Ok(moves)
}

/* ------------------------------ Everest customer / order-list ------------------------------ */

/// A loosely-typed Everest record (one row from a list endpoint). We don't
/// have published schemas for the customer / order-list calls, so each row is
/// kept as the raw JSON object and fields are pulled by name with a
/// case-insensitive fallback.
pub type EverestRow = Map<String, Value>;

/// Pull the first non-empty value for any of `keys` from `row`. Tries exact
/// matches first, then case-insensitive matches.
pub fn row_str(row: &EverestRow, keys: &[&str]) -> String {
    for k in keys {
        if let Some(v) = row.get(*k) {
            let s = value_as_str(v);
            if !s.is_empty() {
                return s;
            }
        }
    }
    for k in keys {
        for (rk, rv) in row.iter() {
            if rk.eq_ignore_ascii_case(k) {
                let s = value_as_str(rv);
                if !s.is_empty() {
                    return s;
                }
            }
        }
    }
    String::new()
}

/// Best-effort display name for a customer/order row.
pub fn row_customer_name(row: &EverestRow) -> String {
    let first = row_str(row, &["FIRST_NAME"]);
    let last = row_str(row, &["LAST_NAME"]);
    let fl = format!("{} {}", first.trim(), last.trim());
    if !fl.trim().is_empty() {
        return fl.trim().to_string();
    }
    let name = row_str(row, &["NAME"]);
    if !name.is_empty() {
        return name;
    }
    let acct = row_str(row, &["ACCT_NAME"]);
    if !acct.is_empty() {
        return acct;
    }
    "Unknown".to_string()
}

pub fn row_cust_code(row: &EverestRow) -> String {
    row_str(row, &["CUST_CODE", "CUSTCODE", "CUST_NO"])
}

pub fn row_doc_no(row: &EverestRow) -> String {
    row_str(row, &["DOC_NO", "DOCNUM", "DOC_NUM", "ID_ORDER", "ORDER_NO"])
}

/// Customer search result delivered to the UI.
#[derive(Debug, Clone, Default)]
pub struct EverestCustomerSearchResult {
    pub query: String,
    pub by_email: bool,
    pub customers: Vec<EverestRow>,
    /// For email searches we already have the orders, so cache them per
    /// customer code to avoid a second round-trip when the user drills in.
    pub prefetched_orders: HashMap<String, Vec<EverestRow>>,
    pub error: Option<String>,
}

/// Orders-for-a-customer result delivered to the UI.
#[derive(Debug, Clone, Default)]
pub struct EverestCustomerOrdersResult {
    pub cust_code: String,
    pub orders: Vec<EverestRow>,
    pub error: Option<String>,
}

/// Generic Everest list call: posts `call` with positional `args` (arg1,
/// arg2, ...) and parses the response into a list of records.
async fn everest_list_call(client: &Client, call: &str, args: &[Value]) -> Result<Vec<EverestRow>> {
    let mut payload = Map::new();
    payload.insert("action".into(), json!("everest_call"));
    payload.insert("application".into(), json!("everest"));
    payload.insert("call".into(), json!(call));
    payload.insert("user_email".into(), json!(SCAFFOLD_USER));
    payload.insert("user_password".into(), json!(SCAFFOLD_PASS));
    for (i, a) in args.iter().enumerate() {
        payload.insert(format!("arg{}", i + 1), a.clone());
    }

    let resp = client
        .post(SCAFFOLD_URL)
        .header("Content-Type", "application/json")
        .json(&Value::Object(payload))
        .send()
        .await?;

    let status = resp.status();
    let body = resp.text().await?;
    if !status.is_success() {
        return Err(anyhow!("HTTP {status}: {body}"));
    }
    parse_everest_rows(&body)
}

/// Parse an Everest list response. Accepts a top-level array, an object that
/// wraps the list under some key, or a single object (treated as one row).
fn parse_everest_rows(body: &str) -> Result<Vec<EverestRow>> {
    let v: Value = serde_json::from_str(body).with_context(|| {
        format!(
            "Parsing Everest list response: {}",
            body.chars().take(200).collect::<String>()
        )
    })?;
    match v {
        Value::Array(arr) => Ok(arr.into_iter().filter_map(|x| x.as_object().cloned()).collect()),
        Value::Object(map) => {
            for (_k, val) in map.iter() {
                if let Some(arr) = val.as_array() {
                    return Ok(arr.iter().filter_map(|x| x.as_object().cloned()).collect());
                }
            }
            Ok(vec![map])
        }
        _ => Ok(Vec::new()),
    }
}

/// Search Everest for customers. Phone searches run every formatted variant
/// (matching the rest of the app's phone lookups) through `getCustomerByPhone`
/// and dedupe. Email searches use `getOrdersByEmail`, grouping the returned
/// orders into a synthetic customer list (and caching those orders).
pub async fn search_everest_customers(
    query: String,
    by_email: bool,
    tx: Sender<EverestCustomerSearchResult>,
) -> Result<(), Error> {
    let client = Client::builder().build()?;
    let mut out = EverestCustomerSearchResult {
        query: query.clone(),
        by_email,
        ..Default::default()
    };

    if by_email {
        // getOrdersByEmail returns lightweight order rows with NO customer
        // code, so we enrich a single customer from the first order's full
        // header (which carries CUST_CODE + name). The drill-down then uses
        // getOrdersByCustomerId like the phone path, giving richer rows.
        match everest_list_call(&client, "getOrdersByEmail", &[json!(query.trim())]).await {
            Ok(orders) if !orders.is_empty() => {
                let mut cust = EverestRow::new();
                let first_order_no = row_doc_no(&orders[0]);
                if !first_order_no.is_empty() {
                    if let Ok(order) = get_order_by_docnum(&client, &first_order_no).await {
                        let h = &order.header;
                        cust.insert("CUST_CODE".into(), json!(h.cust_code));
                        cust.insert("NAME".into(), json!(h.name));
                        cust.insert("FIRST_NAME".into(), json!(h.first_name));
                        cust.insert("LAST_NAME".into(), json!(h.last_name));
                        cust.insert("ACCT_NAME".into(), json!(h.acct_name));
                        cust.insert("TEL1".into(), json!(h.tel1));
                        cust.insert("EMAIL".into(), json!(h.email));
                    }
                }
                if row_cust_code(&cust).is_empty() {
                    // Enrichment failed; fall back to the lightweight orders
                    // under a synthetic (email) key so the drill-down still
                    // works without a customer code.
                    cust.insert("CUST_CODE".into(), json!(query.trim()));
                    cust.insert("EMAIL".into(), json!(query.trim()));
                    out.prefetched_orders
                        .insert(query.trim().to_string(), orders.clone());
                }
                cust.insert("total_documents".into(), json!(orders.len().to_string()));
                out.customers.push(cust);
            }
            Ok(_) => {}
            Err(e) => out.error = Some(format!("Email lookup failed: {e}")),
        }
    } else {
        let combos = database::schema::utilities::format_us_phone_number(&query);
        let combos = if combos.is_empty() {
            vec![query.trim().to_string()]
        } else {
            combos
        };
        let mut seen: Vec<String> = Vec::new();
        let mut last_err: Option<String> = None;
        for combo in combos.iter() {
            match everest_list_call(&client, "getCustomerByPhone", &[json!(combo)]).await {
                Ok(rows) => {
                    for r in rows.into_iter() {
                        let code = row_cust_code(&r);
                        let key = if code.is_empty() {
                            row_customer_name(&r)
                        } else {
                            code
                        };
                        if seen.contains(&key) {
                            continue;
                        }
                        seen.push(key);
                        out.customers.push(r);
                    }
                }
                Err(e) => last_err = Some(format!("Phone lookup failed: {e}")),
            }
        }
        if out.customers.is_empty() {
            out.error = last_err;
        }
    }

    let _ = tx.try_send(out);
    Ok(())
}

/// Fetch every order on a customer's account via `getOrdersByCustomerId`.
pub async fn fetch_customer_orders(
    cust_code: String,
    tx: Sender<EverestCustomerOrdersResult>,
) -> Result<(), Error> {
    let client = Client::builder().build()?;
    let out = match everest_list_call(
        &client,
        "getOrdersByCustomerId",
        &[json!(cust_code), json!(200)],
    )
    .await
    {
        Ok(orders) => EverestCustomerOrdersResult {
            cust_code,
            orders,
            error: None,
        },
        Err(e) => EverestCustomerOrdersResult {
            cust_code,
            orders: Vec::new(),
            error: Some(format!("Orders lookup failed: {e}")),
        },
    };
    let _ = tx.try_send(out);
    Ok(())
}

/// Fetch a full Everest order directly by DOCNUM (used when a sales-order
/// number is clicked in the customer-orders list). Reuses the same
/// `EverestLookupResult` channel as the serial flow.
pub async fn lookup_everest_order_by_docnum(
    docnum: String,
    tx: Sender<EverestLookupResult>,
) -> Result<(), Error> {
    let client = Client::builder().build()?;
    match get_order_by_docnum(&client, &docnum).await {
        Ok(order) => {
            let _ = tx.try_send(EverestLookupResult {
                serial: String::new(),
                lookup_entries: Vec::new(),
                order: Some(order),
                error: None,
            });
        }
        Err(e) => {
            let _ = tx.try_send(EverestLookupResult {
                serial: String::new(),
                lookup_entries: Vec::new(),
                order: None,
                error: Some(format!("Order fetch failed for DOCNUM {docnum}: {e}")),
            });
        }
    }
    Ok(())
}

/* ------------------------------------ Row data + viewer ------------------------------------ */

/// Flat per-serial row for the central items table. Items with multiple serials
/// produce multiple rows sharing the item code/qty/price; items with no serials
/// produce a single row with empty serial fields.
#[derive(Default, Serialize, Clone, Debug)]
pub struct EverestItemRow {
    pub sequence: String,
    pub item_code: String,
    pub qty: f64,
    pub unit_price: f64,
    pub stock_serial: String,
    pub mfg_serial: String,
    pub kit_code: String,
}

pub fn order_to_rows(order: &EverestOrder) -> Vec<EverestItemRow> {
    let mut rows = Vec::new();
    for item in order.items.iter() {
        let item_code = item.item_code.clone().unwrap_or_default();
        if item_code.is_empty() { continue; }
        let sequence = item.sequence.clone().unwrap_or_default();
        let qty = item.item_qty.as_deref().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
        let unit_price = item.item_price.as_deref().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
        let kit_code = item.kit_code.clone().unwrap_or_default();

        if item.serials.is_empty() {
            rows.push(EverestItemRow {
                sequence, item_code, qty, unit_price,
                stock_serial: String::new(),
                mfg_serial: String::new(),
                kit_code,
            });
        } else {
            for s in item.serials.iter() {
                rows.push(EverestItemRow {
                    sequence: sequence.clone(),
                    item_code: item_code.clone(),
                    qty, unit_price,
                    stock_serial: s.serial_no.clone().unwrap_or_default(),
                    mfg_serial: s.mfg_ser.clone().unwrap_or_default(),
                    kit_code: kit_code.clone(),
                });
            }
        }
    }
    rows
}

/// Aggregate totals derived from the Everest header.
/// `revenue` is what we billed (`INV_AMOUNT`), `cost` is total cost of goods
/// (`COG` + `MISC_COG`), `profit` = revenue − cost.
#[derive(Debug, Clone, Default)]
pub struct EverestOrderTotals {
    pub revenue: f64,
    pub cost: f64,
    pub profit: f64,
}

impl EverestOrderTotals {
    pub fn margin_pct(&self) -> f64 {
        if self.revenue > 0.0 { (self.profit / self.revenue) * 100.0 } else { 0.0 }
    }
}

pub fn order_totals(order: &EverestOrder) -> EverestOrderTotals {
    let revenue = order.header.inv_amount.parse::<f64>().unwrap_or(0.0);
    let cog = order.header.cog.parse::<f64>().unwrap_or(0.0);
    let misc = order.header.misc_cog.parse::<f64>().unwrap_or(0.0);
    let cost = cog + misc;
    EverestOrderTotals { revenue, cost, profit: revenue - cost }
}

#[derive(Serialize)]
pub struct EverestItemViewer {
    pub filter: String,
    #[serde(skip)]
    pub serial_click_tx: Option<Sender<String>>,
    /// Keys of the currently highlighted rows (for selection stats).
    #[serde(skip)]
    pub selected: std::collections::HashSet<String>,
}

impl Default for EverestItemViewer {
    fn default() -> Self {
        Self { filter: String::new(), serial_click_tx: None, selected: std::collections::HashSet::new() }
    }
}

/// Stable selection key for an Everest line row.
pub fn everest_row_key(r: &EverestItemRow) -> String {
    format!("{}|{}|{}", r.sequence, r.item_code, r.stock_serial)
}

impl RowViewer<EverestItemRow> for EverestItemViewer {
    fn try_create_codec(&mut self, _: bool) -> Option<impl RowCodec<EverestItemRow>> { Some(EverestItemCodec) }

    fn num_columns(&mut self) -> usize { 7 }

    fn on_highlight_change(&mut self, highlighted: &[&EverestItemRow], unhighlighted: &[&EverestItemRow]) {
        for r in unhighlighted.iter() { self.selected.remove(&everest_row_key(r)); }
        for r in highlighted.iter() { self.selected.insert(everest_row_key(r)); }
    }

    fn column_name(&mut self, column: usize) -> std::borrow::Cow<'static, str> {
        ["#", "Item Code", "Qty", "Unit Price", "Stock Serial", "MFG Serial", "Kit"][column].into()
    }

    fn is_sortable_column(&mut self, column: usize) -> bool {
        [true, true, true, true, true, true, true][column]
    }

    fn is_editable_cell(&mut self, _: usize, _: usize, _: &EverestItemRow) -> bool { false }

    fn row_filter_hash(&mut self) -> &impl std::hash::Hash { &self.filter }

    fn filter_row(&mut self, row: &EverestItemRow) -> bool {
        let filter = self.filter.to_uppercase();
        if filter.is_empty() { return true; }
        row.item_code.to_uppercase().contains(&filter)
            || row.stock_serial.to_uppercase().contains(&filter)
            || row.mfg_serial.to_uppercase().contains(&filter)
            || row.kit_code.to_uppercase().contains(&filter)
    }

    fn show_cell_view(&mut self, ui: &mut Ui, row: &EverestItemRow, column: usize) {
        let style = ui.style_mut();
        style.interaction.multi_widget_text_select = false;
        style.interaction.selectable_labels = false;

        match column {
            0 => { ui.label(&row.sequence); }
            1 => {
                if let Some((bracket, name)) = row.item_code.split_once(']') {
                    let parts = bracket.split_terminator('/').collect::<Vec<_>>();
                    ui.horizontal_centered(|ui| {
                        match parts.len() {
                            2 => {
                                ui.colored_label(Color32::LIGHT_GREEN, format!("{}/", parts[0]));
                                ui.colored_label(Color32::from_rgb(42, 195, 222), format!("{}]", parts[1]));
                            }
                            3 => {
                                ui.colored_label(Color32::LIGHT_GREEN, format!("{}/", parts[0]));
                                ui.colored_label(Color32::LIGHT_BLUE, format!("{}/", parts[1]));
                                ui.colored_label(Color32::from_rgb(42, 195, 222), format!("{}]", parts[2]));
                            }
                            _ => { ui.label(&row.item_code); }
                        }
                        ui.add_space(6.);
                        ui.label(name);
                    });
                } else {
                    ui.label(&row.item_code);
                }
            }
            2 => { ui.label(format!("{:.0}", row.qty)); }
            3 => {
                let color = if row.unit_price > 0.0 { Color32::LIGHT_GREEN } else { Color32::GRAY };
                ui.label(RichText::new(format!("$ {:.2}", row.unit_price)).color(color));
            }
            4 => { ui.label(RichText::new(&row.stock_serial).color(Color32::from_rgb(160, 200, 220))); }
            5 => {
                if row.mfg_serial.is_empty() {
                    ui.label(RichText::new("—").color(Color32::GRAY));
                } else {
                    let label = RichText::new(&row.mfg_serial).color(Color32::from_rgb(42, 195, 222));
                    if Button::new(label).ui(ui)
                        .on_hover_text("Click to load Odoo movement history")
                        .clicked()
                    {
                        if let Some(tx) = self.serial_click_tx.as_ref() {
                            let _ = tx.try_send(row.mfg_serial.clone());
                        }
                    }
                }
            }
            6 => {
                if row.kit_code.is_empty() {
                    ui.label(RichText::new("—").color(Color32::GRAY));
                } else {
                    ui.label(RichText::new(&row.kit_code).color(Color32::from_rgb(255, 180, 80)));
                }
            }
            _ => unreachable!(),
        }
    }

    fn show_cell_editor(&mut self, ui: &mut Ui, row: &mut EverestItemRow, column: usize) -> Option<Response> {
        ui.vertical_centered_justified(|ui| {
            match column {
                0 => ui.label(&row.sequence),
                1 => ui.label(&row.item_code),
                2 => ui.label(format!("{}", row.qty)),
                3 => ui.label(format!("{}", row.unit_price)),
                4 => ui.label(&row.stock_serial),
                5 => ui.label(&row.mfg_serial),
                6 => ui.label(&row.kit_code),
                _ => unreachable!(),
            }.into()
        }).inner
    }

    fn set_cell_value(&mut self, src: &EverestItemRow, dst: &mut EverestItemRow, column: usize) {
        match column {
            0 => dst.sequence = src.sequence.clone(),
            1 => dst.item_code = src.item_code.clone(),
            2 => dst.qty = src.qty,
            3 => dst.unit_price = src.unit_price,
            4 => dst.stock_serial = src.stock_serial.clone(),
            5 => dst.mfg_serial = src.mfg_serial.clone(),
            6 => dst.kit_code = src.kit_code.clone(),
            _ => unreachable!(),
        }
    }

    fn compare_cell(&self, l: &EverestItemRow, r: &EverestItemRow, column: usize) -> std::cmp::Ordering {
        match column {
            0 => l.sequence.cmp(&r.sequence),
            1 => l.item_code.cmp(&r.item_code),
            2 => l.qty.partial_cmp(&r.qty).unwrap_or(std::cmp::Ordering::Equal),
            3 => l.unit_price.partial_cmp(&r.unit_price).unwrap_or(std::cmp::Ordering::Equal),
            4 => l.stock_serial.cmp(&r.stock_serial),
            5 => l.mfg_serial.cmp(&r.mfg_serial),
            6 => l.kit_code.cmp(&r.kit_code),
            _ => unreachable!(),
        }
    }

    fn new_empty_row(&mut self) -> EverestItemRow { EverestItemRow::default() }

    fn column_render_config(&mut self, column: usize, _: bool) -> TableColumnConfig {
        let c = TableColumnConfig::auto();
        match column {
            0 => c.resizable(true).at_least(30.).at_most(50.),
            1 => c.resizable(true).at_least(220.).at_most(420.),
            2 => c.resizable(true).at_least(50.).at_most(80.),
            3 => c.resizable(true).at_least(80.).at_most(110.),
            4 => c.resizable(true).at_least(140.).at_most(220.),
            5 => c.resizable(true).at_least(160.).at_most(260.),
            6 => c.resizable(true).at_least(100.).at_most(180.),
            _ => c,
        }
    }
}

struct EverestItemCodec;

impl RowCodec<EverestItemRow> for EverestItemCodec {
    type DeserializeError = &'static str;

    fn encode_column(&mut self, src: &EverestItemRow, column: usize, dst: &mut String) {
        match column {
            0 => dst.push_str(&src.sequence),
            1 => dst.push_str(&src.item_code),
            2 => dst.push_str(&format!("{}", src.qty)),
            3 => dst.push_str(&format!("{}", src.unit_price)),
            4 => dst.push_str(&src.stock_serial),
            5 => dst.push_str(&src.mfg_serial),
            6 => dst.push_str(&src.kit_code),
            _ => unreachable!(),
        }
    }

    fn decode_column(&mut self, src: &str, column: usize, dst: &mut EverestItemRow) -> Result<(), DecodeErrorBehavior> {
        match column {
            0 => dst.sequence = src.to_string(),
            1 => dst.item_code = src.to_string(),
            2 => dst.qty = src.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            3 => dst.unit_price = src.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            4 => dst.stock_serial = src.to_string(),
            5 => dst.mfg_serial = src.to_string(),
            6 => dst.kit_code = src.to_string(),
            _ => unreachable!(),
        }
        Ok(())
    }

    fn create_empty_decoded_row(&mut self) -> EverestItemRow { EverestItemRow::default() }
}
