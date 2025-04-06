use database::schema::{prestashop_schema::{self, Prestashop}, utilities::get_missing_call_days};
use std::collections::HashMap;
use anyhow::Context;

use super::MissedCallOrder;

pub async fn get_services_by_status(
    status: &str, 
    store: &str
) -> anyhow::Result<Vec<MissedCallOrder>, anyhow::Error> {
    let mut api_call = Prestashop::default();
    let mut query: HashMap<&str, &str> = HashMap::new();
    let mut missed_orders = Vec::new();
    
    query.insert("filter[current_state]", status);
    query.insert("filter[id_order_type]", "2");
    query.insert("filter[id_store]", store);
    query.insert("output_format", "JSON");
    query.insert("sort", "[id_DESC]");
    api_call.display = "[id, date_add]";

    let orders: Vec<MissedCallOrder> = api_call
        .request_resources_wasm("orders", query.clone())
        .await
        .context("Pulling orders list")?;

    println!("Orders: {orders:?}");
    println!("Api query: {query:?}");

    for order in orders.iter() {
        let api_call = Prestashop::default();
        let mut query = HashMap::new();
        
        if order.id.is_empty() {
            break;
        }

        println!("Pulling order {}", order.id);
        
        query.insert("filter[id_order]", order.id.as_str());
        query.insert("output_format", "JSON");

        let customer_threads: Vec<prestashop_schema::CustomerThread> = api_call
            .request_resources_wasm("customer_threads", query.clone())
            .await?;

        let mut customer_messages: Vec<prestashop_schema::CustomerMessage> = Vec::new();

        if !customer_threads.is_empty() {
            for thread in customer_threads.iter() {
                for msg in thread.associations.customer_messages.iter() {
                    let msg = api_call
                        .request_subresources_by_id_wasm(
                            "customer_messages",
                            "customer_message",
                            msg.id.as_str(),
                        )
                        .await?;
                    customer_messages.push(msg);
                }
            }
        }
        
        // Get the missing days for this order.
        let missing_days = get_missing_call_days(&order.date_add, &customer_messages);
        
        // Only include orders with missing call days.
        if !missing_days.is_empty() {
            missed_orders.push(MissedCallOrder {
                date_add: order.date_add.clone(),
                id: order.id.clone(),
                missing_days,
            });
        }
    }

    println!("Missed orders: {:#?}", missed_orders);

    Ok(missed_orders)
}