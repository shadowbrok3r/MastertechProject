use super::store_inventory_viewer::ExtraInventoryData;
use super::row_viewer::{RawStockData, SerialData, SerialInfo, CostBreakdownData, SystemInStoreData, SystemType, BulkOrderData, BulkProductData};
use database::{db, ODOO_API_KEY, ODOO_DB, ODOO_JSONRPC_URL, ODOO_UID, schema::{Store, ComputerData, prestashop::{Customer, Order, OrderDetail, OrderState, OrderType, Prestashop, PrestashopId}}};
use crossbeam::channel::Sender;
use anyhow::{Error, Result};
use serde::Deserialize;
use serde_json::json;
use reqwest::Client;
use log::info;

/* ------------------------------------ Odoo stock pulls + shared cache ------------------------------------ */

/// Max age of a cached Odoo pull before one client re-fetches it for everyone.
const STOCK_CACHE_TTL: &str = "1d";
/// Max age of a refresh claim before another client may take over a stuck refresh.
const STOCK_CLAIM_TTL: &str = "3m";

#[derive(Debug, Deserialize)]
struct OdooEnvelope<T> {
    #[serde(default = "Option::default")]
    result: Option<Vec<T>>,
    #[serde(default = "Option::default")]
    error: Option<serde_json::Value>,
}

/// Client with a request timeout so a hung Odoo pull cannot hold a cache claim.
fn odoo_client() -> Client {
    #[cfg(not(target_arch = "wasm32"))]
    {
        Client::builder()
            .timeout(std::time::Duration::from_secs(90))
            .build()
            .unwrap_or_else(|_| Client::new())
    }
    #[cfg(target_arch = "wasm32")]
    {
        Client::new()
    }
}

/// Odoo `execute_kw` search_read over JSON-RPC. `domain` is the full
/// positional domain argument, e.g. `[[["name", "in", [...]]]]`.
async fn odoo_search_read<T: serde::de::DeserializeOwned>(
    model: &str,
    domain: serde_json::Value,
    fields: serde_json::Value,
    limit: Option<u32>,
) -> Result<Vec<T>, Error> {
    let uid: u32 = ODOO_UID.parse()?;
    let mut kwargs = json!({ "fields": fields });
    if let Some(l) = limit {
        kwargs["limit"] = json!(l);
    }
    let body = json!({
        "jsonrpc": "2.0",
        "method": "call",
        "id": 1,
        "params": {
            "service": "object",
            "method": "execute_kw",
            "args": [ODOO_DB, uid, ODOO_API_KEY, model, "search_read", domain, kwargs]
        }
    });
    let env: OdooEnvelope<T> = odoo_client()
        .post(ODOO_JSONRPC_URL)
        .json(&body)
        .send()
        .await?
        .json()
        .await?;
    if let Some(err) = env.error {
        return Err(anyhow::anyhow!("Odoo {model} search_read error: {err}"));
    }
    Ok(env.result.unwrap_or_default())
}

/// Mirrors the retired fn::store_stock.
async fn fetch_store_stock_from_odoo(location: u64) -> Result<Vec<RawStockData>, Error> {
    odoo_search_read(
        "stock.quant",
        json!([[
            ["location_id", "=", location],
            ["location_id.usage", "in", ["internal", "transit"]],
            ["product_id", "!=", false],
            ["lot_id", "!=", false]
        ]]),
        json!(["location_id", "product_id", "quantity", "available_quantity", "inventory_quantity", "inventory_diff_quantity", "reserved_quantity", "lot_id"]),
        Some(5000),
    )
    .await
}

/// Mirrors the retired fn::find_attached_serials.
async fn fetch_attached_serials_from_odoo(serials: &[String]) -> Result<Vec<SerialInfo>, Error> {
    odoo_search_read(
        "stock.lot",
        json!([[["name", "in", serials]]]),
        json!(["id", "name", "product_id", "bs_sale_line_id", "bs_prest_ref"]),
        None,
    )
    .await
}

/// Mirrors the retired fn::get_stock_extra_info.
async fn fetch_extra_stock_info_from_odoo() -> Result<Vec<ExtraInventoryData>, Error> {
    odoo_search_read(
        "product.template",
        json!([[["qty_available", ">", 3]]]),
        json!(["name", "product_variant_id", "qty_available", "display_name", "virtual_available", "list_price", "standard_price"]),
        Some(5000),
    )
    .await
}

/// Cached `data` from `stock_cache:<key>` when fresher than the TTL.
/// Freshness is a scalar RETURN; the array is taken as `Vec<T>` (the SDK
/// rejects an array RETURN taken into `Option<Vec<T>>`).
async fn read_stock_cache<T: database::SurrealValue>(key: &str) -> Result<Option<Vec<T>>, Error> {
    let mut res = db()
        .query(format!(
            "LET $r = SELECT * FROM ONLY stock_cache:{key}; \
             RETURN $r.refreshed_at != NONE AND $r.refreshed_at > (time::now() - {STOCK_CACHE_TTL}); \
             RETURN $r.data ?? [];"
        ))
        .await?
        .check()?;
    let fresh: Option<bool> = res.take(1)?;
    if fresh != Some(true) {
        return Ok(None);
    }
    let data: Vec<T> = res.take(2)?;
    Ok(Some(data))
}

/// Claims the refresh slot for `stock_cache:<key>`.
/// Returns (claim token if this client won the slot, current cached data).
async fn claim_stock_cache<T: database::SurrealValue>(
    key: &str,
    kind: &str,
    location: Option<u64>,
) -> Result<(Option<String>, Option<Vec<T>>), Error> {
    let location_sql = location.map_or("NONE".to_string(), |l| l.to_string());
    // Claim UPSERT sets the required `kind` field; check() surfaces errors take() never reads.
    let mut res = db()
        .query(format!(
            "LET $r = SELECT * FROM ONLY stock_cache:{key}; \
             LET $tok = IF $r.refreshing_since == NONE OR $r.refreshing_since < (time::now() - {STOCK_CLAIM_TTL}) {{ (UPSERT stock_cache:{key} SET kind = '{kind}', location_id = {location_sql}, refreshing_since = time::now(), claim_token = rand::ulid() RETURN AFTER)[0].claim_token }} ELSE {{ NONE }}; \
             RETURN $tok; \
             RETURN $r.data ?? [];"
        ))
        .await?
        .check()?;
    let token: Option<String> = res.take(2)?;
    let stale_rows: Vec<T> = res.take(3)?;
    let stale = (!stale_rows.is_empty()).then_some(stale_rows);
    Ok((token, stale))
}

