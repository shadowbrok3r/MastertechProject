use routes::api::prestashop::MissedCallOrder;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use tower_http::{add_extension::AddExtensionLayer, cors::CorsLayer};
use middleware::{context::Ctx, middleware_log::middleware_logger};
use axum::{middleware::map_response, Router};
use std::collections::HashMap;
use std::net::SocketAddr;
use tokio::sync::Mutex;
use std::sync::Arc;
use dotenv::dotenv;
use uuid::Uuid;
use log::info;


pub mod middleware;
pub mod error;
pub mod routes;


#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>>{
	dotenv().ok();

	tracing_subscriber::registry() // level set by either RUST_LOG env variable or defaults to debug
		.with(tracing_subscriber::EnvFilter::new(
		std::env::var("RUST_LOG").unwrap_or_else(|_| "mtech_srv=trace".into()),
	))
	.with(tracing_subscriber::fmt::layer())
	.init();

	let server_url = dotenv::var("SERVER_URL").unwrap_or("0.0.0.0".to_string());
	let server_port = dotenv::var("SERVER_PORT").unwrap_or("8082".to_string());
	let addr: SocketAddr = format!("{}:{}", server_url, server_port).parse().expect("Can not parse address and port");
	let listener = tokio::net::TcpListener::bind(&addr).await?;
	info!("Starting server on {addr}");

	// Initialize the application state
    let app_state = AppState::new();


	let app = Router::new()
		// routes requring auth
		// .route_layer(middleware::from_fn(mw_require_auth))
		// .layer(layer)
		// Routes
		.merge(routes::routes(app_state.clone()))
        // .merge(routes::routes())
        // Layers
        .layer(map_response(middleware_logger))
		.layer(CorsLayer::permissive())
		.layer(
			AddExtensionLayer::new(
				Ctx::new(
					Ok("Shadowbroker".to_string()), 
					Uuid::new_v4()
				)
			)
		);
		
	axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await?;
	Ok(())
}

#[derive(Clone)]
pub struct AppState {
    cache: Arc<Mutex<HashMap<String, CachedData>>>,
}

impl AppState {
    pub fn new() -> Self {
        AppState {
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct CachedData {
	orders: Vec<MissedCallOrder>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct RefreshRequest {
    pub refresh: bool, // Flag to indicate cache refresh
}