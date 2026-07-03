use surrealdb::{
    engine::remote::ws::{Client as WsClient, Ws, Wss}, 
    opt::auth::Record as SurrealRec, 
    Surreal
};

// Re-export SurrealValue from the correct location
pub use surrealdb::types::SurrealValue;
use once_cell::sync::Lazy;
use serde::Serialize;
use std::fmt::Debug;
use schema::User;
use log::info;

pub mod live_data;
// SNTP-based startup clock correction; UdpSocket/SetSystemTime are native-only.
#[cfg(not(target_arch = "wasm32"))]
pub mod clock_sync;
// Native-only HTTP order backends; their futures require Send, unavailable on wasm reqwest.
#[cfg(not(target_arch = "wasm32"))]
pub mod orders;
pub mod schema;
#[cfg(not(target_arch = "wasm32"))]
pub mod xbm;

pub use platform::PlatformSpawner;

use crate::schema::{NOTIFICATION_TABLE, Notification, file_storage};

pub static DATABASE: Lazy<Surreal<WsClient>> = Lazy::new(Surreal::init);

pub const USER_SCOPE: &str = env!("USER_SCOPE");
pub const DB: &str = env!("DB");
pub const NS: &str = env!("NS");
pub const SCAFFOLD_URL: &str = env!("SCAFFOLD_URL");
pub const SCAFFOLD_USER: &str = env!("SCAFFOLD_USER");
pub const SCAFFOLD_PASS: &str = env!("SCAFFOLD_PASS");
pub const DB_URL: &str = env!("DB_URL");
pub const STORAGE_URL: &str = env!("STORAGE_URL");
pub const REGION: &str = env!("REGION");
pub const DB_URL_DEV: &str = env!("DB_URL_DEV");
pub const DB_URL_LOCAL: &str = env!("DB_URL_LOCAL");
pub const DB_URL_BETA: &str = env!("DB_URL_BETA");
pub const WS_CLIENT_URL_LOCAL: &str = env!("WS_CLIENT_URL_LOCAL");
pub const WS_MASTER_URL_LOCAL: &str = env!("WS_MASTER_URL_LOCAL");
pub const WS_CLIENT_URL: &str = env!("WS_CLIENT_URL");
pub const WS_MASTER_URL: &str = env!("WS_MASTER_URL");

/// Build a WebSocket URL with `room_id` and `role` query parameters.
///
/// The websocket server defaults missing `role` to **`client`**. If the admin console
/// connects without `role=master`, it is treated as a second client and **replaces** the
/// real remote client in the room — use `role` `"master"` for all admin URLs and `"client"`
/// for remote Mastertech clients.
#[must_use]
pub fn websocket_url_with_room(base_url: &str, room_id: &str, role: &str) -> String {
    let base = base_url.trim_end_matches(['&', '?']).trim_end();
    let join = if base.contains('?') { '&' } else { '?' };
    format!("{base}{join}room_id={room_id}&role={role}")
}

pub const ODOO_API_KEY: &str = env!("ODOO_API_KEY");

/// Build the version-with-build-hash display string for UI and logs.
///
/// Expands at the **caller's** crate boundary, so each consumer's own
/// `CARGO_PKG_VERSION` and `BUILD_HASH` are baked in (the latter is
/// emitted by each crate's `build.rs` via the shared `build_hash.rs`).
///
/// Example output: `"v4.7.8 (1a3f2b9d.e8a3c1)"` — the suffix changes
/// every iterative dev build so two compiles at the same version
/// number are still distinguishable in screenshots and log scrapes.
///
/// Returns a `&'static str` (via `concat!`) so it works in every API
/// that wants a static string (CLI `--version`, window titles, etc.).
#[macro_export]
macro_rules! version_with_build {
    () => {
        concat!(
            "v",
            env!("CARGO_PKG_VERSION"),
            " (",
            env!("BUILD_HASH"),
            ")"
        )
    };
}

/// SurrealDB `guest` record access password (rotate server-side when exposed).
pub const SURREAL_GUEST_PASSWORD: &str = env!("SURREAL_GUEST_PASSWORD");

pub const BUCKET_DEV_WINDOWS_URL: &str = env!("BUCKET_DEV_WINDOWS_URL");
pub const BUCKET_DEV_LINUX_URL: &str = env!("BUCKET_DEV_LINUX_URL");
pub const BUCKET_URL: &str = env!("BUCKET_URL");

pub const ODOO_JSONRPC_URL: &str = env!("ODOO_JSONRPC_URL");
pub const ODOO_DB: &str = env!("ODOO_DB");
pub const ODOO_UID: &str = env!("ODOO_UID");

pub const PRESTASHOP_API_URL: &str = env!("PRESTASHOP_API_URL");
pub const PRESTASHOP_API_URL_WASM: &str = env!("PRESTASHOP_API_URL_WASM");

