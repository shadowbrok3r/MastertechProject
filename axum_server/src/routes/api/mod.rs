pub mod surreal;
pub mod orders;
pub mod parts;

pub use surreal::*;
pub use orders::*;
// pub use parts::*;

pub fn routes(state: crate::AppState) -> axum::Router {
    axum::Router::new()
        .route(
            "/api/orders/{endpoint}/overdueCalls", 
            axum::routing::post(get_all_missed_calls)
        )
        .route(
            "/api/orders/dailySales", 
            axum::routing::post(get_all_missed_calls)
        )
        .route(
            "/api/parts/{part_request_type}", 
            axum::routing::post(get_all_missed_calls)
        )
        .route(
            "/api/repo", 
            axum::routing::post(surreal::handle_response)
        )
        .with_state(state)
}