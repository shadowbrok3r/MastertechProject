pub mod surreal;
pub mod orders;
pub mod parts;
pub mod qc_fleet;
pub mod scheduled;
pub mod build;

pub use surreal::*;
pub use orders::*;
pub use qc_fleet::*;
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
        .merge(qc_fleet::qc_fleet_routes())
        .merge(build::build_routes())
        .with_state(state)
}