/// Stores a fresh pull in `stock_cache:<key>` if this client still holds the
/// claim; a claim removed by DELETE or taken over by another client skips the
/// write. Returns whether the cache row was written.
async fn write_stock_cache<T: database::SurrealValue>(
    key: &str,
    kind: &str,
    location: Option<u64>,
    data: Vec<T>,
    token: &str,
) -> Result<bool, Error> {
    let location_sql = location.map_or("NONE".to_string(), |l| l.to_string());
    let mut res = db()
        .query(format!(
            "RETURN IF (SELECT VALUE claim_token FROM ONLY stock_cache:{key}) == $tok {{ UPSERT stock_cache:{key} CONTENT {{ kind: '{kind}', location_id: {location_sql}, data: $data, refreshed_at: time::now(), refreshing_since: NONE, claim_token: NONE }}; true }} ELSE {{ false }};"
        ))
        .bind(("data", data))
        .bind(("tok", token.to_string()))
        .await?;
    let wrote: Option<bool> = res.take(0)?;
    Ok(wrote == Some(true))
}

/// Releases this client's claim; a claim owned by another token is untouched.
async fn release_stock_cache_claim(key: &str, token: &str) {
    let _ = db()
        .query(format!(
            "UPDATE stock_cache:{key} SET refreshing_since = NONE, claim_token = NONE WHERE claim_token == $tok;"
        ))
        .bind(("tok", token.to_string()))
        .await;
}

/// Cache-through pull: serve fresh cache, else claim + fetch + write, else
/// serve the current copy while another client refreshes, else fetch without
/// writing. Returns (rows, whether this call wrote the cache).
async fn cached_pull<T, Fut>(
    key: &str,
    kind: &str,
    location: Option<u64>,
    force: bool,
    fetch: impl FnOnce() -> Fut,
) -> Result<(Vec<T>, bool), Error>
where
    T: database::SurrealValue + Clone,
    Fut: std::future::Future<Output = Result<Vec<T>, Error>>,
{
    if !force {
        if let Some(data) = read_stock_cache::<T>(key).await? {
            info!("{key}: {} rows from cache", data.len());
            return Ok((data, false));
        }
    }

    let (token, stale) = match claim_stock_cache::<T>(key, kind, location).await {
        Ok(pair) => pair,
        Err(e) => {
            // Claim contention (e.g. write-conflict) degrades to a personal fetch.
            log::warn!("{key}: cache claim failed, fetching without cache write: {e:?}");
            (None, None)
        }
    };

    let Some(token) = token else {
        if let Some(data) = stale {
            info!("{key}: refresh in progress elsewhere, serving current cache");
            return Ok((data, false));
        }
        let data = fetch().await?;
        info!("{key}: {} rows from Odoo (no cache write)", data.len());
        return Ok((data, false));
    };

    match fetch().await {
        Ok(data) => {
            let wrote = match write_stock_cache(key, kind, location, data.clone(), &token).await {
                Ok(w) => w,
                Err(e) => {
                    log::error!("{key}: cache write failed: {e:?}");
                    release_stock_cache_claim(key, &token).await;
                    false
                }
            };
            info!("{key}: {} rows from Odoo (cache written: {wrote})", data.len());
            Ok((data, wrote))
        }
        Err(e) => {
            release_stock_cache_claim(key, &token).await;
            if let Some(data) = stale {
                log::error!("{key}: Odoo fetch failed, serving current cache: {e:?}");
                return Ok((data, false));
            }
            Err(e)
        }
    }
}

pub async fn get_stock(stock_tx: Sender<Vec<RawStockData>>, location: u64, force: bool) -> Result<(), Error> {
    let key = format!("store_stock_{location}");
    let (data, wrote) = cached_pull(&key, "store_stock", Some(location), force, || {
        fetch_store_stock_from_odoo(location)
    })
    .await?;
    if wrote {
        // Stock changed, so the per-store serial cache is no longer valid.
        if let Err(e) = db()
            .query(format!("DELETE stock_cache:serials_{location};"))
            .await
            .and_then(|r| r.check())
        {
            log::error!("store_stock {location}: serial cache invalidation failed: {e:?}");
        }
    }
    stock_tx.try_send(data)?;
    Ok(())
}

/// `write_cache: false` is a personal live check: it bypasses the shared
/// per-store cache entirely (no read, no claim, no write), so audit subsets
/// or an out-of-date table can never overwrite the store's serial cache.
pub async fn find_attached_serials(
    serials: Vec<String>,
    location: u64,
    force: bool,
    write_cache: bool,
    stock_tx: Sender<SerialData>,
) -> Result<(), Error> {
    if !write_cache {
        let result = fetch_attached_serials_from_odoo(&serials).await?;
        info!("serials (uncached check): {} rows from Odoo", result.len());
        stock_tx.try_send(SerialData { result })?;
        return Ok(());
    }
    let key = format!("serials_{location}");
    let (result, _) = cached_pull(&key, "serials", Some(location), force, move || async move {
        fetch_attached_serials_from_odoo(&serials).await
    })
    .await?;
    stock_tx.try_send(SerialData { result })?;
    Ok(())
}

pub async fn get_extra_stock_info(stock_tx: Sender<Vec<ExtraInventoryData>>, force: bool) -> Result<(), Error> {
    let (data, _) = cached_pull("extra_info", "extra_info", None, force, fetch_extra_stock_info_from_odoo).await?;
    stock_tx.try_send(data)?;
    Ok(())
}

/* ------------------------------------ Cost Breakdown API ------------------------------------ */

/// Order row from Prestashop order associations
#[derive(Debug, Deserialize, Clone, Default)]
pub struct OrderRow {
    pub product_id: String,
    pub product_reference: String,
    pub product_quantity: String,
    pub unit_price_tax_excl: String,
}

/// Prestashop order associations
#[derive(Debug, Deserialize, Clone, Default)]
pub struct OrderAssociations {
    pub order_rows: Vec<OrderRow>,
}

/// Prestashop order response
#[derive(Debug, Deserialize, Clone, Default)]
pub struct PrestashopOrder {
    pub id: String,
    pub associations: OrderAssociations,
}

/// Wrapper for order response
#[derive(Debug, Deserialize, Default)]
pub struct OrderResponse {
    pub order: PrestashopOrder,
}

/// Odoo product cost response
#[derive(Debug, Deserialize, Clone, Default)]
pub struct OdooProductCost {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub default_code: String,
    pub standard_price: f64,
    #[serde(default)]
    pub qty_available: f64,
}

