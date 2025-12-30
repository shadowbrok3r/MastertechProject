use super::{store_inventory_viewer::ExtraInventoryData, row_viewer::{RawStockData, SerialData, StockData, CostBreakdownData}};
use database::{DATABASE, schema::prestashop::{Customer, Order, Prestashop}};
use crossbeam::channel::Sender;
use anyhow::{Error, Result};
use serde::Deserialize;
use serde_json::json;
use reqwest::Client;
use log::info;

pub async fn get_stock(stock_tx: Sender<Vec<RawStockData>>, location: u64) -> Result<(), Error> {
    let res: Option<StockData> = DATABASE
        .query("RETURN fn::store_stock($location, 5000)")
        .bind(("location", location))
        .await?
        .take(0)?;

    // info!("Result: {res:?}");

    stock_tx.try_send(res.unwrap().result)?;
    Ok(())
}

pub async fn find_attached_serial(
    serial: String,
    stock_tx: Sender<SerialData>,
) -> Result<(), Error> {
    // info!("Finding S/N info: {serial}");
    let res: Option<SerialData> = DATABASE
        .query("RETURN fn::find_attached_serial($serial)")
        .bind(("serial", serial))
        .await?
        .take(0)?;

    // info!("Result: {res:?}");

    stock_tx.try_send(res.unwrap())?;
    Ok(())
}

pub async fn find_attached_serials(
    serials: Vec<String>,
    stock_tx: Sender<SerialData>,
) -> Result<(), Error> {
    // info!("Finding S/N info: {serials:?}");
    let res: Option<SerialData> = DATABASE
        .query("RETURN fn::find_attached_serials($serials)")
        .bind(("serials", serials))
        .await?
        .take(0)?;

    // info!("Result: {res:?}");

    stock_tx.try_send(res.unwrap())?;
    Ok(())
}

pub async fn find_products_by_name(
    serial: String,
    stock_tx: Sender<StockData>,
) -> Result<(), Error> {
    let res: Option<StockData> = DATABASE
        .query("RETURN fn::search_stock($serial)")
        .bind(("serial", serial))
        .await?
        .take(0)?;

    // info!("Result: {res:?}");

    stock_tx.try_send(res.unwrap())?;
    Ok(())
}

pub async fn get_extra_stock_info(stock_tx: Sender<Vec<ExtraInventoryData>>) -> Result<(), Error> {
    let res: Vec<ExtraInventoryData> = DATABASE
        .query("RETURN fn::get_stock_extra_info(5000)")
        .await?
        .take(0)?;

    stock_tx.try_send(res)?;
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
    let url = "https://odoo.master-tech.app/jsonrpc";
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
