use surrealdb::{engine::remote::ws::{Client as WsClient, Wss}, opt::auth::{Jwt, Record as SurrealRec/*Scope*/}, Error, Surreal}; 
use serde::{Serialize, Deserialize, de::DeserializeOwned};
use lazy_static::lazy_static;
use once_cell::sync::Lazy;
use serde_json::Value;
use std::{fmt::Debug, sync::RwLock};
use schema::User;
use log::info;
use self::schema::Record;
pub mod schema;
pub mod live_data;

#[derive(Clone, Debug, Default)]
pub struct Database{
    // pub database: Surreal<WsClient>,
    pub jwt: Option<Jwt>,
    pub user: Option<User>
}
#[derive(Serialize, Deserialize)]
pub struct DataSuccess{
    success: bool
}

#[derive(Serialize, Deserialize)]
pub struct Data{
    import_path: Option<String>,
    export_path: Option<String>,
}

#[derive(Serialize)]
pub struct DataResult{
    pub result: Result<DataSuccess, Error>
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
    Beta
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
/*
    Trait epi::Storage

    pub trait Storage {
        fn get_string(&self, key: &str) -> Option<String>;
        fn set_string(&mut self, key: &str, value: String);
        fn flush(&mut self);
    }
*/

impl Database{
    pub async fn new(username: String, password: String, jwt: Option<String>) -> anyhow::Result<Self, anyhow::Error> {

        DATABASE.connect::<Wss>(DB_URL_DEV).await?; //(&get_db_url()).await?;
        DATABASE.use_ns(NS).use_db(DB).await?;

        match jwt{
            Some(jwt) => {
                info!("We already have a jwt, attempting token auth");
                let auth = DATABASE.authenticate(jwt.clone()).await;
                // Self::handle_auth(auth, jwt, username, password).await
                match auth{
                    Ok(_) => {
                        info!("Auth ok");
                        if !username.is_empty() || !password.is_empty(){
                            let query = "SELECT * FROM user WHERE email == $email";
                            DATABASE.set("email", username).await?;
                            let user: Vec<Value> = DATABASE.query(query).await?.take(0)?;
                            info!("user: {user:#?}");
                            let usr: User = serde_json::from_value(user.get(0).unwrap().clone())?;
                            Ok(Self { jwt: Some(jwt.into()), user: Some(usr) })
                        }else{
                            info!("Auth not ok");
                            Ok(Self { jwt: Some(jwt.into()), user: None })
                        }
                    },
                    Err(e) => Err(e.into()),
                }
            },
            None => {
                info!("connecting");
                // let database: Surreal<WsClient> = Surreal::new::<Wss>(db_url).await?;
                info!("signing in");
                // database.use_ns(NS).use_db(DB).await?;

                // Select a specific namespace / database
                let jwt = DATABASE.signin(
                    SurrealRec { 
                        namespace: NS, 
                        database: DB, 
                        access: USER_SCOPE, // access: "user"
                        params: 
                            Auth{
                                email: username.clone(), 
                                password
                            }
                    }
                ).await?;
                
                let query = "SELECT id, name, everest_initials, email, store, minio_access_key, minio_secret_key FROM user WHERE email == $email";
                DATABASE.set("email", username.clone().to_lowercase()).await?;
                let user: Vec<Value> = DATABASE.query(query).await?.take(0)?;
                info!("querying {:?}", user.clone());
                    
                let usr: User = serde_json::from_value(user.get(0).unwrap().clone())?;

                Ok(Self {jwt: Some(jwt), user: Some(usr) })
            },
        }
    }

    
    pub async fn signup<T: Serialize + Debug + Clone>(signup: T, email: String) -> anyhow::Result<Self, anyhow::Error> {
        // let db_url = get_db_url();
        // let database: Surreal<WsClient> = Surreal::new::<Wss>(db_url).await?;
        DATABASE.connect::<Wss>(DB_URL_DEV).await?; //(&get_db_url()).await?;(&db_url).await?;
        DATABASE.use_ns(NS).use_db(DB).await?;
        // Select a specific namespace / database
        let jwt = DATABASE.signup(
            SurrealRec { 
                namespace: NS, database: DB, access: USER_SCOPE,
                params: signup.clone()
            }
        ).await?;

        info!("signup: {:?}", signup);
        let query = "SELECT  id, name, everest_initials, email, store, minio_access_key, minio_secret_key FROM user WHERE email == $email";

        DATABASE.set("email", email).await?;

        let user: Vec<Value> = DATABASE.query(query).await?.take(0)?;
            
        let usr: User = serde_json::from_value(user.get(0).unwrap().clone())?;

        Ok(Self { jwt: Some(jwt), user: Some(usr) })
    }

    pub async fn insert<T: Serialize>(&self, table: &str, record: T) -> Result<Vec<Record>, Error> {
        let created: Vec<Record> = DATABASE
            .create(table)
            .content(record)
            .await?;
        Ok(created)
    }

    pub async fn select<T: DeserializeOwned>(&self, table: &str) -> Result<Vec<T>, Error> {
        let result: Vec<T> = DATABASE.select(table).await?;
        Ok(result)
    }
    
    pub async fn sql<T: DeserializeOwned>(&self, sql_query: &str) -> Result<Vec<T>, Error> {
        let query: Vec<T> = DATABASE
            .query(sql_query)
            .await?
            .take(0)?;

        Ok(query)
    }

    pub async fn delete(&self, table: &str, id: &str) -> Result<Option<Record>, Error> {
        let result: Option<Record> = DATABASE
            .delete((table, id))
            .await.unwrap();
        Ok(result)
    }
}