/// Odoo JSON-RPC response for cost lookup
#[derive(Debug, Deserialize)]
pub struct OdooCostResponse {
    pub jsonrpc: String,
    pub result: Vec<OdooProductCost>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct OrderConfigData {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub id_order: String,
}

/// Summary data for cost breakdown (customer name, total revenue, total cost, profit)
#[derive(Debug, Clone, Default)]
pub struct CostBreakdownSummary {
    pub customer_name: String,
    pub order_total: f64,
    pub total_cost: f64,
    pub profit: f64,
}

/// Fetch order details from Prestashop
pub async fn get_order_from_prestashop(order_id: &str) -> Result<PrestashopOrder, Error> {
    let presta = Prestashop::default();
    
    let order: Order = presta.request_subresources_by_id_wasm("orders", "order", order_id).await?;
    info!("Fetched order {} with {} rows", order.id, order.associations.order_rows.len());
    
    // Convert the database Order type to our local PrestashopOrder type
    let order_rows: Vec<OrderRow> = order.associations.order_rows
        .iter()
        .map(|row| OrderRow {
            product_id: row.product_id.clone(),
            product_reference: row.product_reference.clone(),
            product_quantity: row.product_quantity.clone(),
            unit_price_tax_excl: row.product_price.clone(),
        })
        .collect();
    
    Ok(PrestashopOrder {
        id: order.id,
        associations: OrderAssociations { order_rows },
    })
}

/// Result from Odoo product lookup containing both ID and cost
#[derive(Debug, Clone)]
pub struct OdooProductResult {
    pub id: i64,
    pub cost: f64,
}

/// Look up product cost from Odoo by product code/name
/// Returns the Odoo product ID and cost from the product with the highest qty_available
pub async fn get_product_cost_from_odoo(search_term: &str) -> Result<Option<OdooProductResult>, Error> {
    let client = Client::new();
    let url = ODOO_JSONRPC_URL;
    log::warn!("Searching for product: {search_term}");
    let request_body = json!({
        "jsonrpc": "2.0",
        "method": "call",
        "params": {
            "service": "object",
            "method": "execute_kw",
            "args": [
                "pcl_live",
                374,
                database::ODOO_API_KEY,
                "product.template",
                "search_read",
                [ 
                    [
                        "&",
                        ["type", "in", ["consu", "product"]],
                        "|",
                        "|",
                        "|",
                        ["default_code", "ilike", search_term],
                        ["product_variant_ids.default_code", "ilike", search_term],
                        ["name", "ilike", search_term],
                        ["barcode", "ilike", search_term]
                    ]
                ],
                {
                    "fields": ["id", "name", "default_code", "standard_price", "virtual_available", "list_price", "qty_available"],
                    "limit": 20
                }
            ]
        },
        "id": 1
    });
    
    let response = client
        .post(url)
        .json(&request_body)
        .send()
        .await?;
    
    let response_text = response.text().await?;
    log::warn!("Response text: {response_text}");
    if let Ok(parsed) = serde_json::from_str::<OdooCostResponse>(&response_text) {
        if !parsed.result.is_empty() {
            // Sort by qty_available descending and take the product with highest quantity
            let mut products = parsed.result;
            products.sort_by(|a, b| b.qty_available.partial_cmp(&a.qty_available).unwrap_or(std::cmp::Ordering::Equal));
            
            if let Some(product) = products.first() {
                info!("Found product '{}' (id: {}, qty: {}) with cost ${:.2}", 
                      product.default_code, product.id, product.qty_available, product.standard_price);
                return Ok(Some(OdooProductResult {
                    id: product.id,
                    cost: product.standard_price,
                }));
            }
        }
    }
    
    Ok(None)
}

/// Fetch cost breakdown for an order
pub async fn get_cost_breakdown(
    order_id: String,
    cost_tx: Sender<Vec<CostBreakdownData>>,
    summary_tx: Sender<CostBreakdownSummary>,
) -> Result<(), Error> {
    info!("Fetching cost breakdown for order #{}", order_id);
    
    let presta = Prestashop::default();
    
    // Get order from Prestashop (full order for customer ID and total)
    let full_order: Order = presta.request_subresources_by_id_wasm("orders", "order", &order_id).await?;
    info!("Got {} order rows", full_order.associations.order_rows.len());
    
    // Get customer name
    let customer_name = if !full_order.id_customer.is_empty() && full_order.id_customer != "0" {
        match presta.request_subresources_by_id_wasm::<Customer>("customers", "customer", &full_order.id_customer).await {
            Ok(customer) => format!("{} {}", customer.firstname, customer.lastname),
            Err(e) => {
                info!("Failed to fetch customer: {:?}", e);
                "Unknown".to_string()
            }
        }
    } else {
        "Unknown".to_string()
    };
    
    // Get order total (total_products_wt = total products with tax)
    let order_total: f64 = full_order.total_products.parse().unwrap_or(0.0) - full_order.total_discounts_tax_excl.parse().unwrap_or(0.0);
    
    let mut cost_data = Vec::new();
    let mut total_cost: f64 = 0.0;
    
    for row in full_order.associations.order_rows.iter() {
        let quantity: f64 = row.product_quantity.parse().unwrap_or(0.0);
        let unit_price: f64 = row.product_price.parse().unwrap_or(0.0);
        
        // Look up cost from Odoo using product reference - returns both Odoo ID and cost
        let odoo_result = if !row.product_reference.is_empty() {
            get_product_cost_from_odoo(&row.product_reference).await.unwrap_or(None)
        } else {
            None
        };
        
        let (odoo_id, item_cost) = match odoo_result {
            Some(result) => (result.id.to_string(), result.cost),
            None => (String::new(), 0.0),
        };
        
        // Total cost = cost per unit * quantity
        total_cost += item_cost * quantity;
        
        cost_data.push(CostBreakdownData(
            odoo_id,                      // Odoo Product ID
            row.product_id.clone(),       // Prestashop Product ID
            row.product_reference.clone(), // Product Name/Reference
            quantity,
            unit_price,
            item_cost,
        ));
    }
    
    let profit = order_total - total_cost;
    
    info!("Cost breakdown complete: {} items, total: ${:.2}, cost: ${:.2}, profit: ${:.2}", 
          cost_data.len(), order_total, total_cost, profit);
    
    // Send summary
    let _ = summary_tx.try_send(CostBreakdownSummary {
        customer_name,
        order_total,
        total_cost,
        profit,
    });
    
    cost_tx.try_send(cost_data)?;

    Ok(())
}

/* ------------------------------------ Bulk Cost Breakdown API ------------------------------------ */

/// Date window pulled per PrestaShop query (days).
const BULK_WINDOW_DAYS: i64 = 7;
/// Page size for the paginated order-ID pull within a window.
const BULK_PAGE: i32 = 100;
/// Hard cap on orders processed in one bulk run.
const MAX_BULK_ORDERS: usize = 1500;

/// Aggregate result for a bulk cost breakdown run.
#[derive(Debug, Clone, Default)]
pub struct BulkCostSummary {
    pub order_count: usize,
    pub item_count: f64,
    pub total_revenue: f64,
    pub total_cost: f64,
    pub total_profit: f64,
    pub avg_revenue: f64,
    pub avg_cost: f64,
    pub avg_profit: f64,
    pub margin_pct: f64,
    pub truncated: bool,
}

/// Order-state preset applied to the bulk pull.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum BulkStateFilter {
    /// Delivered / done states only (revenue realized).
    Completed,
    /// All applicable states for the type except Returned.
    ExcludeReturned,
    /// No `current_state` filter.
    Any,
}

impl BulkStateFilter {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Completed => "Completed / Delivered",
            Self::ExcludeReturned => "Exclude Returned",
            Self::Any => "Any State",
        }
    }
}

