use axum::{middleware::{self, map_response}, routing::{get, post}, Router};
use routes::api::live::{client_state::Sessions, live_connection_handler, websocket_middleware::authenticate_middleware};
use socketioxide::{handler::ConnectHandler, SocketIo};
use tower_cookies::CookieManagerLayer;
use tower_http::{add_extension::AddExtensionLayer, cors::CorsLayer};
use std::net::SocketAddr;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use dotenv::dotenv;
use database::Database;
use crate::{
    middlewares::{
		middleware_log::middleware_logger, 
		auth_middleware::{mw_require_auth, jwt_auth}}, 
    routes::{
        web_console::on_connect, 
		api_routes, 
        user::{
			self, 
			logout, 
			CreateAccountAction, 
			LoginAction}
    }
};

pub mod middlewares;
pub mod utils;
mod routes;


#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>>{
	dotenv().ok();

	tracing_subscriber::registry() // level set by either RUST_LOG env variable or defaults to debug
		.with(tracing_subscriber::EnvFilter::new(
		std::env::var("RUST_LOG").unwrap_or_else(|_| "mtech_srv=trace".into()),
	))
	.with(tracing_subscriber::fmt::layer())
	.init();

	let server_url = dotenv::var("SERVER_URL").expect("SERVER_URL undefined");
	let server_port = dotenv::var("SERVER_PORT").expect("SERVER_PORT undefined");
	
	let addr: SocketAddr = format!("{}:{}", server_url, server_port).parse().expect("Can not parse address and port");
	let listener = tokio::net::TcpListener::bind(&addr).await?;
	let db = Database::new().await;

	let (layer, io) = SocketIo::builder()
		.with_state(Sessions::default())
		.with_state(db.clone())
		.build_layer();

	io.ns("/live", live_connection_handler.with(authenticate_middleware));
	io.ns("/ws", on_connect); // on_connect.with(authenticate_middleware)
	

	let app = Router::new()
		// routes requring auth
        .merge(api_routes::routes())
		// .route("/api/downloads/:file_path",get(downloads::download_mastertech))
		.route_layer(middleware::from_fn(mw_require_auth))
		.layer(layer)
		// Routes
        .route("/login", post(user::account_action_handler::<LoginAction>))
        .route("/logout", get(logout))
        .route("/signup", post(user::account_action_handler::<CreateAccountAction>))
        // Layers
		.layer(CorsLayer::permissive())
        .layer(map_response(middleware_logger))
        .layer(middleware::from_fn(jwt_auth))
		.layer(AddExtensionLayer::new(db))
		// Layers are executed from bottom up,
		// so CookieManager has to be under ctx_constructor
		.layer(CookieManagerLayer::new());
		
	axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await?;
	Ok(())
}