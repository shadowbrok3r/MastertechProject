use surrealdb::{engine::remote::ws::{Client as WsClient, Wss}, opt::{auth::{Jwt, Record as SurrealRec}, capabilities::Capabilities, Config}, Error, Surreal};
use serde::{de::DeserializeOwned, Serialize};
use once_cell::sync::Lazy;
use self::schema::Record;
use std::fmt::Debug;
use schema::User;
use log::info;

pub mod live_data;
pub mod schema;

const USER_SCOPE: &str = "user";
const DB: &str = "MastertechDB";
const NS: &str = "Mastertech";
pub const STORAGE_URL: &str = "https://storage-api.master-tech.app";
pub const DB_URL: &str = "surrealdb.master-tech.app"; // "";
pub const DB_URL_DEV: &str = "surrealdb-dev.master-tech.app";
pub const DB_URL_LOCAL: &str = "localhost:8000";
pub static DATABASE: Lazy<Surreal<WsClient>> = Lazy::new(Surreal::init);
// pub const WS_CLIENT_URL: &str = "ws://localhost:8081/websocket?role=client";
// pub const WS_MASTER_URL: &str = "ws://localhost:8081/websocket?role=master";
pub const WS_CLIENT_URL: &str = "wss://socket.master-tech.app/websocket?role=client";
pub const WS_MASTER_URL: &str = "wss://socket.master-tech.app/websocket?role=master";


pub use platform::PlatformSpawner;
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



#[derive(Clone, Debug, Default)]
pub struct Database {
    pub jwt: Option<Jwt>,
    pub user: Option<User>,
}


#[derive(Serialize)]
pub struct Auth {
    pub email: String,
    pub password: String,
}

#[derive(Clone, Debug)]
pub struct Session {
    pub jwt: Jwt,
    pub user: User,
}

pub async fn initialize_db() -> anyhow::Result<()> {
    DATABASE.connect::<Wss>(DB_URL_DEV).await?;
    DATABASE.use_ns(NS).use_db(DB).await?;
    Ok(())
}

pub async fn login(email: String, password: String) -> anyhow::Result<Session> {
    let jwt = DATABASE
        .signin(SurrealRec {
            namespace: NS,
            database: DB,
            access: USER_SCOPE,
            params: Auth { email, password },
        })
        .await?;

    let user: User = DATABASE
        .query("SELECT * FROM user WHERE id == $auth.id")
        .await?
        .take::<Option<User>>(0)?
        .ok_or_else(|| anyhow::anyhow!("User not found"))?;

    Ok(Session { jwt, user })
}

pub async fn signup<T: Serialize>(signup_data: T) -> anyhow::Result<Session> {
    let jwt = DATABASE
        .signup(SurrealRec {
            namespace: NS,
            database: DB,
            access: USER_SCOPE,
            params: signup_data,
        })
        .await?;

    let user: User = DATABASE
        .query("SELECT * FROM user WHERE id == $auth.id")
        .await?
        .take::<Option<User>>(0)?
        .ok_or_else(|| anyhow::anyhow!("User not found"))?;

    Ok(Session { jwt, user })
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

impl Database {
    pub async fn new(
        email: String,
        password: String,
        jwt: Option<String>,
    ) -> anyhow::Result<Self, anyhow::Error> {
        match DATABASE.connect::<surrealdb::engine::remote::ws::Wss>(DB_URL_DEV).await {
            Ok(_) => log::info!("Connected to {DB_URL_DEV:?}"),
            Err(e) => {
                let try_local = DATABASE.connect::<surrealdb::engine::remote::ws::Ws>(DB_URL_LOCAL).await;
                log::info!("Failed connecting to: {DB_URL_DEV:?}\n{e:?}\nattempting to connect to local DB: {try_local:?}");
            },
        }
        // let _ = DATABASE.connect::<surrealdb::engine::remote::ws::Ws>(DB_URL_LOCAL).await;
        match DATABASE.use_ns(NS).use_db(DB).await {
            Ok(_) => log::info!("Using NS: {NS:?}\nUsing DB: {DB:?}"),
            Err(e) => log::info!("Failed Using NS: {NS:?}\nFailed Using DB: {DB:?}\nE: {e:?}"),
        }

        match jwt {
            Some(jwt) => {
                info!("Have a JWT, attempting token auth");
                DATABASE.authenticate(jwt.clone()).await?;
                let user: Option<User> = DATABASE.query("SELECT * FROM user WHERE id == $auth.id").await?.take(0)?;
                Ok( Self { jwt: Some(jwt.into()), user } )
            }
            None => {
                info!("No JWT, sigining in: {:?}", email.clone());

                // Select a specific namespace / database
                let jwt = DATABASE
                    .signin(SurrealRec {
                        namespace: NS,
                        database: DB,
                        access: USER_SCOPE,
                        params: Auth { email, password },
                    })
                    .await?;

                let user: Option<User> = DATABASE.query("SELECT * FROM user WHERE id == $auth.id").await?.take(0)?;
                Ok( Self { jwt: Some(jwt), user } )
            }
        }
    }

    pub async fn signup<T: Serialize + Debug + Clone>(
        signup: T,
        email: String,
    ) -> anyhow::Result<Self, anyhow::Error> {
        // let db_url = get_db_url();
        let cap = Capabilities::all();
        let config = Config::new().capabilities(cap);

        DATABASE.connect::<Wss>((DB_URL_DEV, config)).await?; //(&get_db_url()).await?;(&db_url).await?;
        DATABASE.use_ns(NS).use_db(DB).await?;
        // Select a specific namespace / database
        let jwt = DATABASE
            .signup(SurrealRec {
                namespace: NS,
                database: DB,
                access: USER_SCOPE,
                params: signup.clone(),
            })
            .await?;

        info!("signup: {signup:?}");
        let query = "SELECT * FROM user WHERE email == $email";
        DATABASE.set("email", email).await?;
        let user: Option<User> = DATABASE.query(query).await?.take(0)?;
        Ok(Self {
            jwt: Some(jwt),
            user,
        })
    }

    pub async fn insert<T: Serialize + 'static>(
        &self,
        table: &str,
        record: T,
    ) -> Result<Option<Record>, Error> {
        let created: Option<Record> = DATABASE.create(table).content(record).await?;
        Ok(created)
    }

    pub async fn select<T: DeserializeOwned>(&self, table: &str) -> Result<Vec<T>, Error> {
        let result: Vec<T> = DATABASE.select(table).await?;
        Ok(result)
    }

    pub async fn sql<T: DeserializeOwned>(&self, sql_query: &str) -> Result<Vec<T>, Error> {
        let query: Vec<T> = DATABASE.query(sql_query).await?.take(0)?;

        Ok(query)
    }

    pub async fn delete(&self, table: &str, id: &str) -> Result<Option<Record>, Error> {
        let result: Option<Record> = DATABASE.delete((table, id)).await.unwrap();
        Ok(result)
    }
}