/// PrestaShop `current_state` ids to include for an order type + filter preset.
/// An empty vec means no state filter.
pub fn bulk_included_state_ids(order_type: &OrderType, filter: BulkStateFilter) -> Vec<String> {
    let states: Vec<OrderState> = match filter {
        BulkStateFilter::Any => Vec::new(),
        BulkStateFilter::Completed => match order_type {
            OrderType::SalesOrder => vec![OrderState::Shipped, OrderState::DeliveredToStore],
            OrderType::ServiceOrder => vec![OrderState::DoneShelf],
            _ => Vec::new(),
        },
        BulkStateFilter::ExcludeReturned => order_type
            .applicable_states()
            .into_iter()
            .filter(|s| !matches!(s, OrderState::Returned))
            .collect(),
    };
    states.iter().map(|s| s.to_id_str().to_string()).collect()
}

/// Bulk cost breakdown over a date range for a given order type.
/// Pulls order ids in `BULK_WINDOW_DAYS` windows (paginated), then computes
/// per-order revenue/cost/profit reusing a per-reference Odoo cost cache, and
/// aggregates into per-order rows, a per-product rollup, and a summary.
pub async fn get_bulk_cost_breakdown(
    from: String,
    to: String,
    order_type_id: String,
    store_id: Option<u64>,
    state_ids: Vec<String>,
    orders_tx: Sender<Vec<BulkOrderData>>,
    products_tx: Sender<Vec<BulkProductData>>,
    summary_tx: Sender<BulkCostSummary>,
    progress_tx: Sender<(usize, usize)>,
) -> Result<(), Error> {
    use std::collections::HashSet;
    use chrono::{Duration, NaiveDate};

    let start = NaiveDate::parse_from_str(&from, "%Y-%m-%d")?;
    let end = NaiveDate::parse_from_str(&to, "%Y-%m-%d")?;
    let (start, end) = if start <= end { (start, end) } else { (end, start) };

    let states_filter = (!state_ids.is_empty()).then(|| format!("[{}]", state_ids.join("|")));
    let store_str = store_id.map(|s| s.to_string());

    let mut id_api = Prestashop::default();
    id_api.display = "[id]";

    let mut order_ids: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut capped = false;

    let mut win_start = start;
    'windows: while win_start <= end {
        let win_end = (win_start + Duration::days(BULK_WINDOW_DAYS - 1)).min(end);
        let date_range = format!(
            "[{} 00:00:00,{} 23:59:59]",
            win_start.format("%Y-%m-%d"),
            win_end.format("%Y-%m-%d")
        );

        let mut page_start = 0i32;
        loop {
            let pagination = format!("{},{}", page_start, BULK_PAGE);
            let mut query: HashMap<&str, &str> = HashMap::new();
            query.insert("filter[id_order_type]", order_type_id.as_str());
            query.insert("filter[date_add]", date_range.as_str());
            query.insert("date", "1");
            if let Some(ref s) = store_str {
                query.insert("filter[id_store]", s.as_str());
            }
            if let Some(ref st) = states_filter {
                query.insert("filter[current_state]", st.as_str());
            }
            query.insert("sort", "[id_DESC]");
            query.insert("limit", pagination.as_str());
            query.insert("output_format", "JSON");

            let page: Vec<PrestashopId> = id_api
                .request_resources_checked("orders", query)
                .await
                .unwrap_or_default();
            let real: Vec<String> = page
                .into_iter()
                .map(|p| p.id)
                .filter(|id| !id.is_empty() && id != "0")
                .collect();
            let n = real.len();

            for id in real {
                if seen.insert(id.clone()) {
                    order_ids.push(id);
                    if order_ids.len() >= MAX_BULK_ORDERS {
                        capped = true;
                        break 'windows;
                    }
                }
            }

            if n < BULK_PAGE as usize {
                break;
            }
            page_start += BULK_PAGE;
        }

        win_start = win_end + Duration::days(1);
    }

    let total = order_ids.len();
    info!("bulk cost: collected {total} order ids ({from}..{to}) capped={capped}");
    let _ = progress_tx.try_send((0, total));

    let order_api = Prestashop::default();
    // reference (upper) -> unit cost from Odoo
    let mut cost_cache: HashMap<String, f64> = HashMap::new();
    // reference -> (name, qty, total_cost, total_revenue)
    let mut rollup: HashMap<String, (String, f64, f64, f64)> = HashMap::new();

    let mut order_rows: Vec<BulkOrderData> = Vec::new();
    let mut grand_revenue = 0.0;
    let mut grand_cost = 0.0;
    let mut grand_items = 0.0;

    for (i, oid) in order_ids.iter().enumerate() {
        let full: Order = match order_api
            .request_subresources_by_id_wasm("orders", "order", oid)
            .await
        {
            Ok(o) => o,
            Err(e) => {
                log::error!("bulk cost: order {oid} fetch failed: {e:?}");
                let _ = progress_tx.try_send((i + 1, total));
                continue;
            }
        };
        if full.id.is_empty() {
            let _ = progress_tx.try_send((i + 1, total));
            continue;
        }

        // Defensive date guard in case the server-side date filter is loose.
        let date_short = full.date_add.split_whitespace().next().unwrap_or("").to_string();
        if let Ok(od) = NaiveDate::parse_from_str(&date_short, "%Y-%m-%d") {
            if od < start || od > end {
                let _ = progress_tx.try_send((i + 1, total));
                continue;
            }
        }

        let revenue = full.total_products.parse::<f64>().unwrap_or(0.0)
            - full.total_discounts_tax_excl.parse::<f64>().unwrap_or(0.0);
        let mut order_cost = 0.0;
        let mut order_items = 0.0;

        for row in full.associations.order_rows.iter() {
            let qty = row.product_quantity.parse::<f64>().unwrap_or(0.0);
            let unit_price = row.product_price.parse::<f64>().unwrap_or(0.0);
            order_items += qty;

            let cost = if row.product_reference.is_empty() {
                0.0
            } else {
                let key = row.product_reference.to_uppercase();
                if let Some(c) = cost_cache.get(&key) {
                    *c
                } else {
                    let c = get_product_cost_from_odoo(&row.product_reference)
                        .await
                        .ok()
                        .flatten()
                        .map(|r| r.cost)
                        .unwrap_or(0.0);
                    cost_cache.insert(key, c);
                    c
                }
            };

            order_cost += cost * qty;

            let entry = rollup
                .entry(row.product_reference.clone())
                .or_insert_with(|| (row.product_name.clone(), 0.0, 0.0, 0.0));
            if entry.0.is_empty() {
                entry.0 = row.product_name.clone();
            }
            entry.1 += qty;
            entry.2 += cost * qty;
            entry.3 += unit_price * qty;
        }

        let profit = revenue - order_cost;
        let margin = if revenue != 0.0 { profit / revenue * 100.0 } else { 0.0 };

        order_rows.push(BulkOrderData(
            full.id.clone(),
            date_short,
            order_items,
            revenue,
            order_cost,
            profit,
            margin,
        ));
        grand_revenue += revenue;
        grand_cost += order_cost;
        grand_items += order_items;

        let _ = progress_tx.try_send((i + 1, total));
    }

    let mut product_rows: Vec<BulkProductData> = rollup
        .into_iter()
        .map(|(reference, (name, qty, tcost, trev))| {
            let profit = trev - tcost;
            let margin = if trev != 0.0 { profit / trev * 100.0 } else { 0.0 };
            BulkProductData(reference, name, qty, tcost, trev, profit, margin)
        })
        .collect();
    product_rows.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap_or(std::cmp::Ordering::Equal));

    let order_count = order_rows.len();
    let total_profit = grand_revenue - grand_cost;
    let margin_pct = if grand_revenue != 0.0 { total_profit / grand_revenue * 100.0 } else { 0.0 };
    let (avg_revenue, avg_cost, avg_profit) = if order_count > 0 {
        let n = order_count as f64;
        (grand_revenue / n, grand_cost / n, total_profit / n)
    } else {
        (0.0, 0.0, 0.0)
    };

    let summary = BulkCostSummary {
        order_count,
        item_count: grand_items,
        total_revenue: grand_revenue,
        total_cost: grand_cost,
        total_profit,
        avg_revenue,
        avg_cost,
        avg_profit,
        margin_pct,
        truncated: capped,
    };
    info!(
        "bulk cost: {order_count} orders, revenue ${:.2}, cost ${:.2}, profit ${:.2}",
        grand_revenue, grand_cost, total_profit
    );

    let _ = products_tx.try_send(product_rows);
    let _ = summary_tx.try_send(summary);
    orders_tx.try_send(order_rows)?;

    Ok(())
}

