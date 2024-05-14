use log::{debug, info};
use serde::{Serialize, Deserialize, de::DeserializeOwned};
use surrealdb::{
    engine::remote::ws::{Client as WsClient, Wss}, sql::Thing, Error, Surreal
    
};


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

#[derive(Debug, Deserialize, Serialize)]
pub struct Record {
    #[allow(dead_code)]
    pub id: Thing,
}

impl Database{
    pub async fn new() -> Self {
        let database: Surreal<WsClient> = Surreal::new::<Wss>("surreal.master-tech.app/rpc") // localhost:8000
            .await.unwrap();

        // Select a specific namespace / database
        database
            .use_ns("Mastertech")
            .use_db("MastertechDB")
            .await
            .expect("Could not use ns or db name");

        Database { database }
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
    pub async fn query<T: DeserializeOwned>(&self, sql_query: &str) -> Result<Vec<T>, Error> {
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


pub async fn handle_db_data<T: Serialize + DeserializeOwned + Clone>(database: Database, tx: crossbeam::channel::Sender<T>) 
    -> anyhow::Result<(), anyhow::Error>
{
    let task_data: Vec<T> = database.select("task").await?;
    for task_data in task_data.iter(){
        
        match tx.send(task_data.clone()){
            Ok(_) => info!("Sent db connection across thread"),
            Err(err) => debug!("Error sending db connection: {err:?}"),
        }
    }

    Ok(())
}

