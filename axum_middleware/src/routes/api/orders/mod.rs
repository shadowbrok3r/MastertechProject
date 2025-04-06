use database::schema::{deserializer::deserialize_to_string, prestashop_schema::Employee};
use crate::{error::ApiError, middleware::context::Ctx, AppState};
use axum::{extract::State, Json};

pub mod data;
pub mod schedule;

#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize)]
pub struct MissedCallOrder {
    #[serde(deserialize_with = "deserialize_to_string")]
    pub id: String,
    // 2025-04-04 16:48:01
    pub date_add: String,
    #[serde(default, skip_deserializing)]
    pub missing_days: Vec<String>,
}

#[derive(Debug, Default, serde::Deserialize, serde::Serialize)]
pub struct RequiredData {
    pub refresh: bool,
    pub schedule: bool,
    pub employee_id: String,
    pub employee_store: String,
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
) -> Result<Json<crate::CachedData>, ApiError> {
    println!("Got request: {payload:?}");
    
    let refresh = payload.refresh;
    let schedule_job = payload.schedule;

    let employee = Employee {
        id: payload.employee_id,
        id_store: payload.employee_store,
        ..Default::default()
    };

    let mut cache = state.cache.lock().await;
    let endpoint_key = format!("/api/{}", endpoint.as_str());

    // If refresh is true, grab new data immediately
    if refresh {
        match super::data::get_services_by_status(endpoint.id(), &employee.id_store).await {
            Ok(new_orders) => {
                let new_data = crate::CachedData { orders: new_orders };
                cache.insert(endpoint_key.clone(), new_data);
            }
            Err(e) => {
                return Err(ApiError {
                    error: crate::error::Error::Generic { description: e.to_string() },
                    req_id: ctx.req_id(),
                });
            }
        }
    }

    // If scheduling is requested, start the cron job that updates the cache periodically
    if schedule_job {
        super::schedule::start_cron_job(
            state.clone(), 
            endpoint.id().to_string(),
            endpoint_key.clone(), 
            employee.clone()
        )
        .await
        .map_err(|e| ApiError {
            error: crate::error::Error::Generic { description: e.to_string() },
            req_id: ctx.req_id(),
        })?;
    }

    // If no refresh and cache is empty, fetch data now
    if !cache.contains_key(&endpoint_key) {
        match super::data::get_services_by_status(endpoint.id(), &employee.id_store).await {
            Ok(new_orders) => {
                let new_data = crate::CachedData { orders: new_orders };
                cache.insert(endpoint_key.clone(), new_data);
            }
            Err(e) => {
                return Err(ApiError {
                    error: crate::error::Error::Generic { description: e.to_string() },
                    req_id: ctx.req_id(),
                });
            }
        }
    }

    let cached_data = cache.get(&endpoint_key).unwrap().clone();
    Ok(Json(cached_data))
}