/* ------------------------------------ Systems In-Store API ------------------------------------ */

use std::collections::HashMap;

/// Customer IDs for in-store systems (R2R inventory accounts)
pub const INSTORE_CUSTOMER_IDS: [&str; 2] = ["148642", "128011"];
pub const LTN_CUSTOMER_ID: &str = "20378";
pub const MUR_CUSTOMER_IDS: [&str; 2] = ["162605", "147292"];
pub const ORE_CUSTOMER_IDS: [&str; 2] = ["138100", "136515"];
pub const SAN_CUSTOMER_IDS: [&str; 2] = ["59882", "136919"];

pub fn get_customer_ids_for_store(store_id: u64) -> Vec<&'static str> {
    match Store::from_presta_store_id(&store_id.to_string()) {
        Store::RIV => INSTORE_CUSTOMER_IDS.to_vec(),
        Store::LTN => vec![LTN_CUSTOMER_ID],
        Store::MUR => MUR_CUSTOMER_IDS.to_vec(),
        Store::ORE => ORE_CUSTOMER_IDS.to_vec(),
        Store::SAN => SAN_CUSTOMER_IDS.to_vec(),
    }
}

/// One message in the incremental Systems In-Store stream.
pub enum SystemStreamMsg {
    /// A row to insert or update, keyed by `order_id`.
    Row(Box<SystemInStoreData>),
    /// An order that left the in-store set; remove it by `order_id`.
    Remove(String),
    /// The stream finished; the consumer persists the table to storage.
    Done,
}

/// Order types shown in the Systems In-Store view.
fn systems_order_types() -> [OrderType; 4] {
    [
        OrderType::SalesOrder,
        OrderType::ReadyToRoll,
        OrderType::Bsd,
        OrderType::Rci,
    ]
}

/// Full streamed pull: fetch every in-store order for a store and emit each
/// processed row as it completes. Used for a cold cache or a forced reload.
pub async fn get_systems_in_store(
    store_id: u64,
    ctx: eframe::egui::Context,
    tx: Sender<SystemStreamMsg>,
) -> Result<(), Error> {
    info!("Streaming systems in-store for store {}", store_id);

    let presta = Prestashop::default();

    for customer_id in get_customer_ids_for_store(store_id) {
        for order_type in systems_order_types().iter() {
            let store_id_str = store_id.to_string();
            let order_type_id = order_type.to_id().to_string();
            let order_state_id = OrderState::DeliveredToStore.to_id().to_string();

            let mut query = HashMap::new();
            query.insert("output_format", "JSON");
            query.insert("filter[id_customer]", customer_id);
            query.insert("filter[id_store]", &store_id_str);
            query.insert("filter[id_order_type]", &order_type_id);
            query.insert("filter[current_state]", &order_state_id);
            let query_refs: HashMap<&str, &str> = query.iter().map(|(k, v)| (*k, *v)).collect();

            match presta.request_resources_checked::<Order>("orders", query_refs).await {
                Ok(orders) => {
                    info!("Got {} orders for customer {} type {:?}", orders.len(), customer_id, order_type.to_id());
                    for order in orders.iter() {
                        let data = process_order_to_system_data(order).await;
                        let _ = tx.try_send(SystemStreamMsg::Row(Box::new(data)));
                        ctx.request_repaint();
                    }
                }
                Err(e) => {
                    log::error!("Failed to fetch orders for customer {} type {:?}: {:?}", customer_id, order_type.to_id(), e);
                }
            }
        }
    }

    let _ = tx.try_send(SystemStreamMsg::Done);
    ctx.request_repaint();
    Ok(())
}

/// Reconcile a cached Systems In-Store list against the live order set.
/// Runs a minimal `[id,date_upd]` order-list query per customer/type, reuses
/// cached rows whose `date_upd` is unchanged, re-fetches new or changed
/// orders, and removes orders no longer in the in-store set. Each decision is
/// streamed so the table patches incrementally.
pub async fn reconcile_systems_in_store(
    store_id: u64,
    cached: Vec<SystemInStoreData>,
    ctx: eframe::egui::Context,
    tx: Sender<SystemStreamMsg>,
) -> Result<(), Error> {
    info!("Reconciling {} cached systems for store {}", cached.len(), store_id);

    let cached_by_id: HashMap<String, SystemInStoreData> =
        cached.into_iter().map(|s| (s.order_id.clone(), s)).collect();

    // Minimal staleness query pulls only id + date_upd per order.
    let mut list_api = Prestashop::default();
    list_api.display = "[id,date_upd]";
    // Full-display client for fetching new/changed orders.
    let detail_api = Prestashop::default();

    let mut live: HashMap<String, String> = HashMap::new();
    for customer_id in get_customer_ids_for_store(store_id) {
        for order_type in systems_order_types().iter() {
            let store_id_str = store_id.to_string();
            let order_type_id = order_type.to_id().to_string();
            let order_state_id = OrderState::DeliveredToStore.to_id().to_string();

            let mut query = HashMap::new();
            query.insert("output_format", "JSON");
            query.insert("filter[id_customer]", customer_id);
            query.insert("filter[id_store]", &store_id_str);
            query.insert("filter[id_order_type]", &order_type_id);
            query.insert("filter[current_state]", &order_state_id);
            let query_refs: HashMap<&str, &str> = query.iter().map(|(k, v)| (*k, *v)).collect();

            match list_api.request_resources_checked::<Order>("orders", query_refs).await {
                Ok(orders) => {
                    for o in orders.iter() {
                        if !o.id.is_empty() && o.id != "0" {
                            live.insert(o.id.clone(), o.date_upd.clone());
                        }
                    }
                }
                Err(e) => {
                    log::error!("staleness query failed (customer {} type {:?}): {e:?}", customer_id, order_type.to_id());
                }
            }
        }
    }

    // Drop cached orders no longer in the live in-store set.
    for id in cached_by_id.keys() {
        if !live.contains_key(id) {
            let _ = tx.try_send(SystemStreamMsg::Remove(id.clone()));
            ctx.request_repaint();
        }
    }

    // Reuse unchanged rows; fetch and process new or changed orders.
    for (id, date_upd) in live.iter() {
        let reuse = cached_by_id
            .get(id)
            .filter(|c| !date_upd.is_empty() && &c.date_upd == date_upd);
        if let Some(cached_row) = reuse {
            let _ = tx.try_send(SystemStreamMsg::Row(Box::new(cached_row.clone())));
            ctx.request_repaint();
            continue;
        }
        match detail_api.request_subresources_by_id_wasm::<Order>("orders", "order", id).await {
            Ok(order) => {
                let data = process_order_to_system_data(&order).await;
                let _ = tx.try_send(SystemStreamMsg::Row(Box::new(data)));
                ctx.request_repaint();
            }
            Err(e) => log::error!("systems reconcile: order {id} fetch failed: {e:?}"),
        }
    }

    let _ = tx.try_send(SystemStreamMsg::Done);
    ctx.request_repaint();
    Ok(())
}