/// Base URL for the Xidax admin (PrestaShop) backoffice. Use the helpers
/// (`xidax_order_url`, `xidax_product_url`) instead of concatenating ad-hoc
/// suffixes so URL paths stay defined in one place.
pub const XIDAX_ADMIN_URL: &str = env!("XIDAX_ADMIN_URL");

/// Production fleet-orchestrator base (axum_server). Empty string disables
/// fleet reporting at runtime. Use [`orchestrator_url`] to pick between this
/// and [`ORCHESTRATOR_URL_DEV`] based on the build profile.
pub const ORCHESTRATOR_URL: &str = env!("ORCHESTRATOR_URL");

/// Dev fleet-orchestrator base. Selected when the binary is built with
/// `cfg(debug_assertions)` (i.e. any non-release profile).
pub const ORCHESTRATOR_URL_DEV: &str = env!("ORCHESTRATOR_URL_DEV");

/// PrestaShop employee credential-check endpoint (QC tech sign-off).
/// Empty string disables tech authentication at runtime.
pub const PRESTASHOP_AUTH_URL: &str = env!("PRESTASHOP_AUTH_URL");

/// Shopify Admin API base.
/// Empty string disables the Shopify order backend at runtime.
pub const SHOPIFY_STORE_URL: &str = env!("SHOPIFY_STORE_URL");
/// Read-only Admin API token. Writes go through the Worker, never from here.
pub const SHOPIFY_ADMIN_TOKEN: &str = env!("SHOPIFY_ADMIN_TOKEN");
pub const SHOPIFY_API_VERSION: &str = env!("SHOPIFY_API_VERSION");

/// Xidax Build Management API base (`/api/v1`).
pub const XBM_API_URL: &str = env!("XBM_API_URL");
/// Per-consumer `xbm_` bearer key. Empty string disables the XBM client at
/// runtime; reads then fall back to the Admin GraphQL path where available.
pub const XBM_API_KEY: &str = env!("XBM_API_KEY");

/// Pick the active orchestrator URL based on the current build profile.
/// Debug → [`ORCHESTRATOR_URL_DEV`]; release → [`ORCHESTRATOR_URL`].
/// Callers should treat an empty return as "fleet disabled" and short-circuit.
#[inline]
pub fn orchestrator_url() -> &'static str {
    if cfg!(debug_assertions) {
        ORCHESTRATOR_URL_DEV
    } else {
        ORCHESTRATOR_URL
    }
}

/// Build a Xidax admin URL that opens an order detail page.
#[must_use]
pub fn xidax_order_url(order_id: impl std::fmt::Display) -> String {
    format!("{XIDAX_ADMIN_URL}/index.php?controller=AdminOrders&vieworder=&id_order={order_id}")
}

/// Build a Xidax admin URL that opens a product detail page.
#[must_use]
pub fn xidax_product_url(product_id: impl std::fmt::Display) -> String {
    format!("{XIDAX_ADMIN_URL}/index.php/sell/catalog/products/{product_id}")
}

// JWT token type - in v3.0 this is just a String
pub type Jwt = String;

// The static variable holding the currently logged-in user
// Wrapped in Mutex for safe interior mutability
// Wrapped in Lazy for easy static initialization
pub static CURRENT_USER_INFO: Lazy<std::sync::Mutex<Option<User>>> = Lazy::new(|| {
    std::sync::Mutex::new(None) // Initialize with no user logged in
});

/// In-memory cache of the credentials we used for the most recent
/// successful sign-in, so [`ensure_db_connected`] can transparently re-auth
/// after a SurrealDB hiccup instead of dropping the operator to guest.
///
/// Prefer the `jwt` path on replay — it's the same token SurrealDB itself
/// issued, and using it avoids putting a cleartext password on the wire a
/// second time. The `email` / `password` pair is a fallback used only when
/// the JWT has expired (default ~1 h in SurrealDB).
///
/// **Security note:** this is *memory-only*, never persisted to disk. The
/// admin process already holds the operator's password in its login flow;
/// this static doesn't widen the attack surface beyond what's already in
/// the process address space. We deliberately do not log it.
#[derive(Clone)]
pub struct CachedAuth {
    pub jwt: Option<String>,
    pub email: Option<String>,
    pub password: Option<String>,
}

pub static CACHED_AUTH: Lazy<std::sync::Mutex<Option<CachedAuth>>> = Lazy::new(|| {
    std::sync::Mutex::new(None)
});

/// Helper for the signin sites. Writes any field that's `Some` and leaves
/// the others untouched, so a JWT-only path doesn't blow away a previously-
/// cached password.
pub fn cache_auth(jwt: Option<String>, email: Option<String>, password: Option<String>) {
    if let Ok(mut g) = CACHED_AUTH.try_lock() {
        let cur = g.clone().unwrap_or(CachedAuth {
            jwt: None,
            email: None,
            password: None,
        });
        *g = Some(CachedAuth {
            jwt: jwt.or(cur.jwt),
            email: email.or(cur.email),
            password: password.or(cur.password),
        });
    }
}

