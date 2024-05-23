pub mod schema;

use serde::{Serialize, Deserialize, de::DeserializeOwned};
use surrealdb::{
    Error, Surreal, 
    engine::remote::ws::{Client as WsClient, Ws} // http::{Client as HttpClient, Https},
};
        

use self::schema::Record;

#[derive(Clone)]
pub struct Database{
    pub database: Surreal<WsClient>
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

impl Database{
    pub async fn new() -> Self {
        let db_url = dotenv::var("DB_URL").expect("No Env var for DB_URL");
        println!("db: {db_url}");
        
        // let root_user = dotenv::var("SURREAL_USER").expect("No Env var for SURREAL_USER");
        // let root_pass = dotenv::var("SURREAL_PASS").expect("No Env var for SURREAL_PASS");
        // let capabilities = Capabilities::all();
        // let config = Config::default()
        //     .user(Root{username: root_user.as_str(),password: root_pass.as_str()}).capabilities(capabilities);
        // let database = Surreal::new::<RocksDb>(
        //     (db_path,config))
        //      .await
        //      .expect("Something wrong with database files");

        let database: Surreal<WsClient> = Surreal::new::<Ws>(db_url) // localhost:8000
            .await.unwrap();
        // Select a specific namespace / database
        database
            .use_ns("Mastertech")
            .use_db("MastertechDB")
            .await
            .expect("Could not use ns or db name");

        Self { database }
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