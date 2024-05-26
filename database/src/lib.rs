pub mod schema;

use schema::User;
use serde::{Serialize, Deserialize, de::DeserializeOwned};
use serde_json::Value;
use surrealdb::{
    engine::remote::ws::{Client as WsClient, Ws}, opt::auth::{Jwt, Scope}, Error, Surreal // http::{Client as HttpClient, Https},
};
        

use self::schema::Record;

#[derive(Clone)]
pub struct Database{
    pub database: Surreal<WsClient>,
    pub jwt: Jwt,
    pub user: User
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
struct Auth {
    email: String,
    password: String,
}

impl Database{
    pub async fn new(username: String, password: String) -> anyhow::Result<Self, anyhow::Error> {
        // dotenv::var("DB_URL").expect("No Env var for DB_URL");
        let db_url = "localhost:8000".to_string(); 
        // let root_user = dotenv::var("SURREAL_USER").expect("No Env var for SURREAL_USER");
        // let root_pass = dotenv::var("SURREAL_PASS").expect("No Env var for SURREAL_PASS");
        let database: Surreal<WsClient> = Surreal::new::<Ws>(db_url) // localhost:8000
            .await?;
            
        let auth = Auth{
            email: username.clone(),
            password: password,
        };

        // Select a specific namespace / database
        let jwt = database.signin(
            Scope {
                namespace: "Mastertech",
                database: "MastertechDB",
                scope: "user",
                params: auth
            }
        ).await?;

        let query = format!("SELECT id, name, everest_initials, email, store, notifications FROM user WHERE email = '{}'", username);
        log::info!("query: {query:?}");

        let user: Vec<Value> = database.query(query)
            .await?
            .take(0).unwrap();

        log::info!("User: {user:?}");

        let usr: User = serde_json::from_value(user.get(0).unwrap().clone())?;

        log::info!("User: {usr:?}");

        Ok(Self { database, jwt, user: usr })
    }

    pub async fn insert<T: Serialize>(&self, table: &str, record: T) -> Result<Vec<Record>, Error> {
        let created: Vec<Record> = self
            .database
            .create(table)
            .content(record)
            .await?;
        Ok(created)
    }

    pub async fn select<T: DeserializeOwned>(&self, table: &str) -> Result<Vec<T>, Error> {
        let result: Vec<T> = self.database.select(table).await?;
        Ok(result)
    }
    pub async fn sql<T: DeserializeOwned>(&self, sql_query: &str) -> Result<Vec<T>, Error> {
        let query: Vec<T> = self.database
            .query(sql_query)
            .await?
            .take(0)?;

        Ok(query)
    }

    pub async fn delete(&self, table: &str, id: &str) -> Result<Option<Record>, Error> {
        let result: Option<Record> = self.database
            .delete((table, id))
            .await.unwrap();
        Ok(result)
    }
}