pub static STORE_USERS: Lazy<std::sync::Mutex<Vec<User>>> = Lazy::new(|| {
    std::sync::Mutex::new(vec![]) 
});

#[derive(Clone, Debug, Default)]
pub struct Database {
    pub jwt: Option<String>,
    pub user: Option<User>,
}

#[derive(Serialize, SurrealValue)]
pub struct Auth {
    pub email: String,
    pub password: String,
}

#[derive(Clone, Debug)]
pub struct Session {
    pub jwt: String,
    pub user: User,
}

#[derive(Serialize, Debug, Clone, PartialEq, SurrealValue)]
pub enum DatabaseSelection {
    Stable,
    Beta,
    Local
}

impl Default for DatabaseSelection {
    fn default() -> Self {
        if cfg!(debug_assertions) {
            Self::Local
        } else {
            Self::Stable
        }
    }
}

impl DatabaseSelection {
    pub fn get_db_url(&self) -> &'static str {
        match self {
            Self::Stable => DB_URL,
            Self::Beta => DB_URL_DEV,
            Self::Local => DB_URL_LOCAL,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Stable => "Stable",
            Self::Beta => "Beta",
            Self::Local => "Local",
        }
    }

    pub fn from_str(selection: &str) -> Self {
        match selection {
            "Stable"=> Self::Stable,
            "Beta"=> Self::Beta,
            "Local"=> Self::Local,
            _ => Self::Stable,
        }
    }

    pub async fn set_database(&self) -> anyhow::Result<(), anyhow::Error>{
        let inv = DATABASE.invalidate().await;
        match inv {
            Ok(_) => {
                let db = DATABASE.clone();
                drop(db);
                let url = self.get_db_url();
                match self {
                    Self::Stable => DATABASE.connect::<Wss>(url).await?,
                    Self::Beta => DATABASE.connect::<Wss>(url).await?,
                    Self::Local => DATABASE.connect::<Ws>(url).await?,
                };

                DATABASE.use_ns(NS).use_db(DB).await?;
            },
            Err(e) => log::error!("Failed to invalidate database connection: {e}"),
        }
        Ok(())
    }
}

