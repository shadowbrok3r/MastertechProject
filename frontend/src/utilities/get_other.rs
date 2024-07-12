use database::{schema::{ClientId, ConnectedClient, Store, User}, DATABASE};
use log::{debug, error, info};
use crossbeam::channel::Sender;
use wasm_bindgen_futures::spawn_local;


pub fn get_store_users(tx: Sender<Vec<User>>, store: Store) {
    spawn_local(async move {
        DATABASE.set("store", store).await.unwrap();
        let data: Vec<User> = DATABASE
            .query("SELECT name, store, everest_initials, id, email FROM user WHERE store == $store")
            .await
            .unwrap()
            .take(0)
            .unwrap();
        match tx.try_send(data){
            Ok(_) => info!("Sent Store User Info"),
            Err(e) => error!("Error sending Task Data: {e:?}")
        };
    });
}

pub async fn get_connected_clients(tx: Sender<Vec<ConnectedClient>>, user_id: User)
    -> anyhow::Result<(), anyhow::Error>
{
    DATABASE.set("id", user_id.id.0).await?;
    let query: Vec<ConnectedClient> = DATABASE.query("SELECT * FROM connected_client WHERE assigned_user == $id").await?.take(0)?;

    match tx.try_send(query){
        Ok(_) => info!("Sent connected clients"),
        Err(e) => debug!("Error sending connected_clients: {e:?}")
    };

    Ok(())
}

pub async fn disconnect_client(tx: Sender<Vec<ClientId>>, id: ClientId)
    -> anyhow::Result<(), anyhow::Error>
{
    DATABASE.set("id", id.0.id).await?;
    let query: Vec<ClientId> = DATABASE.update("UPDATE connected_client SET connected = false WHERE id == $id").await.unwrap();

    match tx.try_send(query){
        Ok(_) => info!("Sent connected clients"),
        Err(e) => debug!("Error sending connected_clients: {e:?}")
    };

    Ok(())
}

pub async fn modify_connected_client(tx: Sender<Vec<ConnectedClient>>, user_id: User)
    -> anyhow::Result<(), anyhow::Error>
{
    DATABASE.set("id", user_id.id.0).await?;
    let query: Vec<ConnectedClient> = DATABASE.query("SELECT * FROM connected_client WHERE assigned_user == $id").await?.take(0)?;

    match tx.try_send(query){
        Ok(_) => info!("Sent connected clients"),
        Err(e) => debug!("Error sending connected_clients: {e:?}")
    };

    Ok(())
}