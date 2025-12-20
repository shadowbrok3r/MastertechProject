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

pub static DATABASE: Lazy<Surreal<WsClient>> = Lazy::new(Surreal::init);

const USER_SCOPE: &str = env!("USER_SCOPE");
const DB: &str = env!("DB");
const NS: &str = env!("NS");
pub const SCAFFOLD_USER: &str = env!("SCAFFOLD_USER");
pub const SCAFFOLD_PASS: &str = env!("SCAFFOLD_PASS");
pub const DB_URL: &str = env!("DB_URL");
pub const STORAGE_URL: &str = env!("STORAGE_URL");
pub const REGION: &str = env!("REGION");
pub const DB_URL_DEV: &str = env!("DB_URL_DEV");
pub const DB_URL_LOCAL: &str = env!("DB_URL_LOCAL");
pub const WS_CLIENT_URL_LOCAL: &str = env!("WS_CLIENT_URL_LOCAL");
pub const WS_MASTER_URL_LOCAL: &str = env!("WS_MASTER_URL_LOCAL");
pub const WS_CLIENT_URL: &str = env!("WS_CLIENT_URL");
pub const WS_MASTER_URL: &str = env!("WS_MASTER_URL");
pub const ISSUE_TOKEN: &str = env!("ISSUE_TOKEN");
pub const DOWNLOAD_TOKEN: &str = env!("DOWNLOAD_TOKEN");

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

        match jwt {
            Some(jwt) => {
                info!("Have a JWT, attempting token auth");
                DATABASE.authenticate(jwt.clone()).await?;
                let user: Option<User> = DATABASE.query("SELECT * FROM user WHERE id == $auth.id").await?.take(0)?;
                let users: Vec<User> = DATABASE.query("SELECT * FROM user WHERE active == true").await?.take(0)?;
                let sess = DATABASE.query("RETURN <string>$session").await?.take::<Option<String>>(0)?;
                log::info!("Session: {:?}", sess);
 
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
                info!("No JWT, sigining in: {:?}", email.clone());
                let full_email = if email.ends_with("@pclaptops.com") {
                    email.clone()
                } else {
                    format!("{}@pclaptops.com", email.clone())
                };
                // Select a specific namespace / database
                let jwt = DATABASE
                    .signin(SurrealRec {
                        namespace: NS.to_string(),
                        database: DB.to_string(),
                        access: USER_SCOPE.to_string(),
                        params: Auth { email: full_email, password },
                    })
                    .await?;

                let user: Option<User> = DATABASE.query("SELECT * FROM user WHERE id == $auth.id").await?.take(0)?;
                let users: Vec<User> = DATABASE.query("SELECT * FROM user WHERE active == true").await?.take(0)?;
                let sess = DATABASE.query("RETURN <string>$session").await?.take::<Option<String>>(0)?;
                log::info!("Session: {:?}", sess);
                if !users.is_empty() {
                    if let Ok(mut users_guard) = STORE_USERS.try_lock() {
                        *users_guard = users.clone(); 
                    }
                }

                if let Ok(mut user_info_guard) = CURRENT_USER_INFO.try_lock() {
                    // log::warn!("SET THE USER: {:?}", user_info_guard.clone());
                    *user_info_guard = user.clone(); // Set the user info
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
