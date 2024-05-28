
use database::{schema::*, Database};
use wasm_bindgen_futures::spawn_local;
use log::{info, error};
use crossbeam::channel::Sender;


pub fn get_my_tasks(db: Database, tx: Sender<Vec<TaskPayload>>, user_id: UserId)
{
    spawn_local(async move {
        info!("getting tasks");
        let query = format!(
            "SELECT * FROM task WHERE assignee == {} ", user_id.0
        );
        let query_results: Result<Vec<TaskPayload>, surrealdb::Error> = db.database.query(query).await.unwrap().take(0);
        
        match query_results{
            Ok(data) => {
                match tx.send(data){
                    Ok(_) => drop(tx),
                    Err(e) => info!("Error sending Task Data: {e:?}")
                }
            },
            Err(e) => info!("Error unwrapping data: {e:?}"),
        }

    });
}

pub fn get_store_tasks(db: Database, tx: Sender<Vec<TaskPayload>>, store: Store)
{
    spawn_local(async move {
        let query = format!(
            "SELECT * FROM task \
            WHERE dep == '{store:?}'"
        );
        let query_results: Result<Vec<TaskPayload>, surrealdb::Error> = db.database.query(query).await.unwrap().take(0);
        match query_results{
            Ok(data) => {
                match tx.send(data){
                    Ok(_) => drop(tx),
                    Err(e) => error!("Error sending Task Data: {e:?}")
                }
            },
            Err(e) => error!("Error unwrapping data: {e:?}"),
        }
    });
}

pub fn get_completed_tasks(db: Database, tx: Sender<Vec<TaskPayload>>, store: Store)
{
    spawn_local(async move {
        let query = format!(
            "SELECT * FROM task \
            WHERE dep == '{store:?}' && task.completed == true"
        );
        let query_results: Result<Vec<TaskPayload>, surrealdb::Error> = db.database.query(query).await.unwrap().take(0);
        info!("get_completed_tasks: {query_results:?}");
        match query_results{
            Ok(data) => {
                match tx.send(data){
                    Ok(_) => drop(tx),
                    Err(e) => error!("Error sending Task Data: {e:?}")
                }
            },
            Err(e) => error!("Error unwrapping data: {e:?}"),
        }
    });
}


