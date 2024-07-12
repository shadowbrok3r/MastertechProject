use crossbeam::channel::Sender;
use database::{schema::TaskPayload, DATABASE};
use log::{error, info};
use serde::{Deserialize, Serialize};
use surrealdb::opt::RecordId;
use wasm_bindgen_futures::spawn_local;
use std::fmt::Debug;

use super::Task;


impl Task for TaskPayload{
    fn get_computer_data<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(&mut self, tx: Sender<Option<T>>){
        let id: RecordId = self.id.clone().unwrap().0;
        spawn_local(async move {
            let query = format!(
                "SELECT service_ticket.computer FROM task WHERE id={id} FETCH service_ticket.computer"
            );
            let get_data: Option<T> = DATABASE
                .query(query)
                .await
                .unwrap()
                .take(0).unwrap();
            info!("get_data: {get_data:#?}");

                match tx.try_send(get_data){
                    Ok(_) => info!("Sent data"),
                    Err(e) => error!("Error sending data: {e:?}")
                };
        });
        
    }
    fn get_customer_data<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(&mut self, tx: Sender<Option<T>>){
        let id: RecordId = self.id.clone().unwrap().0;
        spawn_local(async move {
            let query = format!(
                "SELECT service_ticket.customer FROM task WHERE id={id} FETCH service_ticket.customer"
            );
            let get_data: Option<T> = DATABASE
                .query(query)
                .await
                .unwrap()
                .take(0).unwrap();
            info!("get_data: {get_data:#?}");

                match tx.try_send(get_data){
                    Ok(_) => info!("Sent data"),
                    Err(e) => error!("Error sending data: {e:?}")
                };
        });
        
    }
    // fn get_service_data<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(&mut self, tx: Sender<Option<T>>){
    //     let id: RecordId = self.service_ticket.clone().unwrap().clone().0;
    //     spawn_local(async move {
    //         let query = format!(
    //             "SELECT * FROM service_order WHERE id={id}"
    //         );
    //         let get_data: Option<T> = db
    //             .database
    //             .query(query)
    //             .await
    //             .unwrap()
    //             .take(0).unwrap();
    //         info!("get_data: {get_data:#?}");

    //             match tx.try_send(get_data){
    //                 Ok(_) => info!("Sent data"),
    //                 Err(e) => error!("Error sending data: {e:?}")
    //             };
    //     });
        
    // }
    fn get_task_notes<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(&mut self, tx: Sender<Option<T>>){
        let id: RecordId = self.id.clone().unwrap().0;
        spawn_local(async move {
            let query = format!(
                "SELECT * FROM task_note WHERE id={id}"
            );
            let get_data: Option<T> = DATABASE
                .query(query)
                .await
                .unwrap()
                .take(0).unwrap();
            info!("get_data: {get_data:#?}");

                match tx.try_send(get_data){
                    Ok(_) => info!("Sent data"),
                    Err(e) => error!("Error sending data: {e:?}")
                };
        });
    }

    fn get_ticket_payload<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(&mut self, tx: Sender<Option<T>>){
        let id: RecordId = self.id.clone().unwrap().0;
        spawn_local(async move {
            
            let get_data: Option<T> = DATABASE
                .query(format!("SELECT service_ticket.*, service_ticket.customer.*, service_ticket.computer.* FROM task WHERE id={id}"))
                .await
                .unwrap()
                .take(0).unwrap();

            match tx.try_send(get_data){
                Ok(_) => info!("Sent data"),
                Err(e) => error!("Error sending data: {e:?}")
            };
        });
    }
}