use anyhow::{anyhow, Context, Result};
use database::schema::prestashop::order::{Order as FullOrder, OrderState};
use database::schema::service_match::{
    OpenServiceCandidate, PrestaSpecsSnapshot, PrestashopCustomerMatch,
};
use database::{PRESTASHOP_API_URL_WASM, SCAFFOLD_URL};
use reqwest::{Client, Method};
use serde::Deserialize;
use std::sync::{Mutex, OnceLock};

/// In-memory cache of the last successful PrestaShop lookup for this
/// running Mastertech client.  Populated by `first_run.rs` after the
/// OA3 lookup succeeds; consumed by the
/// `Cmd::RequestOpenServiceCandidates` handler when the admin opens
/// the suggestion modal.
///
/// Cleared by the admin-side "Refresh suggestions" button (which
/// re-issues the lookup), and survives only the lifetime of the
/// process — by design, per the product decision to keep suggestions
/// in-memory only (no DB persistence of transient suggestions).
pub static OPEN_SERVICE_CACHE: OnceLock<Mutex<Option<CachedOpenServiceLookup>>> =
    OnceLock::new();

/// Snapshot of one OA3 → PrestaShop resolution.  `match_` is `None`
/// when PrestaShop failed and we only have an Everest friendly_name
/// (the admin's modal should fall back to manual relink in that case).
#[derive(Debug, Clone)]
pub struct CachedOpenServiceLookup {
    pub match_: Option<PrestashopCustomerMatch>,
    pub candidates: Vec<OpenServiceCandidate>,
}

/// Replace the in-memory open-service cache.  Called by
/// `first_run.rs` after each successful (or partial) OA3 lookup —
/// including the admin-triggered "Refresh suggestions" path.
pub fn set_open_service_cache(value: CachedOpenServiceLookup) {
    let cell = OPEN_SERVICE_CACHE.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = cell.lock() {
        *guard = Some(value);
    }
}

/// Read the current open-service cache (if populated).  Returns
/// `None` when first-install hasn't completed the lookup yet, or when
/// every lookup so far has hard-failed.
pub fn get_open_service_cache() -> Option<CachedOpenServiceLookup> {
    let cell = OPEN_SERVICE_CACHE.get_or_init(|| Mutex::new(None));
    cell.lock().ok().and_then(|g| g.clone())
}

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

#[derive(Debug, Deserialize)]
struct CustomerOrdersResponse {
    orders: Vec<CustomerOrderRef>,
}

#[derive(Debug, Deserialize)]
struct CustomerOrderRef {
    id: serde_json::Value, // PrestaShop returns int or string depending on display
}

