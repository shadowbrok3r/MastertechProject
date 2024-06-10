
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
        let query = format!("SELECT * FROM task WHERE dep == $store && task.completed == true");
        db.database.set("store", store).await.unwrap();
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


pub fn get_tasks(db: Database, tx: Sender<Vec<TaskPayload>>){
    spawn_local(async move {

        let query = format!("SELECT * FROM task FETCH service_ticket, service_ticket.computer, service_ticket.customer, task_note");

        let query_results: Result<Vec<TaskPayload>, surrealdb::Error> = db
            .database
            .query(query)
            .await
            .unwrap()
            .take(0);

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


// pub fn find_task_by_id(&mut self, id: &String) -> Option<&mut TaskPayload> {
//     for task_layout in self.task_layouts.values_mut() {
//         if let Some(task) = task_layout.tasks.iter_mut().find(|task| task.id.as_ref().map(|t_id| t_id.0.id.to_string()) == Some(id.to_string())) {
//             return Some(task);
//         }
//     }
//     None
// }