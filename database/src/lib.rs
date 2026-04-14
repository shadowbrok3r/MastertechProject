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
pub mod schema;

pub use platform::PlatformSpawner;

use crate::schema::{NOTIFICATION_TABLE, Notification, file_storage};

pub static DATABASE: Lazy<Surreal<WsClient>> = Lazy::new(Surreal::init);

const USER_SCOPE: &str = env!("USER_SCOPE");
const DB: &str = env!("DB");
const NS: &str = env!("NS");
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

pub const ISSUE_TOKEN: &str = env!("ISSUE_TOKEN");
pub const DOWNLOAD_TOKEN: &str = env!("DOWNLOAD_TOKEN");
pub const ODOO_API_KEY: &str = env!("ODOO_API_KEY");
pub const BUCKET_DEV_WINDOWS_URL: &str = "C:/SurrealBuckets/";
pub const BUCKET_DEV_LINUX_URL: &str = "/home/shadowbroker/Documents/SurrealKV/";
pub const BUCKET_URL: &str = "/SurrealBuckets";

// JWT token type - in v3.0 this is just a String
pub type Jwt = String;

// The static variable holding the currently logged-in user
// Wrapped in Mutex for safe interior mutability
// Wrapped in Lazy for easy static initialization
pub static CURRENT_USER_INFO: Lazy<std::sync::Mutex<Option<User>>> = Lazy::new(|| {
    std::sync::Mutex::new(None) // Initialize with no user logged in
});

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
                    params: Auth { email: full_email.clone(), password },
                };

                info!("No JWT, sigining in: {:?}\n{:?}\n{:?}\n{:?}\n", full_email, creds.namespace, creds.database, creds.access);

                // Select a specific namespace / database
                let jwt = DATABASE
                    .signin(creds)
                    .await?;

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

                Ok( Self { jwt: Some(jwt.access.as_insecure_token().to_string()), user } )
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
            password: "toor10!9".to_string()
        }
    }).await?;

    Ok(())
}

/// Check if the database connection is alive by running a simple query
/// Returns true if connected, false if connection is dead
pub async fn is_db_connected() -> bool {
    // Try a simple query to check connection health
    match DATABASE.query("RETURN true").await {
        Ok(mut response) => {
            let result: Option<bool> = response.take(0).unwrap_or(None);
            result.unwrap_or(false)
        }
        Err(e) => {
            log::warn!("Database connection check failed: {}", e);
            false
        }
    }
}

/// Ensure database connection is alive, attempt to reconnect if not
/// This is useful for operations that may fail due to dropped WebSocket connections
/// 
/// # Returns
/// - `Ok(())` if connection is alive or was successfully re-established
/// - `Err(...)` if reconnection failed
pub async fn ensure_db_connected() -> anyhow::Result<(), anyhow::Error> {
    // First, try a simple health check
    if is_db_connected().await {
        return Ok(());
    }
    
    log::warn!("Database connection lost, attempting to reconnect...");
    
    log::info!("About to query DB. DATABASE ptr: {:p}", &*DATABASE);
    // Connection is dead, try to reconnect
    // First invalidate the old connection
    let _ = DATABASE.invalidate().await;
    
    // Reconnect
    if cfg!(debug_assertions) {
        DATABASE.connect::<surrealdb::engine::remote::ws::Ws>(DB_URL_LOCAL).await?;
        log::info!("Reconnected to local DB: {}", DB_URL_LOCAL);
    } else {
        DATABASE.connect::<surrealdb::engine::remote::ws::Wss>(DB_URL_DEV).await?;
        log::info!("Reconnected to: {}", DB_URL_DEV);
    }
    
    // Re-select namespace and database
    DATABASE.use_ns(NS).use_db(DB).await?;
    
    // Check if we had a user (don't hold MutexGuard across await)
    let had_user = CURRENT_USER_INFO.try_lock().map(|g| g.is_some()).unwrap_or(false);
    
    if had_user {
        // We had a user, need to re-authenticate
        // For now, sign in as guest - the actual re-auth should happen through the app's login flow
        log::warn!("Re-authenticating as guest after reconnect. User should re-login for full access.");
        DATABASE.signin(SurrealRec {
            namespace: NS.to_string(),
            database: DB.to_string(),
            access: "guest".to_string(),
            params: Credentials {
                username: "guest".to_string(),
                password: "toor10!9".to_string()
            }
        }).await?;
    }
    
    log::info!("Database reconnection successful");
    Ok(())
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
            password: "toor10!9".to_string()
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