impl Database {
    pub async fn new(
        email: String,
        password: String,
        jwt: Option<String>,
    ) -> anyhow::Result<Self, anyhow::Error> {
        if cfg!(debug_assertions) {
            let try_local = DATABASE.connect::<surrealdb::engine::remote::ws::Ws>(DB_URL_LOCAL).await;
            log::info!("Attempting to connect to local DB: {try_local:?}");
        } else {
            match DATABASE.connect::<surrealdb::engine::remote::ws::Wss>(DB_URL_DEV).await {
                Ok(_) => log::info!("Connected to {DB_URL_DEV:?}"),
                Err(e) => log::error!("Failed connecting to: {DB_URL_DEV:?}\n{e:?}"),
            }
        }

        match DATABASE.use_ns(NS).use_db(DB).await {
            Ok(_) => log::info!("Using NS: {NS:?}\nUsing DB: {DB:?}"),
            Err(e) => log::error!("Failed Using NS: {NS:?}\nFailed Using DB: {DB:?}\nE: {e:?}"),
        }

        log::info!("About to query DB. DATABASE ptr: {:p}", &*DATABASE);
        
        match jwt {
            Some(jwt) => {
                info!("Have a JWT, attempting token auth");
                DATABASE.authenticate(jwt.clone()).await?;
                // Cache the JWT so `ensure_db_connected` can replay it
                // after a DB blip without dropping the operator to guest.
                cache_auth(Some(jwt.clone()), None, None);
                let user: Option<User> = DATABASE.query("SELECT * FROM user WHERE id == $auth.id").await?.take(0)?;
                let users: Vec<User> = DATABASE.query("SELECT * FROM user WHERE active == true").await?.take(0)?;
                // let sess = DATABASE.query("RETURN <string>$session").await?.take::<Option<String>>(0)?;
                // log::info!("Session: {:?}", sess);
 
                if !users.is_empty() {
                    if let Ok(mut users_guard) = STORE_USERS.try_lock() {
                        *users_guard = users.clone(); 
                    }
                }
                if let Ok(mut user_info_guard) = CURRENT_USER_INFO.try_lock() {
                    // log::warn!("SET THE USER: {:?}", user_info_guard.clone());
                    *user_info_guard = user.clone(); // Set the user info
                }
                Ok( Self { jwt: Some(jwt.into()), user } )
            }
            None => {
                
                let full_email = if email.ends_with("@pclaptops.com") {
                    email.clone()
                } else {
                    format!("{}@pclaptops.com", email.clone())
                };

                let creds = SurrealRec {
                    namespace: NS.to_string(),
                    database: DB.to_string(),
                    access: USER_SCOPE.to_string(),
                    params: Auth { email: full_email.clone(), password: password.clone() },
                };

                info!("No JWT, sigining in: {:?}\n{:?}\n{:?}\n{:?}\n", full_email, creds.namespace, creds.database, creds.access);

                // Select a specific namespace / database
                let jwt = DATABASE
                    .signin(creds)
                    .await?;

                // Cache everything we need to fully re-auth after a DB
                // reconnect. JWT is preferred; email/password is the
                // fallback when the JWT has expired.
                let jwt_str = jwt.access.as_insecure_token().to_string();
                cache_auth(Some(jwt_str.clone()), Some(full_email.clone()), Some(password));

                let user: Option<User> = DATABASE.query("SELECT * FROM user WHERE id == $auth.id").await?.take(0)?;
                let users: Vec<User> = DATABASE.query("SELECT * FROM user WHERE active == true").await?.take(0)?;
                // let sess = DATABASE.query("RETURN <string>$session").await?.take::<Option<String>>(0)?;
                // log::info!("Session: {:?}", sess);
                if !users.is_empty() {
                    if let Ok(mut users_guard) = STORE_USERS.try_lock() {
                        *users_guard = users.clone(); 
                    }
                }

                if let Some(u) = user.clone() {
                    if cfg!(debug_assertions)  {
                        if cfg!(target_os = "windows") {
                            let bucket_url = format!("{}{}", BUCKET_DEV_WINDOWS_URL, u.get_user_bucket_name());
                            if let Err(e) = file_storage::define_bucket(&u.get_user_bucket_name(), &bucket_url).await {
                                log::warn!("Failed to define user bucket: {e}");
                            }
                        } else if cfg!(target_os = "linux") {
                            let bucket_url = format!("{}{}", BUCKET_DEV_LINUX_URL, u.get_user_bucket_name());
                            if let Err(e) = file_storage::define_bucket(&u.get_user_bucket_name(), &bucket_url).await {
                                log::warn!("Failed to define user bucket: {e}");
                            }
                        }
                    } else {
                        let bucket_url = format!("{}{}", BUCKET_URL, u.get_user_bucket_name());
                        if let Err(e) = file_storage::define_bucket(&u.get_user_bucket_name(), &bucket_url).await {
                            log::warn!("Failed to define user bucket: {e}");
                        }
                    }
                
                    // lock only after await
                    if let Ok(mut user_info_guard) = CURRENT_USER_INFO.try_lock() {
                        *user_info_guard = Some(u);
                    }
                }

                Ok( Self { jwt: Some(jwt_str), user } )
            }
        }
    }

    pub async fn signup<T: Serialize + Debug + Clone + SurrealValue>(
        signup: T,
        email: String,
    ) -> anyhow::Result<Self, anyhow::Error> {
        if cfg!(debug_assertions) {
            let try_local = DATABASE.connect::<surrealdb::engine::remote::ws::Ws>(DB_URL_LOCAL).await;
            log::info!("Attempting to connect to local DB: {try_local:?}");
        } else {
            match DATABASE.connect::<surrealdb::engine::remote::ws::Wss>(DB_URL_DEV).await {
                Ok(_) => log::info!("Connected to {DB_URL_DEV:?}"),
                Err(e) => {
                    let try_local = DATABASE.connect::<surrealdb::engine::remote::ws::Ws>(DB_URL_LOCAL).await;
                    log::error!("Failed connecting to: {DB_URL_DEV:?}\n{e:?}\nattempting to connect to local DB: {try_local:?}");
                },
            }
        }
        // let _ = DATABASE.connect::<surrealdb::engine::remote::ws::Ws>(DB_URL_LOCAL).await;
        match DATABASE.use_ns(NS).use_db(DB).await {
            Ok(_) => log::info!("Using NS: {NS:?}\nUsing DB: {DB:?}"),
            Err(e) => log::error!("Failed Using NS: {NS:?}\nFailed Using DB: {DB:?}\nE: {e:?}"),
        }

        if cfg!(debug_assertions) {
            info!("signup: {signup:?}");
        }

        // Select a specific namespace / database
        let jwt = DATABASE
            .signup(SurrealRec {
                namespace: NS.to_string(),
                database: DB.to_string(),
                access: USER_SCOPE.to_string(),
                params: signup.clone(),
            })
            .await;

        match jwt {
            Ok(j) => {
                let query = "SELECT * FROM user WHERE email == $email";
                DATABASE.set("email", email).await?;
                let user: Option<User> = DATABASE.query(query).await?.take(0)?;
                Ok(Self {
                    jwt: Some(j.access.as_insecure_token().to_string()),
                    user,
                })
            },
            Err(e) => {
                log::error!("Error signing up: {e:?}");
                return Err(anyhow::anyhow!("Error signing up: {e:?}"));
            },
        }
    }
}