/// Add a single order to the systems table
pub async fn add_order_to_systems(
    order_id: String,
    systems_tx: Sender<SystemInStoreData>,
) -> Result<(), Error> {
    info!("Adding order {} to systems in-store", order_id);
    
    let presta = Prestashop::default();
    let order: Order = presta.request_subresources_by_id_wasm("orders", "order", &order_id).await?;
    
    let system_data = process_order_to_system_data(&order).await;
    systems_tx.try_send(system_data)?;
    
    Ok(())
}

/// Process a single order into SystemInStoreData
/// This function is public so it can be reused by other modules (e.g., receive_prestashop)
pub async fn process_order_to_system_data(order: &Order) -> SystemInStoreData {
    let price: f64 = order.total_paid_tax_excl.parse().unwrap_or(0.0);
    
    // Determine system type from order type
    let system_type = SystemType::from_order_type_id(&order.id_order_type);
    
    // Extract model name from order rows (main product, usually laptop/desktop)
    let model = extract_model_from_order(order).await;
    
    // Extract specs (CPU, GPU, RAM) from order rows
    let (cpu, gpu, ram) = extract_specs_from_order(order).await;
    
    // Extract warranty info
    let warranty = extract_warranty_from_order(order);
    
    // Calculate spiffs
    let spiff = calculate_spiffs_for_order(order);
    
    // Get cost from Odoo (for the main product)
    let cost = get_system_cost_from_order(order).await;
    
    let revenue = price - cost;
    
    // Fetch customer info
    let (customer_id, customer_name) = get_customer_info(&order.id_customer).await;
    
    // Build ComputerData from the extracted specs
    let computer_data = ComputerData {
        cpu: cpu.clone(),
        gpu: gpu.clone(),
        ram: ram.clone(),
        device_model: Some(model.clone()),
        ..Default::default()
    };
    
    SystemInStoreData {
        order_id: order.id.clone(),
        customer_id,
        customer_name,
        model,
        price,
        cost,
        revenue,
        spiff,
        system_type,
        cpu,
        gpu,
        ram,
        warranty,
        store_id: order.id_store.clone(),
        date_upd: order.date_upd.clone(),
        computer_data,
    }
}

/// Fetch customer info (id and name) from Prestashop
async fn get_customer_info(customer_id: &str) -> (String, String) {
    if customer_id.is_empty() || customer_id == "0" {
        return (String::new(), "Unknown".to_string());
    }
    
    let presta = Prestashop::default();
    match presta.request_subresources_by_id_wasm::<Customer>(
        "customers", 
        "customer", 
        customer_id
    ).await {
        Ok(customer) => {
            let name = format!("{} {}", customer.firstname, customer.lastname);
            (customer_id.to_string(), name)
        }
        Err(e) => {
            info!("Failed to fetch customer {}: {:?}", customer_id, e);
            (customer_id.to_string(), "Unknown".to_string())
        }
    }
}

/// Extract the main model name from order rows
/// For RCI orders: Returns the product_name directly
/// For non-RCI orders: Fetches order_config to get the system name from the 'name' field
async fn extract_model_from_order(order: &Order) -> String {
    let is_rci = order.id_order_type == OrderType::Rci.to_id().to_string();
    
    // Find the main system product row
    let main_row = order.associations.order_rows.iter().find(|row| {
        let r = row.product_reference.to_lowercase();
        r.starts_with("lap/") 
            || (r.starts_with("case/") && !r.starts_with("case/15") && !r.starts_with("case/17"))
            || r.starts_with("bsd/")
            || r.starts_with("rci/")
            || r.starts_with("r2r/")
            || r.starts_with("rtr/")
    });
    
    if let Some(row) = main_row {
        if is_rci {
            // For RCI orders, use the product_name directly
            return row.product_name.clone();
        }
        
        // For non-RCI orders, try to get order_config for the proper system name
        if !row.id_order_config.is_empty() && row.id_order_config != "0" {
            let presta = Prestashop::default();
            match presta.request_subresources_by_id_wasm::<OrderConfigData>(
                "order_config",
                "order_config",
                &row.id_order_config
            ).await {
                Ok(config) => {
                    if !config.name.is_empty() {
                        info!("Got order_config name: {} for order {}", config.name, order.id);
                        return config.name;
                    }
                }
                Err(e) => {
                    info!("Failed to fetch order_config {}: {:?}, falling back to product_name", row.id_order_config, e);
                }
            }
        }
        
        // Fallback to product_name
        return row.product_name.clone();
    }
    
    // Fallback to first product
    order.associations.order_rows.first()
        .map(|r| r.product_name.clone())
        .unwrap_or_default()
}

