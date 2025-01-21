use self::schema::Record;
use lazy_static::lazy_static;
use log::info;
use once_cell::sync::Lazy;
use schema::User;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{fmt::Debug, sync::RwLock};
use surrealdb::{
    engine::remote::ws::{Client as WsClient, Wss}, // {local::{SurrealKv, Db}, }
    opt::{
        auth::{Jwt, Record as SurrealRec},
        capabilities::Capabilities,
        Config,
    },
    Error, Surreal,
};
pub mod live_data;
pub mod schema;

#[derive(Clone, Debug, Default)]
pub struct Database {
    // pub database: Surreal<WsClient>,
    pub jwt: Option<Jwt>,
    pub user: Option<User>,
}
#[derive(Serialize, Deserialize)]
pub struct DataSuccess {
    success: bool,
}

#[derive(Serialize, Deserialize)]
pub struct Data {
    import_path: Option<String>,
    export_path: Option<String>,
}

#[derive(Serialize)]
pub struct DataResult {
    pub result: Result<DataSuccess, Error>,
}

#[derive(Serialize)]
pub struct Auth {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Default, PartialEq, Serialize, Clone)]
pub enum DatabaseSelection {
    #[default]
    Stable,
    Beta,
}

lazy_static! {
    pub static ref DB_SELECTION: RwLock<DatabaseSelection> = RwLock::new(DatabaseSelection::Beta);
}

const USER_SCOPE: &str = "user";
const DB: &str = "MastertechDB";
const NS: &str = "Mastertech";
pub const STORAGE_URL: &str = "https://storage-api.master-tech.app";
pub const DB_URL: &str = "surrealdb.master-tech.app"; // "";
pub const DB_URL_DEV: &str = "surrealdb-dev.master-tech.app";
pub const DB_URL_LOCAL: &str = "localhost:8000";
pub static DATABASE: Lazy<Surreal<WsClient>> = Lazy::new(Surreal::init);
pub const WS_CLIENT_URL: &str = "ws://localhost:8081/websocket?role=client";
pub const WS_MASTER_URL: &str = "ws://localhost:8081/websocket?role=master";
// pub const WS_CLIENT_URL: &str = "wss://sock.master-tech.app/websocket?role=client";
// pub const WS_MASTER_URL: &str = "wss://sock.master-tech.app/websocket?role=master";

pub fn set_db_selection(selection: DatabaseSelection) {
    let mut db_selection = DB_SELECTION.write().unwrap();
    *db_selection = selection;
}

pub fn get_db_url() -> String {
    let db_selection = DB_SELECTION.read();
    match *db_selection.unwrap() {
        DatabaseSelection::Stable => DB_URL.to_string(),
        DatabaseSelection::Beta => DB_URL_DEV.to_string(),
    }
}


impl Database {
    pub async fn new(
        email: String,
        password: String,
        jwt: Option<String>,
    ) -> anyhow::Result<Self, anyhow::Error> {
        DATABASE.connect::<Wss>(DB_URL_DEV).await?;
        DATABASE.use_ns(NS).use_db(DB).await?;

        match jwt {
            Some(jwt) => {
                info!("Have a JWT, attempting token auth");
                DATABASE.authenticate(jwt.clone()).await?;
                let user: Option<User> = DATABASE.query("SELECT * FROM user WHERE id == $auth.id").await?.take(0)?;
                // info!("Returned Auth: {user:?}");
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
                // info!("Returned Auth: {user:?}");
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