/// Force-rebuild the SurrealDB websocket and re-authenticate with the cached
/// JWT. Used by the WASM client after a Cloudflare-Tunnel-driven WS reset.
///
/// `DATABASE` is a `Lazy<Surreal<WsClient>>` singleton, so after a transport
/// drop the SDK can end up in a state where `.connect()` returns
/// `"Already connected"` even though the underlying socket is dead. Calling
/// `invalidate()` first clears that stale state so the subsequent `.connect()`
/// builds a fresh socket.
pub async fn reconnect_with_jwt(jwt: Option<String>) -> anyhow::Result<()> {
    let jwt = jwt.ok_or_else(|| anyhow::anyhow!("reconnect_with_jwt: no JWT cached"))?;
    let _ = DATABASE.invalidate().await;
    if cfg!(debug_assertions) {
        DATABASE.connect::<Ws>(DB_URL_LOCAL).await?;
    } else {
        DATABASE.connect::<Wss>(DB_URL_DEV).await?;
    }
    DATABASE.use_ns(NS).use_db(DB).await?;
    DATABASE.authenticate(jwt.clone()).await?;
    cache_auth(Some(jwt), None, None);
    log::info!("reconnect_with_jwt: WS reconnected and re-authenticated");
    Ok(())
}

pub fn get_current_user_from_auth() -> Option<User> {
    if let Ok(current_user) = CURRENT_USER_INFO.try_lock() {
        log::trace!("get_current_user_from_auth: user retrieved from global state");
        current_user.clone()
    } else {
        log::warn!("get_current_user_from_auth: failed to acquire lock");
        None
    }
}

pub fn get_database_users() -> Vec<User> {
    if let Ok(users) = STORE_USERS.try_lock() {
        log::trace!("get_database_users: users retrieved from global state");
        users.clone()
    } else {
        log::warn!("get_database_users: failed to acquire lock");
        vec![]
    }
}

pub async fn init_database() -> anyhow::Result<(), anyhow::Error> {
    if cfg!(debug_assertions) {
        let try_local = DATABASE.connect::<surrealdb::engine::remote::ws::Ws>(DB_URL_LOCAL).await;
        log::info!("Attempting to connect to local DB: {try_local:?}");
    } else {
        match DATABASE.connect::<surrealdb::engine::remote::ws::Wss>(DB_URL_DEV).await {
            Ok(_) => log::info!("Connected to {DB_URL_DEV:?}"),
            Err(e) => log::error!("Failed connecting to: {DB_URL_DEV:?}\n{e:?}"),
        }
    }
    DATABASE.use_ns(NS).use_db(DB).await?;

    DATABASE.signin(SurrealRec {
        namespace: NS.to_string(),
        database: DB.to_string(),
        access: "guest".to_string(),
        params: Credentials {
            username: "guest".to_string(),
            password: SURREAL_GUEST_PASSWORD.to_string()
        }
    }).await?;

    Ok(())
}

/// Async sleep available on both native (tokio) and wasm (gloo) targets.
pub async fn sleep_compat(dur: std::time::Duration) {
    #[cfg(not(target_arch = "wasm32"))]
    tokio::time::sleep(dur).await;
    #[cfg(target_arch = "wasm32")]
    gloo_timers::future::TimeoutFuture::new(dur.as_millis().min(u32::MAX as u128) as u32).await;
}

/// Races `fut` against a deadline; `Err` on timeout.
pub async fn with_timeout<T>(
    dur: std::time::Duration,
    fut: impl std::future::IntoFuture<Output = T>,
) -> anyhow::Result<T> {
    use futures::future::{select, Either};
    let fut = fut.into_future();
    let sleep = sleep_compat(dur);
    futures::pin_mut!(fut);
    futures::pin_mut!(sleep);
    match select(fut, sleep).await {
        Either::Left((v, _)) => Ok(v),
        Either::Right(_) => Err(anyhow::anyhow!("operation timed out after {dur:?}")),
    }
}

/// Random id distinguishing one app instance's live-query canaries from
/// other sessions of the same user.
pub fn new_live_session_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// Record id bound to `$auth` on the current connection; `None` when
/// unauthenticated or signed in as something other than a record user.
pub async fn current_auth_id() -> anyhow::Result<Option<schema::RecordId>> {
    let mut response = with_timeout(
        std::time::Duration::from_secs(10),
        DATABASE.query("RETURN $auth.id"),
    )
    .await??;
    Ok(response.take::<Option<schema::RecordId>>(0)?)
}

