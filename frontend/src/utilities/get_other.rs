use database::{schema::{ConnectedClient, Store, User}, Database};
use log::{debug, error, info};
use crossbeam::channel::Sender;
use wasm_bindgen_futures::spawn_local;


pub fn get_store_users(db: Database, tx: Sender<Vec<User>>, store: Store) {
    spawn_local(async move {
        db.database.set("store", store).await.unwrap();
        let data: Vec<User> = db.database
            .query("SELECT name, store, everest_initials, id, email FROM user WHERE store == $store")
            .await
            .unwrap()
            .take(0)
            .unwrap();
        
        match tx.try_send(data){
            Ok(_) => info!("Sent Data from querying tasks"),
            Err(e) => error!("Error sending Task Data: {e:?}")
        };
    });
}

pub async fn get_connected_clients(db: Database, tx: Sender<Vec<ConnectedClient>>, user_id: User)
    -> anyhow::Result<(), anyhow::Error>
{
    db.database.set("id", user_id.id.0).await?;
    let query: Vec<ConnectedClient> = db.database.query("SELECT * FROM connected_client WHERE assigned_user == $id").await?.take(0)?;

    match tx.try_send(query){
        Ok(_) => info!("Sent connected clients"),
        Err(e) => debug!("Error sending connected_clients: {e:?}")
    };

    Ok(())
}

pub async fn modify_connected_client(db: Database, tx: Sender<Vec<ConnectedClient>>, user_id: User)
    -> anyhow::Result<(), anyhow::Error>
{
    db.database.set("id", user_id.id.0).await?;
    let query: Vec<ConnectedClient> = db.database.query("SELECT * FROM connected_client WHERE assigned_user == $id").await?.take(0)?;

    match tx.try_send(query){
        Ok(_) => info!("Sent connected clients"),
        Err(e) => debug!("Error sending connected_clients: {e:?}")
    };

    Ok(())
}