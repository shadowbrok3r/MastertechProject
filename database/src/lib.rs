pub mod schema;

use log::info;
use schema::User;
use serde::{Serialize, Deserialize, de::DeserializeOwned};
use serde_json::Value;
use surrealdb::{
    engine::remote::ws::{Client as WsClient, Ws, Wss}, opt::auth::{Jwt, Scope}, Error, Surreal // http::{Client as HttpClient, Https},
};
        

use self::schema::Record;

#[derive(Clone, Debug)]
pub struct Database{
    pub database: Surreal<WsClient>,
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
struct Auth {
    email: String,
    password: String,
}

impl Database{
    pub async fn new(username: String, password: String, jwt: Option<String>) -> anyhow::Result<Self, anyhow::Error> {
        let db_url = "surrealdb.shadowbroker.app".to_string(); 
        match jwt{
            Some(jwt) => {
                let database: Surreal<WsClient> = Surreal::new::<Wss>(db_url) // localhost:8000
                    .await?;

                info!("auth: {:?}", jwt.clone());
                let auth = database.authenticate(jwt.clone()).await;
                info!("auth: {:?}", auth);
                match auth{
                    Ok(_) => {
                        if !username.is_empty() || !password.is_empty(){
                            info!("username or pass is empty");
                            let query = format!(
                                "SELECT id, name, everest_initials, email, store FROM user WHERE email == '{}'", 
                                username
                            );
            
                            let user: Vec<Value> = database
                                .query(query)
                                .await?
                                .take(0).unwrap();
                    
                            let usr: User = serde_json::from_value(user.get(0).unwrap().clone())?;
                            info!("returning jwt, user, db: {:?}", jwt.clone());
                            Ok(Self { database, jwt: Some(jwt.into()), user: Some(usr) })
                        }else{
                            Ok(Self { database, jwt: Some(jwt.into()), user: None })
                        }
                    },
                    Err(e) => Err(e.into()),
                }

            },
            None => {
                
                let database: Surreal<WsClient> = Surreal::new::<Wss>(db_url) // localhost:8000
                    .await?;
        
                // Select a specific namespace / database
                let jwt = database.signin(
                    Scope {
                        namespace: "Mastertech",
                        database: "MastertechDB",
                        scope: "user",
                        params: Auth{email: username.clone(), password: password}
                    }
                ).await?;
                info!("jwt: {:?}", jwt.as_insecure_token());
                let query = format!(
                    "SELECT  id, name, everest_initials, email, store FROM user WHERE email == '{}'", 
                    username
                );
                info!("query {:?}", query);
                let user: Vec<Value> = database
                    .query(query)
                    .await?
                    .take(0)?;
                    
                info!("Response: {:?}", user);
                 
                

                let usr: User = serde_json::from_value(user.get(0).unwrap().clone())?;
                info!("result of user query: {:?}", usr);
                Ok(Self { database, jwt: Some(jwt), user: Some(usr) })
            },
        }
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