/// Check if the database connection is alive by running a simple query
/// Returns true if connected, false if connection is dead
pub async fn is_db_connected() -> bool {
    // A dead websocket black-holes queries; bound the probe so the check can fail.
    match with_timeout(
        std::time::Duration::from_secs(3),
        DATABASE.query("RETURN true"),
    )
    .await
    {
        Ok(Ok(mut response)) => {
            let result: Option<bool> = response.take(0).unwrap_or(None);
            result.unwrap_or(false)
        }
        Ok(Err(e)) => {
            log::warn!("Database connection check failed: {}", e);
            false
        }
        Err(_) => {
            log::warn!("Database connection check timed out after 3s; treating connection as dead");
            false
        }
    }
}

/// Waits (bounded) for the SurrealDB SDK's internal auto-reconnect to
/// restore the websocket, polling `is_db_connected` with backoff. Returns
/// whether the socket had actually dropped (`true`) vs was already healthy
/// (`false`).
///
/// The SDK owns the socket: its `router` is a process-lifetime `OnceLock`,
/// so `DATABASE.connect()` on the singleton only ever returns
/// `Err(AlreadyConnected)`. On a drop the SDK's own `router_reconnect` loop
/// rebuilds the socket every ~1s and replays `Signin`/`Authenticate`/`Use`.
/// The app's job is only to wait for that, then re-issue its LIVE queries
/// (which the SDK does not replay).
pub async fn await_db_socket() -> anyhow::Result<bool> {
    if is_db_connected().await {
        return Ok(false);
    }
    log::warn!("Database socket down; waiting for SDK auto-reconnect");
    for attempt in 0..8u32 {
        let delay = std::time::Duration::from_millis((250u64 << attempt.min(5)).min(8000));
        sleep_compat(delay).await;
        if is_db_connected().await {
            log::info!("Database socket recovered after {} probe(s)", attempt + 1);
            return Ok(true);
        }
    }
    Err(anyhow::anyhow!("database socket did not recover within budget"))
}

/// Confirms `$auth` is `expected`; if not (the SDK replayed an expired token
/// or the session dropped to guest), replays the cached JWT then the cached
/// email+password. Returns whether `$auth` ended up as `expected`. Never
/// falls back to guest — a genuinely unrecoverable auth is surfaced to the
/// caller so it can route the operator back to login.
pub async fn restore_auth_if_needed(expected: schema::RecordId) -> anyhow::Result<bool> {
    if current_auth_id().await? == Some(expected.clone()) {
        return Ok(true);
    }
    let cached: Option<CachedAuth> = CACHED_AUTH.try_lock().ok().and_then(|g| g.clone());

    if let Some(CachedAuth { jwt: Some(token), .. }) = cached.as_ref() {
        if DATABASE.authenticate(token.clone()).await.is_ok()
            && current_auth_id().await? == Some(expected.clone())
        {
            log::info!("Restored auth via cached JWT after reconnect");
            return Ok(true);
        }
    }

    if let Some(CachedAuth { email: Some(em), password: Some(pw), .. }) = cached.as_ref() {
        let creds = SurrealRec {
            namespace: NS.to_string(),
            database: DB.to_string(),
            access: USER_SCOPE.to_string(),
            params: Auth { email: em.clone(), password: pw.clone() },
        };
        match DATABASE.signin(creds).await {
            Ok(jwt) => {
                cache_auth(Some(jwt.access.as_insecure_token().to_string()), None, None);
                if current_auth_id().await? == Some(expected) {
                    log::info!("Restored auth via cached credentials after reconnect");
                    return Ok(true);
                }
            }
            Err(e) => log::warn!("Cached credential re-signin failed: {e}"),
        }
    }

    Ok(false)
}

/// Ensure the DB socket is usable, waiting for the SDK's auto-reconnect.
/// Auth is not touched here (the SDK replays it); callers needing a
/// *specific* identity restored use [`restore_auth_if_needed`].
pub async fn ensure_db_connected() -> anyhow::Result<(), anyhow::Error> {
    await_db_socket().await.map(|_| ())
}

/// [`ensure_db_connected`] with a hard deadline so a black-holed websocket
/// can never wedge callers waiting on the result.
pub async fn ensure_db_connected_bounded() -> anyhow::Result<(), anyhow::Error> {
    with_timeout(std::time::Duration::from_secs(40), ensure_db_connected()).await?
}

