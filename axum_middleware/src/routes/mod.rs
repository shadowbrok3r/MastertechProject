use api::{surreal::handle_response, prestashop::get_all_missed_calls};
use axum::{routing::{get, post}, Router};

use crate::AppState;
pub mod api;

pub fn routes(state: AppState) -> Router {
    Router::new()
        .route(
            "/api/orders/{endpoint}/overdueCalls", 
            post(get_all_missed_calls)
        )
        .route(
            "/api/orders/soldComputers", 
            post(get_all_missed_calls)
        )
        .route(
            "/api/repo/checkinShelf", 
            post(handle_response)
        )
        .route(
            "/api/repo/inRepair", 
            post(get_all_missed_calls)
        )
        .route(
            "/api/repo/doneShelf", 
            post(get_all_missed_calls)
        )
        .with_state(state)
}