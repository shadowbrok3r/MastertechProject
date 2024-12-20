use database::{schema::TaskPayload, Database, DATABASE};
use gloo_worker::{HandlerId, WorkerScope};
use wasm_bindgen_futures::spawn_local;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use gloo_console::log;

use gloo_worker::Registrable;
fn main() {
    WebWorker::registrar().register();
}

#[derive(Debug)]
pub struct Message(pub u32);

#[derive(Debug, Serialize, Deserialize)]
pub struct Input(pub String);

#[derive(Debug, Serialize, Deserialize)]
pub struct Output {
    pub tasks: Vec<TaskPayload>,
}

pub struct WebWorker;

impl gloo_worker::Worker for WebWorker {
    type Message = Message;
    type Input = Input;
    type Output = Output;

    fn create(_scope: &WorkerScope<Self>) -> Self {
        log!("create");
        Self { }
    }

    fn update(&mut self, _scope: &WorkerScope<Self>, msg: Self::Message) {
        log!(format!("update {:?}", msg));
    }

    fn received(
        &mut self,
        scope: &WorkerScope<Self>,
        msg: Self::Input,
        id: HandlerId,
    ) {
        log!(format!("received {msg:?}"));
        let scope = scope.clone();
        spawn_local(async move {
            get_completed_tasks(msg, scope, id).await.unwrap();
        });
    }
}



pub async fn get_completed_tasks(input: Input, scope: WorkerScope<WebWorker>, id: HandlerId) -> anyhow::Result<(), anyhow::Error> {
    gloo_console::debug!("get_completed_tasks");
    let _ = Database::new(
        "".to_string(), 
        "".to_string(), 
        Some(input.0.clone())
    ).await?;

    let tasks = get_completed_tasks_for_store().await?;

    scope.respond(id, Output { tasks });

    Ok(())
}


pub async fn get_completed_tasks_for_store() -> anyhow::Result<Vec<TaskPayload>, anyhow::Error> {
    let query = r#"
        SELECT *, (
            SELECT * FROM task_note 
                WHERE task_id == $parent.id
        ) AS task_note 
        FROM task 
        WHERE $this.assignee.store == $auth.store AND $this.completed IS true
        FETCH 
            service_ticket, 
            service_ticket.computer, 
            service_ticket.customer
            PARALLEL
    "#;
    
    let start_query = web_time::Instant::now(); // Start timing the query

    let query_results: Vec<TaskPayload> = DATABASE
        .query(query)
        .await?
        .take(0)?;

    let query_duration = start_query.elapsed(); // Measure query duration
    gloo_console::warn!(format!("Query execution time for completed tasks {query_duration:?}"));

    Ok(query_results)
}


// pub async fn get_stock(location: u64) -> anyhow::Result<Vec<RawStockData>, anyhow::Error> {
//     let res: Option<StockData> = DATABASE
//         .query("RETURN fn::store_stock($location, 5000)")
//         .bind(("location", location))
//         .await?
//         .take(0)?;
//     Ok(res.unwrap_or_default())
// }

// pub async fn get_extra_stock_info() -> anyhow::Result<Vec<ExtraInventoryData>, anyhow::Error> {
//     let res: Vec<ExtraInventoryData> = DATABASE
//         .query("RETURN fn::get_stock_extra_info(5000)")
//         .await?
//         .take(0)?;
//     Ok(res)
// }