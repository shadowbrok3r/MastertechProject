use axum::{middleware::map_response, Router};
use log::info;
use middleware::middleware_log::middleware_logger;
use tower_http::cors::CorsLayer;
use std::net::SocketAddr;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use dotenv::dotenv;


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
	let app = Router::new()
		// routes requring auth
        .merge(routes::routes());
		// .route_layer(middleware::from_fn(mw_require_auth))
		// .layer(layer)
		// Routes
        // Layers
		// .layer(CorsLayer::permissive())
        // .layer(map_response(middleware_logger));
		
	axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await?;
	Ok(())
}