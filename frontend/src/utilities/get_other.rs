use database::{schema::{User, Store}, Database};
use wasm_bindgen_futures::spawn_local;
use log::{info, error};
use crossbeam::channel::Sender;


pub fn get_store_users(db: Database, tx: Sender<Vec<User>>, store: Store)
{
    spawn_local(async move {
        let query = format!(
            "SELECT name, store, everest_initials, id FROM user WHERE store == '{store:?}'"
        );
        let query_results: Result<Vec<User>, surrealdb::Error> = db.database.query(query).await.unwrap().take(0);
        match query_results{
            Ok(data) => {
                match tx.send(data){
                    Ok(_) => info!("Sent Data from querying tasks"),
                    Err(e) => error!("Error sending Task Data: {e:?}")
                }
            },
            Err(e) => error!("Error unwrapping data: {e:?}"),
        }

    });
}

