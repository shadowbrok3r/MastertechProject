use api::surreal::handle_response;
use axum::{Router, routing::post};
pub mod api;

pub fn routes() -> Router {
    Router::new()
        .route(
            "/api/submitTicket", 
            post(handle_response)
        )
        // .route(
        //     "/api/sql", 
        //     get(handle_get_ticket)
        // )
}