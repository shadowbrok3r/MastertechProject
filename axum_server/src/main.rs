use database::schema::prestashop_schema::MissedCallOrder;
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

pub use routes::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();

    // Default filter has to actually mention our crate(s) or every tracing::info!
    // gets swallowed. The previous default was "mtech_srv=trace" — that's the
    // name of an entirely different project. RUST_LOG, if set, still wins.
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| {
                "info,axum_server=debug,tower_http=info,database=info".into()
            }),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();
    tracing::info!(
        "axum_server: starting v{} (pid={})",
        env!("CARGO_PKG_VERSION"),
        std::process::id()
    );

    let server_url = dotenv::var("SERVER_URL").unwrap_or("0.0.0.0".to_string());
    let server_port = dotenv::var("SERVER_PORT").unwrap_or("8082".to_string());
    let addr: SocketAddr = format!("{server_url}:{server_port}")
        .parse()
        .expect("Can not parse address and port");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!("Starting server on {addr}");

    let app_state = AppState::new();

    // Bring up the SurrealDB connection so DB-backed routes (e.g. the
    // `/api/build/*` work queue) can land queries. We don't refuse to
    // start the HTTP server on a DB failure: the in-memory routes
    // (qc_fleet, orders) still work, and the build routes return a
    // 500 with the underlying error in the body until DB is reachable.
    match database::init_database().await {
        Ok(()) => tracing::info!("axum_server: SurrealDB connected + signin OK"),
        Err(e) => tracing::error!("axum_server: SurrealDB init failed: {e:?}"),
    }

    // Re-hydrate the fleet from SurrealDB so an axum_server restart doesn't
    // black-hole the admin dashboard until every agent re-registers. Best
    // effort: if the DB is unreachable, the in-memory state starts empty
    // and agents will repopulate it on their next heartbeat.
    match routes::api::qc_fleet::hydrate_from_db(&app_state.fleet).await {
        Ok(n) => tracing::info!("axum_server: hydrated {n} fleet_agent row(s) from SurrealDB"),
        Err(e) => tracing::warn!("axum_server: fleet hydration skipped: {e:?}"),
    }

    // Start the shared cron scheduler. We keep the returned handle alive
    // for the whole process lifetime — dropping it would silently cancel
    // every registered job. New periodic chores should be added inside
    // `scheduled::spawn_cron_scheduler` rather than spawning their own.
    let _cron = match routes::api::scheduled::spawn_cron_scheduler().await {
        Ok(sched) => Some(sched),
        Err(e) => {
            // Don't refuse to start the HTTP server over a cron failure;
            // the heartbeat sweep is best-effort.
            tracing::error!("Failed to start cron scheduler: {e:?}");
            None
        }
    };

    // Plain-TCP QC fingerprint listener for pre-OS UEFI agents (raw TCP4, no
    // TLS). Runs alongside the HTTP server; best-effort (logs + disables itself
    // if the port can't bind).
    tokio::spawn(routes::api::qc_tcp::serve());

    let app = Router::new()
        .merge(routes(app_state.clone()))
        .layer(map_response(middleware_logger))
        .layer(CorsLayer::permissive())
        .layer(AddExtensionLayer::new(Ctx::new(
            Ok("Shadowbroker".to_string()),
            Uuid::new_v4(),
        )));

    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
            tracing::info!("shutdown signal received");
        })
        .await?;

    Ok(())
}

#[derive(Clone)]
pub struct AppState {
    cache: Arc<Mutex<HashMap<String, CachedData>>>,
    /// Shared fleet state for the QC orchestrator routes.
    pub fleet: SharedFleetState,
}

impl AppState {
    pub fn new() -> Self {
        AppState {
            cache: Arc::new(Mutex::new(HashMap::new())),
            fleet: Arc::new(Mutex::new(FleetState::default())),
        }
    }
}

#[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct CachedData {
    orders: Vec<MissedCallOrder>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct RefreshRequest {
    pub refresh: bool,
}
