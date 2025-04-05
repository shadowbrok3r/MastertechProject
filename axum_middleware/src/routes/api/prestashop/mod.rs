use database::schema::{prestashop_schema::{self, Employee, Prestashop}, utilities::has_missed_calls};
use crate::{error::ApiError, middleware::context::Ctx, AppState};
use tokio_cron_scheduler::{Job, JobScheduler};
use axum::{extract::State, Json};
use std::collections::HashMap;
use anyhow::Context;


#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize)]
pub struct DateAndOrderNumber {
    // 2025-04-04 16:48:01
    date_add: String,
    id: String
}

#[derive(Default, serde::Deserialize, serde::Serialize)]
pub struct RequiredData {
    refresh: bool,
    employee_id: String,
    employee_store: String
}

#[derive(Default, serde::Deserialize, serde::Serialize)]
pub enum OrderType {
    #[default]
    CheckinShelf,
    InRepair,
    DoneShelf
}

impl OrderType {
    fn as_str(&self) -> &str {
        match self {
            OrderType::CheckinShelf => "checkinShelf",
            OrderType::InRepair => "inRepair",
            OrderType::DoneShelf => "doneShelf",
        }
    }

    // 30=In Repair, 239=Accepted by Odoo?, 29=CheckinShelf, 40=DoneShelf, 73=Order Placed, 70=PrePulled236=ShipToStore
    fn id(&self) -> &str {
        match self {
            OrderType::CheckinShelf => "29",
            OrderType::InRepair => "30",
            OrderType::DoneShelf => "40",
        }
    }
}

pub async fn get_all_missed_calls(
    State(state): State<AppState>,
    ctx: Ctx,
    axum::extract::Path(endpoint): axum::extract::Path<OrderType>,
    Json(payload): Json<RequiredData>,
) 
    -> Result<Json<crate::CachedData>, ApiError> 
{
    let refresh = payload.refresh;

    let employee = Employee {
        id: payload.employee_id,
        id_store: payload.employee_store,
        ..Default::default()
    };
    
    let mut cache = state.cache.lock().await;
    let endpoint_key = format!("/api/{}", endpoint.as_str());

    if refresh || !cache.contains_key(&endpoint_key) {
        // Clear cache and fetch new data if refresh is requested or cache is empty
        cache.remove(&endpoint_key);
        let new_data = crate::CachedData::default(); // fetch_prestashop_data(&endpoint_key).await?;
        cache.insert(endpoint_key.clone(), new_data);
    }

    let cached_data = cache.get(&endpoint_key).unwrap(); // Safe due to prior insertion

    start_cron_job(
        state.clone(), 
        endpoint.id().to_string(),
        endpoint_key, 
        employee.clone()
    )
    .await
    .map_err(|e| ApiError {
        error: crate::error::Error::Generic { description: e.to_string() },
        req_id: ctx.req_id(),
    })?;

    Ok(Json(cached_data.clone()))
}

async fn start_cron_job(
    state: AppState, 
    id: String,
    endpoint: String, 
    employee: Employee
) 
    -> anyhow::Result<(), anyhow::Error> 
{
    let sched = JobScheduler::new().await?;

    // Schedule a job to run at 2 AM every night ("0 0 2 * * * *")
    sched.add(
                                // Every two minutes just as a test
        Job::new_async("0 */2 * * * *", move |_uuid, _l| {
            let state = state.clone();
            let endpoint = endpoint.clone();
            let store = employee.id_store.clone();
            let id = id.clone();
            Box::pin(async move {
                let mut cache = state.cache.lock().await;

                // Fetch services within the range
                let orders = get_services_by_status(&id, &store).await;

                // Handle the fetched services
                match orders {
                    Ok(svcs) => { cache.insert(endpoint, crate::CachedData { orders: svcs.clone() }); },
                    Err(e) => println!("Error getting in repair shelf services: {:?}", e)
                };

            })
        })?
    ).await?;

    sched.start().await?;

    Ok(())
}

async fn get_services_by_status(
    status: &str, 
    store: &str
) 
    -> anyhow::Result<Vec<DateAndOrderNumber>, anyhow::Error> 
{
    let mut api_call = Prestashop::default();
    let mut query: HashMap<&str, &str> = HashMap::new();
    let mut missed_orders = Vec::new();
    query.insert("filter[current_state]", status);
    query.insert("filter[id_order_type]", "2");
    query.insert("filter[id_store]", store);
    query.insert("output_format", "JSON");
    query.insert("sort", "[id_DESC]");
    api_call.display = "[id, date_add]";

    let orders: Vec<DateAndOrderNumber> = api_call
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

        println!("helper_traits -> Pulling order {}", order.id);
        
        query.insert("filter[id]", order.id.as_str());
        query.insert("output_format", "JSON");

        let customer_threads: Vec<prestashop_schema::CustomerThread> = api_call
            .request_resources_wasm("customer_threads", query.clone())
            .await?;

        let mut customer_messages: Vec<prestashop_schema::CustomerMessage> = Vec::new();

        if !customer_threads.is_empty() {
            for thread in customer_threads.iter() {
                for msg in thread.associations.customer_messages.iter() {
                    let msg =  api_call
                        .request_subresources_by_id_wasm(
                            "customer_messages",
                            "customer_message",
                            msg.id.as_str(),
                        )
                        .await?;
                    customer_messages.push(msg)
                }
            }
        }

        println!("helper_traits -> Orders list: {orders:?}");
        
        // Compare dates: if any required call day is missing, mark this order as having missed calls.
        if has_missed_calls(&order.date_add, &customer_messages) {
            missed_orders.push(order.clone());
        }
    }

    println!("Missed orders: {:?}", missed_orders);

    Ok(missed_orders)
}