/// Retry a DB operation across a transient connection blip.
///
/// Use this for calls that run during a live admin↔agent TCP session
/// (anything reached from `Mastertech4.0/src/terminal_mode/websockets.rs`
/// `TerminalWebsocketClient::handle_command`, or
/// `Mastertech4.0/src/tabs/websockets/mod.rs` handlers): they must not
/// kill the session on a single 30-second DB blip. Read-side calls
/// (idempotent) and append-style writes are safe to wrap; non-idempotent
/// updates that *don't* check current state via `WHERE` should be wrapped
/// only after their callers tolerate at-least-once semantics.
///
/// Walks the standard 250 ms → 500 ms → 1 s back-off, calling
/// [`ensure_db_connected`] between attempts so a dropped websocket gets
/// re-established before the next query runs. After three connection
/// errors in a row the final attempt is forwarded as-is, so callers see
/// the real underlying error if the DB truly never comes back.
///
/// Only available on native targets — uses `tokio::time::sleep` for
/// the back-off, which is not available in the WASM build.
#[cfg(not(target_arch = "wasm32"))]
pub async fn db_call_with_retry<F, Fut, T>(mut f: F) -> anyhow::Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<T>>,
{
    for attempt in 0..3u32 {
        match f().await {
            Ok(v) => return Ok(v),
            Err(e) if is_connection_error(&e) => {
                log::warn!(
                    "db_call_with_retry -> attempt {} failed with connection error ({e}); reconnecting",
                    attempt + 1
                );
                let _ = ensure_db_connected().await;
                tokio::time::sleep(std::time::Duration::from_millis(250u64 << attempt)).await;
            }
            Err(e) => return Err(e),
        }
    }
    f().await
}

/// Check if an error is a connection-related error
pub fn is_connection_error(error: &anyhow::Error) -> bool {
    let error_str = error.to_string();
    error_str.contains("uninitialised") || 
    error_str.contains("Connection") ||
    error_str.contains("WebSocket") ||
    error_str.contains("disconnected")
}

/// Macro-free way to retry a database operation with reconnect
/// 
/// Call this before making database queries to ensure connection is alive.
/// If the connection was dead and reconnection succeeded, returns true.
/// If connection was already alive, returns false.
/// If reconnection failed, returns an error.
pub async fn ensure_connected_or_reconnect() -> anyhow::Result<bool, anyhow::Error> {
    if is_db_connected().await {
        return Ok(false); // Already connected
    }
    
    log::warn!("Database connection lost, attempting to reconnect...");
    ensure_db_connected().await?;
    Ok(true) // Reconnected
}

/// Debug test for WASM time issues - call this to narrow down which DB operation fails
pub async fn test_database_wasm() -> anyhow::Result<String, anyhow::Error> {
    let mut results = String::new();
    
    // Step 1: Connect
    results.push_str("[Step 1] Attempting to connect...\n");
    log::info!("[test_database_wasm] Step 1: Connecting to database");
    
    match init_database().await {
        Ok(_) => {
            results.push_str("[Step 1] ✓ Connected to database\n");
            log::info!("[test_database_wasm] Step 1: Connected successfully");
        }
        Err(e) => {
            results.push_str(&format!("[Step 1] ✗ Failed to connect: {:?}\n", e));
            log::error!("[test_database_wasm] Step 1 FAILED: {:?}", e);
            return Ok(results);
        }
    }
    
    // Step 2: Use namespace/database
    results.push_str("[Step 2] Setting namespace and database...\n");
    log::info!("[test_database_wasm] Step 2: Setting NS={} DB={}", NS, DB);
    
    match DATABASE.use_ns(NS).use_db(DB).await {
        Ok(_) => {
            results.push_str(&format!("[Step 2] ✓ Using NS={} DB={}\n", NS, DB));
            log::info!("[test_database_wasm] Step 2: NS/DB set successfully");
        }
        Err(e) => {
            results.push_str(&format!("[Step 2] ✗ Failed to set NS/DB: {:?}\n", e));
            log::error!("[test_database_wasm] Step 2 FAILED: {:?}", e);
            return Ok(results);
        }
    }
    
    // Step 3: Sign in
    results.push_str("[Step 3] Signing in as guest...\n");
    log::info!("[test_database_wasm] Step 3: Signing in as guest");
    
    match DATABASE.signin(SurrealRec {
        namespace: NS.to_string(),
        database: DB.to_string(),
        access: "guest".to_string(),
        params: Credentials {
            username: "guest".to_string(),
            password: SURREAL_GUEST_PASSWORD.to_string()
        }
    }).await {
        Ok(_) => {
            results.push_str("[Step 3] ✓ Signed in successfully\n");
            log::info!("[test_database_wasm] Step 3: Signed in successfully");
        }
        Err(e) => {
            results.push_str(&format!("[Step 3] ✗ Failed to sign in: {:?}\n", e));
            log::error!("[test_database_wasm] Step 3 FAILED: {:?}", e);
            return Ok(results);
        }
    }
    
    // Step 4: Run a simple query
    results.push_str("[Step 4] Running query: RETURN $auth...\n");
    log::info!("[test_database_wasm] Step 4: Running RETURN $auth query");
    
    match DATABASE.query("RETURN $auth").await {
        Ok(mut response) => {
            match response.take::<Option<serde_json::Value>>(0) {
                Ok(auth_value) => {
                    results.push_str(&format!("[Step 4] ✓ Query result: {:?}\n", auth_value));
                    log::info!("[test_database_wasm] Step 4: Query successful: {:?}", auth_value);
                }
                Err(e) => {
                    results.push_str(&format!("[Step 4] ✗ Failed to take result: {:?}\n", e));
                    log::error!("[test_database_wasm] Step 4 take FAILED: {:?}", e);
                }
            }
        }
        Err(e) => {
            results.push_str(&format!("[Step 4] ✗ Query failed: {:?}\n", e));
            log::error!("[test_database_wasm] Step 4 FAILED: {:?}", e);
            return Ok(results);
        }
    }
    
    // Step 5: Run another query to test time-related operations
    results.push_str("[Step 5] Running query: RETURN time::now()...\n");
    log::info!("[test_database_wasm] Step 5: Running time::now() query");
    
    match DATABASE.query("RETURN time::now()").await {
        Ok(mut response) => {
            match response.take::<Option<serde_json::Value>>(0) {
                Ok(time_value) => {
                    results.push_str(&format!("[Step 5] ✓ Time query result: {:?}\n", time_value));
                    log::info!("[test_database_wasm] Step 5: Time query successful: {:?}", time_value);
                }
                Err(e) => {
                    results.push_str(&format!("[Step 5] ✗ Failed to take time result: {:?}\n", e));
                    log::error!("[test_database_wasm] Step 5 take FAILED: {:?}", e);
                }
            }
        }
        Err(e) => {
            results.push_str(&format!("[Step 5] ✗ Time query failed: {:?}\n", e));
            log::error!("[test_database_wasm] Step 5 FAILED: {:?}", e);
        }
    }
    
    results.push_str("\n[Complete] All steps finished.\n");
    log::info!("[test_database_wasm] Test complete");
    
    Ok(results)
}