// `PrestashopCustomerMatch`, `OpenServiceCandidate`, and
// `PrestaSpecsSnapshot` now live in `database::schema::service_match`
// so the `displays::Cmd` wire enum can reference them without a
// cyclic crate dep.  Re-export through the imports above.

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
///
/// Kept for the existing call sites that only care about the display
/// name.  New code that wants the structured match (customer id, open
/// service orders, etc.) should call
/// [`lookup_customer_and_open_orders`].
pub async fn lookup_customer_by_serial(serial13: &str) -> Result<String> {
    // 1) Try PrestaShop first
    match request_prestashop(serial13).await {
        Ok(m) => {
            log::info!("PrestaShop customer lookup success: {}", m.friendly_name);
            return Ok(m.friendly_name);
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

/// Full PrestaShop lookup: resolves the customer by OA3 serial, then
/// fetches the customer's *open* service orders (anything whose state
/// is not `AcceptedByOdoo`) so the admin can pick which one — if any —
/// to bind to this machine.
///
/// The function intentionally does *not* fall back to Everest for the
/// open-order list: Everest tracks delivered orders, not the open
/// service queue we care about for repair check-ins.  If PrestaShop
/// fails outright we return the error and let the caller decide whether
/// to surface the friendly_name via Everest separately.
pub async fn lookup_customer_and_open_orders(
    serial13: &str,
) -> Result<(PrestashopCustomerMatch, Vec<OpenServiceCandidate>)> {
    let m = request_prestashop(serial13)
        .await
        .context("PrestaShop customer lookup failed")?;

    let candidates = lookup_open_service_orders_for_customer(&m.id_customer)
        .await
        .unwrap_or_else(|e| {
            log::warn!(
                "PrestaShop open-order lookup failed for customer {}: {e:?} \
                 (returning empty candidates list — friendly_name still resolved)",
                m.id_customer
            );
            Vec::new()
        });

    log::info!(
        "PrestaShop open-order lookup: customer={} candidates={}",
        m.id_customer,
        candidates.len()
    );
    Ok((m, candidates))
}

/// Fetch every order for `id_customer` from PrestaShop, parse each one
/// in full, and return the ones whose state is *not* `AcceptedByOdoo`.
///
/// Each candidate carries the bits the admin modal needs: order number,
/// doc_alias (i.e. customer-facing label like "Sales Order" /
/// "Configurator"), the timestamps, the check-in notes, the state
/// name, and the parsed PrestaShop specs (used for the live-vs-presta
/// merge preview before the admin approves computer-row creation).
pub async fn lookup_open_service_orders_for_customer(
    id_customer: &str,
) -> Result<Vec<OpenServiceCandidate>> {
    if id_customer.trim().is_empty() {
        return Ok(Vec::new());
    }
    let client = Client::new();

    // List of order IDs for this customer.  `display=[id]` keeps the
    // payload small — we hit the full /orders/{id} endpoint per match
    // below to get the order_service association and current_state.
    let list_url = format!(
        "{PRESTASHOP_API_URL_WASM}/orders?output_format=JSON&filter[id_customer]={id_customer}&display=[id]"
    );
    log::info!("PrestaShop customer orders list URL: {list_url}");
    let list: CustomerOrdersResponse = client
        .get(&list_url)
        .send()
        .await?
        .error_for_status()
        .context("PrestaShop orders listing failed")?
        .json()
        .await
        .context("Failed to parse PrestaShop orders listing")?;

    // Walk each order, full-detail fetch, parse with the shared
    // `Order` struct from the database crate so we get
    // extract_drives/extract_motherboard/extract_os/extract_specs for
    // free.  Best-effort: an individual order parse error is logged
    // and skipped so one bad order can't sink the whole picker.
    let mut candidates = Vec::new();
    for entry in list.orders.iter() {
        let id = match &entry.id {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Number(n) => n.to_string(),
            _ => continue,
        };
        match fetch_full_order(&client, &id).await {
            Ok(order) => {
                let state = OrderState::state_from_id_str(&order.current_state);
                if matches!(state, OrderState::AcceptedByOdoo) {
                    log::debug!(
                        "PrestaShop order {id} is AcceptedByOdoo; skipping"
                    );
                    continue;
                }
                let candidate = build_candidate(order, state).await;
                candidates.push(candidate);
            }
            Err(e) => {
                log::warn!("PrestaShop full-order fetch failed for {id}: {e:?}");
            }
        }
    }

    // Newest-first so the admin modal can show the most recent order at
    // the top.  `date_upd` is "YYYY-MM-DD HH:MM:SS" — lexical sort is
    // equivalent to chronological for that format.
    candidates.sort_by(|a, b| b.date_upd.cmp(&a.date_upd));
    Ok(candidates)
}

/// GET /orders/{id}?display=full and parse with the canonical
/// `Order` struct so we inherit every extract_* helper.
async fn fetch_full_order(client: &Client, id_order: &str) -> Result<FullOrder> {
    #[derive(Debug, Deserialize)]
    struct FullOrderResponse {
        order: FullOrder,
    }
    let url = format!(
        "{PRESTASHOP_API_URL_WASM}/orders/{id_order}?output_format=JSON&display=full"
    );
    let resp: FullOrderResponse = client
        .get(&url)
        .send()
        .await?
        .error_for_status()
        .context("PrestaShop full order fetch failed")?
        .json()
        .await
        .context("Failed to parse PrestaShop full order")?;
    Ok(resp.order)
}

async fn build_candidate(order: FullOrder, state: OrderState) -> OpenServiceCandidate {
    // Service-order header in the associations carries the per-machine
    // check-in notes that the shop technicians actually want to see.
    let checkin_notes = order
        .associations
        .order_service
        .first()
        .map(|s| s.check_in_notes.clone())
        .unwrap_or_default();
    // Pull cpu/gpu/ram/serial/mfg via the existing async helper.
    let extracted = order.extract_specs().await;

    OpenServiceCandidate {
        service_number: order.id.clone(),
        doc_alias: order.order_type.clone(),
        date_add: order.date_add.clone(),
        date_upd: order.date_upd.clone(),
        checkin_notes,
        state_name: state.as_str().to_string(),
        state_id: order.current_state.clone(),
        specs: PrestaSpecsSnapshot {
            cpu: extracted.cpu,
            gpu: extracted.gpu,
            ram: extracted.ram,
            device_serial: extracted.device_serial,
            device_mfg: extracted.device_mfg,
            device_model: order.extract_model(),
            motherboard_name: order.extract_motherboard().unwrap_or_default(),
            operating_system: order.extract_os().unwrap_or_default(),
            drives: order.extract_drives(),
        },
    }
}

// ============================================
// PrestaShop Implementation
// ============================================

async fn request_prestashop(serial13: &str) -> Result<PrestashopCustomerMatch> {
    let client = Client::new();

    // 1) /order_serial - lookup by serial number
    let url1 = format!(
        "{PRESTASHOP_API_URL_WASM}/order_serial?output_format=JSON&display=full&filter[serial_number]=[{serial13}]"
    );

    log::info!("PrestaShop order_serial request URL: {}", url1);

    let resp1 = client
        .get(&url1)
        .send()
        .await?
        .error_for_status()
        .inspect(|r| log::info!("PrestaShop order_serial response: {:?}", r))
        .inspect_err(|e| log::error!("PrestaShop order_serial request failed: {:?}", e))
        .context("PrestaShop order_serial request failed")?
        .json::<OrderSerialsResponse>()
        .await
        .inspect_err(|e| log::error!("Failed to parse order_serial response: {:?}", e))
        .context("Failed to parse order_serial response")?;

    let id_order = resp1
        .order_serials
        .get(0)
        .ok_or_else(|| anyhow!("No order found for serial"))?
        .id_order
        .clone();

    // 2) /orders/{id} - get customer ID
    let url2 = format!("{PRESTASHOP_API_URL_WASM}/orders/{id_order}?output_format=JSON");

    let resp2 = client
        .get(&url2)
        .send()
        .await?
        .error_for_status()
        .inspect(|r| log::info!("PrestaShop orders response: {:?}", r))
        .inspect_err(|e| log::error!("PrestaShop orders request failed: {:?}", e))
        .context("PrestaShop orders request failed")?
        .json::<OrderResponse>()
        .await
        .inspect_err(|e| log::error!("Failed to parse order response: {:?}", e))
        .context("Failed to parse order response")?;

    let id_customer = resp2.order.id_customer.trim().to_string();
    if id_customer.is_empty() {
        return Err(anyhow!("Order {} had no id_customer", id_order));
    }

    // 3) /customers/{id_customer} - get customer name
    let url3 = format!(
        "{PRESTASHOP_API_URL_WASM}/customers/{id_customer}?output_format=JSON"
    );

    let resp3 = client
        .get(&url3)
        .send()
        .await?
        .error_for_status()
        .context("PrestaShop customers request failed")?
        .json::<CustomerResponse>()
        .await
        .context("Failed to parse customer response")?;

    let first = resp3.customer.firstname.trim().to_string();
    let last = resp3.customer.lastname.trim().to_string();

    Ok(PrestashopCustomerMatch {
        friendly_name: format!("{first} {last} - {id_order}"),
        id_customer,
        id_order,
        first_name: first,
        last_name: last,
    })
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
