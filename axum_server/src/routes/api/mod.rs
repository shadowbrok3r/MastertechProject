pub mod admin;
pub mod audit;
pub mod surreal;
pub mod orders;
pub mod order_lookup;
pub mod parts;
pub mod firmware;
pub mod preboot;
pub mod qc_fleet;
pub mod qc_tcp;
pub mod qc_udp;
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
        .route(
            "/api/v1/qc/order-by-serial/{serial}",
            axum::routing::get(order_lookup::order_by_serial)
        )
        .merge(qc_fleet::qc_fleet_routes())
        .merge(firmware::firmware_routes())
        .merge(preboot::preboot_routes())
        .merge(build::build_routes())
        .merge(admin::admin_routes())
        .merge(audit::audit_routes())
        .with_state(state)
}