pub async fn create_guest_notification(notification: Notification) -> anyhow::Result<(), anyhow::Error> {
    let try_guest = init_database().await;
    match try_guest {
        Ok(_) => {
            DATABASE.create::<Option<Notification>>(NOTIFICATION_TABLE)
            .content(notification)
            .await?;
        },
        Err(e) => log::error!("Couldnt create notification from guest user account either.. {e:?}"),
    }
    Ok(())
}

#[derive(serde::Serialize, SurrealValue)]
struct Credentials {
    username: String,
    password: String,
}

pub async fn login(email: String, password: String) -> anyhow::Result<Session> {
    let jwt = DATABASE
        .signin(SurrealRec {
            namespace: NS.to_string(),
            database: DB.to_string(),
            access: USER_SCOPE.to_string(),
            params: Auth { email, password },
        })
        .await?;

    let user: User = DATABASE
        .query("SELECT * FROM user WHERE id == $auth.id")
        .await?
        .take::<Option<User>>(0)?
        .ok_or_else(|| anyhow::anyhow!("User not found"))?;

    Ok(Session { jwt: jwt.access.as_insecure_token().to_string(), user })
}

pub async fn signup<T: Serialize + SurrealValue>(signup_data: T) -> anyhow::Result<Session> {
    let jwt = DATABASE
        .signup(SurrealRec {
            namespace: NS.to_string(),
            database: DB.to_string(),
            access: USER_SCOPE.to_string(),
            params: signup_data,
        })
        .await?;

    let user: User = DATABASE
        .query("SELECT * FROM user WHERE id == $auth.id")
        .await?
        .take::<Option<User>>(0)?
        .ok_or_else(|| anyhow::anyhow!("User not found"))?;

    Ok(Session { jwt: jwt.access.as_insecure_token().to_string(), user })
}

pub async fn token_login(jwt: &str) -> anyhow::Result<Session> {
    DATABASE.authenticate(jwt).await?;
    let user: User = DATABASE
        .query("SELECT * FROM user WHERE id == $auth.id")
        .await?
        .take::<Option<User>>(0)?
        .ok_or_else(|| anyhow::anyhow!("User not found"))?;

    Ok(Session { jwt: jwt.into(), user })
}


pub trait Spawner {
    #[cfg(not(target_arch = "wasm32"))]
    fn spawn<F>(future: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static;

    #[cfg(target_arch = "wasm32")]
    fn spawn<F>(future: F)
    where
        F: std::future::Future<Output = ()> + 'static;
}

#[cfg(target_arch = "wasm32")]
mod platform {
    use super::Spawner;
    use wasm_bindgen_futures::spawn_local;

    pub struct PlatformSpawner;

    impl Spawner for PlatformSpawner {
        fn spawn<F>(future: F)
        where
            F: std::future::Future<Output = ()> + 'static,
        {
            spawn_local(future);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod platform {
    use super::Spawner;
    use tokio::task;

    pub struct PlatformSpawner;

    impl Spawner for PlatformSpawner {
        fn spawn<F>(future: F)
        where
            F: std::future::Future<Output = ()> 
                + 'static 
                + std::marker::Send,
                
        {
            task::spawn(future);
        }
    }
}
