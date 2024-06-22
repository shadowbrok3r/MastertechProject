pub mod schema;

use log::info;
use std::fmt::Debug;
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

const DB_URL: &str = "surreal.master-tech.app/rpc"; // surreal.master-tech.app/rpc // localhost:8000
const USER_SCOPE: &str = "user";
const DB: &str = "MastertechDB";
const NS: &str = "Mastertech";


impl Database{
    pub async fn new(username: String, password: String, jwt: Option<String>) -> anyhow::Result<Self, anyhow::Error> {
        match jwt{
            Some(jwt) => {
                info!("We already have a jwt, attempting token auth");
                let database: Surreal<WsClient> = Surreal::new::<Ws>(DB_URL).await?;
                let auth = database.authenticate(jwt.clone()).await;

                match auth{
                    Ok(_) => {
                        info!("Auth ok");
                        if !username.is_empty() || !password.is_empty(){
                            let query = format!("SELECT id, name, everest_initials, email, store FROM user WHERE email = $email");
                            database.set("email", username).await?;
                            let user: Vec<Value> = database.query(query).await?.take(0)?;
                            info!("user: {user:#?}");
                            let usr: User = serde_json::from_value(user.get(0).unwrap().clone())?;
                            Ok(Self { database, jwt: Some(jwt.into()), user: Some(usr) })
                        }else{
                            info!("Auth not ok");
                            Ok(Self { database, jwt: Some(jwt.into()), user: None })
                        }
                    },
                    Err(e) => Err(e.into()),
                }
            },
            None => {
                info!("connecting");
                let database: Surreal<WsClient> = Surreal::new::<Ws>(DB_URL).await?;
                info!("signing in");
                
                database.use_ns(NS).use_db(DB).await?;

                // Select a specific namespace / database
                let jwt = database.signin(
                    Scope { 
                        namespace: NS, 
                        database: DB, 
                        scope: USER_SCOPE, // access: "user"
                        params: 
                            Auth{
                                email: username.clone(), 
                                password: password
                            }
                    }
                ).await?;
                
                let query = format!("SELECT  id, name, everest_initials, email, store FROM user WHERE email = $email");
                database.set("email", username.clone().to_lowercase()).await?;
                info!("querying ");
                let user: Vec<Value> = database.query(query).await?.take(0)?;
                    
                let usr: User = serde_json::from_value(user.get(0).unwrap().clone())?;

                Ok(Self { database, jwt: Some(jwt), user: Some(usr) })
            },
        }
    }

    pub async fn signup<T: Serialize + Debug + Clone>(signup: T, email: String) -> anyhow::Result<Self, anyhow::Error> {
        let database: Surreal<WsClient> = Surreal::new::<Wss>(DB_URL).await?;
        // Select a specific namespace / database
        let jwt = database.signup(
            Scope { 
                namespace: NS, database: DB, scope: USER_SCOPE,
                params: signup.clone()
            }
        ).await?;

        info!("signup: {:?}", signup);
        let query = format!("SELECT  id, name, everest_initials, email, store FROM user WHERE email == $email");

        database.set("email", email).await?;

        let user: Vec<Value> = database.query(query).await?.take(0)?;
            
        let usr: User = serde_json::from_value(user.get(0).unwrap().clone())?;

        Ok(Self { database, jwt: Some(jwt), user: Some(usr) })
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