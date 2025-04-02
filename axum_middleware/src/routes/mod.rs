use api::{surreal::handle_response, prestashop::handle_prestashop_response};
use axum::{routing::{get, post}, Router};
pub mod api;

pub fn routes() -> Router {
    Router::new()
        .route(
            "/api/submitTicket", 
            post(handle_response)
        )
        .route(
            "/api/getOpenOrders", 
            post(handle_prestashop_response)
        )
}