use crossbeam::channel::Sender;
use database::{schema::{ComputerData, CustomerData, User, TaskNotePayload, TaskPayload, TicketData, TicketId}, Database};
use log::{error, info};
use surrealdb::opt::RecordId;
use wasm_bindgen_futures::spawn_local;



pub trait TaskContext {
    fn get_store_users(&mut self, db: Database, tx: Sender<Vec<User>>);
    fn get_computer_data(&mut self, db: Database, tx: Sender<Vec<ComputerData>>);
    fn get_customer_data(&mut self, db: Database, tx: Sender<Vec<CustomerData>>);
    fn get_service_data(&mut self, db: Database, tx: Sender<Vec<TicketData>>);
    fn get_task_notes(&mut self, db: Database, tx: Sender<Vec<TaskNotePayload>>);
}


impl TaskContext for TaskPayload{
    fn get_store_users(&mut self, db: Database, tx: Sender<Vec<User>>){
        let id: RecordId = self.id.clone().unwrap().0;
        spawn_local(async move {
            let query = format!(
                "SELECT * FROM user WHERE id={id}"
            );
            let get_data: Result<Vec<User>, surrealdb::Error> = db
                .database
                .query(query)
                .await
                .unwrap()
                .take(0);
            info!("get_data: {get_data:#?}");

            if let Ok(data) = get_data{
                match tx.send(data){
                    Ok(_) => info!("Sent data"),
                    Err(e) => error!("Error sending data: {e:?}")
                };
            }
        });
        
    }
    fn get_computer_data(&mut self, db: Database, tx: Sender<Vec<ComputerData>>){
        let id: RecordId = self.id.clone().unwrap().0;
        spawn_local(async move {
            let query = format!(
                "SELECT service_ticket.computer FROM task WHERE id={id} FETCH service_ticket.computer"
            );
            let get_data: Result<Vec<ComputerData>, surrealdb::Error> = db
                .database
                .query(query)
                .await
                .unwrap()
                .take(0);
            info!("get_data: {get_data:#?}");

            if let Ok(data) = get_data{
                match tx.send(data){
                    Ok(_) => info!("Sent data"),
                    Err(e) => error!("Error sending data: {e:?}")
                };
            }
        });
        
    }
    fn get_customer_data(&mut self, db: Database, tx: Sender<Vec<CustomerData>>){
        let id: RecordId = self.id.clone().unwrap().0;
        spawn_local(async move {
            let query = format!(
                "SELECT service_ticket.customer FROM task WHERE id={id} FETCH service_ticket.customer"
            );
            let get_data: Result<Vec<CustomerData>, surrealdb::Error> = db
                .database
                .query(query)
                .await
                .unwrap()
                .take(0);
            info!("get_data: {get_data:#?}");

            if let Ok(data) = get_data{
                match tx.send(data){
                    Ok(_) => info!("Sent data"),
                    Err(e) => error!("Error sending data: {e:?}")
                };
            }
        });
        
    }
    fn get_service_data(&mut self, db: Database, tx: Sender<Vec<TicketData>>){
        let id: RecordId = self.service_ticket.clone().unwrap().clone().0;
        spawn_local(async move {
            let query = format!(
                "SELECT * FROM service_order WHERE id={id}"
            );
            let get_data: Result<Vec<TicketData>, surrealdb::Error> = db
                .database
                .query(query)
                .await
                .unwrap()
                .take(0);
            info!("get_data: {get_data:#?}");

            if let Ok(data) = get_data{
                match tx.send(data){
                    Ok(_) => info!("Sent data"),
                    Err(e) => error!("Error sending data: {e:?}")
                };
            }
        });
        
    }
    fn get_task_notes(&mut self, db: Database, tx: Sender<Vec<TaskNotePayload>>){
        let id: RecordId = self.id.clone().unwrap().0;
        spawn_local(async move {
            let query = format!(
                "SELECT * FROM task_note WHERE id={id}"
            );
            let get_data: Result<Vec<TaskNotePayload>, surrealdb::Error> = db
                .database
                .query(query)
                .await
                .unwrap()
                .take(0);
            info!("get_data: {get_data:#?}");

            if let Ok(data) = get_data{
                match tx.send(data){
                    Ok(_) => info!("Sent data"),
                    Err(e) => error!("Error sending data: {e:?}")
                };
            }
        });
        
    }
}