/// Extract CPU, GPU, RAM specs from order rows
async fn extract_specs_from_order(order: &Order) -> (String, String, String) {
    let mut cpu = String::new();
    let mut gpu = String::new();
    let mut ram = String::new();
    
    // Step 1: Check for serialized products with explicit part references
    for row in order.associations.order_rows.iter() {
        let r = row.product_reference.to_lowercase();
        let name = &row.product_name;
        
        // CPU detection from serialized parts
        if r.starts_with("cpu/") {
            cpu = name.clone();
        }
        
        // GPU detection from serialized parts
        if r.starts_with("gpu/") || r.starts_with("vid/") {
            gpu = name.clone();
        }
        
        // RAM detection from serialized parts
        if r.starts_with("ddr5/") || r.starts_with("ddr4/") || r.starts_with("ram/") || r.starts_with("mem/") {
            ram = name.clone();
        }
    }
    
    // Step 2: For RCI systems, fetch OrderDetail and parse specs from detail_notes
    let is_rci = order.id_order_type == OrderType::Rci.to_id().to_string();
    if is_rci && (cpu.is_empty() || gpu.is_empty() || ram.is_empty()) {
        info!("RCI order {} - looking for U/DESKTOP, U/LAPTOPS, or RCI/ in {} order_serials", 
              order.id, order.associations.order_serial.len());
        
        // Find U/DESKTOP, U/LAPTOPS, or RCI-prefixed product in order_serial to get id_order_detail
        for serial in order.associations.order_serial.iter() {
            let ref_lower = serial.product_reference.to_lowercase();
            info!("  Checking serial: product_ref='{}', id_order_detail='{}'", 
                  serial.product_reference, serial.id_order_detail);
            
            // Match U/DESKTOP, U/LAPTOPS, or any RCI/ prefixed products
            let is_rci_product = ref_lower == "u/desktop" 
                || ref_lower == "u/laptops" 
                || ref_lower == "u/laptop"
                || ref_lower.starts_with("rci/");
            
            if is_rci_product && !serial.id_order_detail.is_empty() && serial.id_order_detail != "0" {
                info!("  Found matching serial, fetching OrderDetail id={}", serial.id_order_detail);
                
                // Fetch OrderDetail using id_order_detail
                let presta = Prestashop::default();
                match presta.request_subresources_by_id_wasm::<OrderDetail>(
                    "order_details", 
                    "order_detail", 
                    &serial.id_order_detail
                ).await {
                    Ok(detail) => {
                        info!("  OrderDetail fetched, detail_notes length={}", detail.detail_notes.len());
                        if !detail.detail_notes.is_empty() {
                            info!("  detail_notes: {:?}", &detail.detail_notes[..detail.detail_notes.len().min(200)]);
                        }
                        
                        // Parse detail_notes format: "Brand: DELL\r\nCPU: i7-10610U\r\nRAM: 16GB\r\n..."
                        let (parsed_cpu, parsed_gpu, parsed_ram) = parse_detail_notes(&detail.detail_notes);
                        info!("  Parsed specs: cpu='{}', gpu='{}', ram='{}'", parsed_cpu, parsed_gpu, parsed_ram);
                        
                        if cpu.is_empty() && !parsed_cpu.is_empty() {
                            cpu = parsed_cpu;
                        }
                        if gpu.is_empty() && !parsed_gpu.is_empty() {
                            gpu = parsed_gpu;
                        }
                        if ram.is_empty() && !parsed_ram.is_empty() {
                            ram = parsed_ram;
                        }
                        
                        // If we found any specs, break out
                        if !cpu.is_empty() || !gpu.is_empty() || !ram.is_empty() {
                            break;
                        }
                    }
                    Err(e) => {
                        info!("  Failed to fetch OrderDetail: {:?}", e);
                    }
                }
            }
        }
        
        // Fallback: If still missing specs, check ALL order_serial entries for detail_notes
        if cpu.is_empty() || gpu.is_empty() || ram.is_empty() {
            info!("RCI order {} - fallback: checking all order_serial entries for detail_notes", order.id);
            for serial in order.associations.order_serial.iter() {
                if serial.id_order_detail.is_empty() || serial.id_order_detail == "0" {
                    continue;
                }
                
                let presta = Prestashop::default();
                match presta.request_subresources_by_id_wasm::<OrderDetail>(
                    "order_details", 
                    "order_detail", 
                    &serial.id_order_detail
                ).await {
                    Ok(detail) => {
                        if !detail.detail_notes.is_empty() {
                            info!("  Found detail_notes in serial '{}': {:?}", 
                                  serial.product_reference, 
                                  &detail.detail_notes[..detail.detail_notes.len().min(200)]);
                            
                            let (parsed_cpu, parsed_gpu, parsed_ram) = parse_detail_notes(&detail.detail_notes);
                            
                            if cpu.is_empty() && !parsed_cpu.is_empty() {
                                cpu = parsed_cpu;
                            }
                            if gpu.is_empty() && !parsed_gpu.is_empty() {
                                gpu = parsed_gpu;
                            }
                            if ram.is_empty() && !parsed_ram.is_empty() {
                                ram = parsed_ram;
                            }
                            
                            if !cpu.is_empty() && !gpu.is_empty() && !ram.is_empty() {
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        info!("  Failed to fetch OrderDetail {}: {:?}", serial.id_order_detail, e);
                    }
                }
            }
        }
    }
    
    // Step 3: For non-RCI laptops, parse specs from LAP/ product name
    if cpu.is_empty() || gpu.is_empty() {
        for row in order.associations.order_rows.iter() {
            let r = row.product_reference.to_lowercase();
            if r.starts_with("lap/") {
                // Parse CPU and GPU from laptop product name
                // Examples: "SM-5 15" RTX 5060 Core Ultra 7 275HX", "SM3 14" RYZEN 7 255"
                let (parsed_cpu, parsed_gpu) = parse_laptop_product_name(&row.product_name);
                if cpu.is_empty() && !parsed_cpu.is_empty() {
                    cpu = parsed_cpu;
                }
                if gpu.is_empty() && !parsed_gpu.is_empty() {
                    gpu = parsed_gpu;
                }
                break;
            }
        }
    }
    
    (cpu, gpu, ram)
}

/// Parse detail_notes from RCI order_serial
/// Format: "Brand: DELL\r\nCPU: i7-10610U\r\nRAM: 16GB\r\nGPU: INTEGRATED\r\n..."
fn parse_detail_notes(notes: &str) -> (String, String, String) {
    let mut cpu = String::new();
    let mut gpu = String::new();
    let mut ram = String::new();
    
    for line in notes.split(|c| c == '\n' || c == '\r') {
        let line = line.trim();
        if line.is_empty() { continue; }
        
        if let Some(value) = line.strip_prefix("CPU:") {
            cpu = value.trim().to_string();
        } else if let Some(value) = line.strip_prefix("GPU:") {
            let val = value.trim();
            // Skip "INTEGRATED" as it's not useful
            if val.to_lowercase() != "integrated" {
                gpu = val.to_string();
            }
        } else if let Some(value) = line.strip_prefix("RAM:") {
            ram = value.trim().to_string();
        }
    }
    
    (cpu, gpu, ram)
}

/// Parse CPU and GPU from laptop product name
/// Examples:
/// - "SM-5 15" RTX 5060 Core Ultra 7 275HX" -> CPU: "Core Ultra 7 275HX", GPU: "RTX 5060"
/// - "SM3 14" RYZEN 7 255" -> CPU: "RYZEN 7 255", GPU: ""
/// - "SMT-8 17" RTX 5070 U9 275HX" -> CPU: "Ultra 9 275HX", GPU: "RTX 5070"
fn parse_laptop_product_name(name: &str) -> (String, String) {
    let cpu = extract_cpu_from_laptop_name(name);
    let gpu = extract_gpu_from_laptop_name(name);
    (cpu, gpu)
}

/// Extract CPU model from laptop product name
fn extract_cpu_from_laptop_name(name: &str) -> String {
    let name_upper = name.to_uppercase();
    
    // Check for "Core Ultra X" pattern (e.g., "Core Ultra 7 275HX")
    if let Some(idx) = name_upper.find("CORE ULTRA") {
        let after = &name[idx..];
        return extract_until_resolution(after, 25);
    }
    
    // Check for "Core X" pattern without Ultra (e.g., "Core 7 250H", "Core 5 120U")
    if let Some(idx) = name_upper.find("CORE ") {
        // Make sure it's not "Core Ultra" (already handled above)
        let after = &name_upper[idx..];
        if !after.starts_with("CORE ULTRA") {
            let after_orig = &name[idx..];
            return extract_until_resolution(after_orig, 20);
        }
    }
    
    // Check for "U5/U7/U9" shorthand pattern (e.g., "U9 275HX" -> "Ultra 9 275HX")
    for (short, full) in [("U9 ", "Ultra 9"), ("U7 ", "Ultra 7"), ("U5 ", "Ultra 5")] {
        if let Some(idx) = name_upper.find(short) {
            // Get the model number after UX
            let after_prefix = &name[idx + 3..];
            let model = extract_until_resolution(after_prefix, 10);
            if !model.is_empty() {
                return format!("{} {}", full, model);
            }
        }
    }
    
    // Check for RYZEN patterns (e.g., "RYZEN 7 7435HS", "RYZEN AI 7 350")
    if let Some(idx) = name_upper.find("RYZEN") {
        let after_ryzen = &name[idx..];
        return extract_until_resolution(after_ryzen, 25);
    }
    
    // Check for Intel i5/i7/i9 patterns
    for prefix in ["I9 ", "I9-", "I7 ", "I7-", "I5 ", "I5-"] {
        if let Some(idx) = name_upper.find(prefix) {
            let after_prefix = &name[idx..];
            return extract_until_resolution(after_prefix, 15);
        }
    }
    
    String::new()
}

/// Extract CPU/model string until we hit a resolution indicator or end
fn extract_until_resolution(s: &str, max_len: usize) -> String {
    let end_idx = s.len().min(max_len);
    let spec = &s[..end_idx];
    
    // Split by common resolution/end markers
    let end_markers = ["1080", "2K", "4K", "FHD", "QHD", "LAPTOP", "Ready", "SOLD"];
    
    let mut result = spec.to_string();
    for marker in end_markers {
        if let Some(pos) = result.to_uppercase().find(marker) {
            result = result[..pos].to_string();
        }
    }
    
    result.trim().to_string()
}

/// Extract GPU model from laptop product name  
fn extract_gpu_from_laptop_name(name: &str) -> String {
    let name_upper = name.to_uppercase();
    
    // Look for RTX patterns (e.g., RTX 5070, RTX 4060, RTX 3080Ti)
    if let Some(idx) = name_upper.find("RTX") {
        let after_rtx = &name[idx..];
        // Extract "RTX XXXX" or "RTX XXXXTi"
        let parts: Vec<&str> = after_rtx.split_whitespace().take(2).collect();
        if parts.len() >= 2 {
            let model = parts[1];
            // Check if next part is "Ti" suffix
            if after_rtx.to_uppercase().contains(&format!("RTX {}TI", model.to_uppercase())) {
                return format!("RTX {}Ti", model);
            }
            return format!("RTX {}", model);
        }
    }
    
    // Look for GTX patterns
    if let Some(idx) = name_upper.find("GTX") {
        let after_gtx = &name[idx..];
        let parts: Vec<&str> = after_gtx.split_whitespace().take(2).collect();
        if parts.len() >= 2 {
            return format!("GTX {}", parts[1]);
        }
    }
    
    String::new()
}

/// Extract warranty info from order rows
fn extract_warranty_from_order(order: &Order) -> String {
    for row in order.associations.order_rows.iter() {
        let r = row.product_reference.to_lowercase();
        
        // Check for WTY/ prefix warranties
        if r.starts_with("wty/") {
            return row.product_name.clone();
        }
        
        // Check for PCL/ prefix warranties
        // PCL/12MONTH, PCL/18MONTH, PCL/6MONTH, PCL/90DAY, PCL/R2R-WAR/2Y
        if r == "pcl/12month" {
            return "12 Month Warranty".to_string();
        }
        if r == "pcl/18month" {
            return "18 Month Warranty".to_string();
        }
        if r == "pcl/6month" {
            return "6 Month Warranty".to_string();
        }
        if r == "pcl/90day" {
            return "90 Day Warranty".to_string();
        }
        if r == "pcl/r2r-war/2y" {
            return "2 Year Parts Warranty".to_string();
        }
    }
    "None".to_string()
}

/// Calculate spiffs for an order (using same logic as koth)
fn calculate_spiffs_for_order(order: &Order) -> f64 {
    let mut spiffs_total: f64 = 0.0;
    let mut has_system_product = false;
    let mut cps_units: i32 = 0;
    let mut has_sas: bool = false;
    let mut has_wrav: bool = false;
    
    for row in order.associations.order_rows.iter() {
        let r = row.product_reference.to_lowercase();
        let qty: i32 = row.product_quantity.parse().unwrap_or(1);
        
        // Track if this order contains a system product
        if r.starts_with("lap/") || (r.starts_with("case/") && !r.starts_with("case/15") && !r.starts_with("case/17")) {
            has_system_product = true;
        }
        
        // Track SAS/WRAV presence
        if r.starts_with("sw/sas") { has_sas = true; }
        if r.starts_with("sw/wrav") { has_wrav = true; }
        
        // CPS $10 (not cps-plat)
        if r.starts_with("sw/cps") && !r.starts_with("sw/cps-plat") {
            cps_units += qty;
        }
        
        // CPS Plat $25
        if r.starts_with("sw/cps-plat") {
            spiffs_total += 25.0 * qty as f64;
        }
        
        // SEB/Year $15
        if r == "seb/year" {
            spiffs_total += 15.0 * qty as f64;
        }
        
        // Parts with $2 spiff
        if r.starts_with("mon/")
            || r.starts_with("kb/")
            || r.starts_with("mou/")
            || r.contains("/dock/")
            || r == "dvdrw/usb"
            || r.starts_with("case/15")
            || r.starts_with("case/17")
            || r.starts_with("spkr/")
            || r.starts_with("belk/")
        {
            spiffs_total += 2.0 * qty as f64;
        }
    }
    
    // CPS spiff rules: $10 per CPS, but paired with SAS/WRAV means first unit is free
    if cps_units > 0 {
        let mut payable_cps = cps_units;
        if has_system_product && (has_sas || has_wrav) {
            payable_cps = (cps_units - 1).max(0);
        }
        spiffs_total += 10.0 * payable_cps as f64;
    }
    
    spiffs_total
}

/// Get cost for the main system product from Odoo
async fn get_system_cost_from_order(order: &Order) -> f64 {
    // Find the main system product
    for row in order.associations.order_rows.iter() {
        let r = row.product_reference.to_lowercase();
        if r.starts_with("lap/") 
            || (r.starts_with("case/") && !r.starts_with("case/15") && !r.starts_with("case/17"))
            || r.starts_with("bsd/")
            || r.starts_with("rci/")
            || r.starts_with("r2r/")
            || r.starts_with("rtr/")
        {
            if let Ok(Some(result)) = get_product_cost_from_odoo(&row.product_reference).await {
                return result.cost;
            }
        }
    }
    0.0
}
