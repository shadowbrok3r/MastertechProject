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

    let mut offset = 0;
    let limit = 2; // Number of tasks per chunk
    loop {

        let tasks = get_completed_tasks_for_store(offset, limit).await?;
        // Break the loop if no more results
        if tasks.is_empty() {
            break;
        }

        scope.respond(id, Output { tasks });
        
        // Update the offset for the next chunk
        offset += limit;
    }

    Ok(())
}


pub async fn get_completed_tasks_for_store(offset: i32, limit: i32) -> anyhow::Result<Vec<TaskPayload>, anyhow::Error> {
    let query = r#"
        SELECT *, (
            SELECT * FROM task_note 
                WHERE task_id == $parent.id
        ) AS task_note 
        FROM task 
        WHERE $this.assignee.store == $auth.store AND $this.completed IS true
        LIMIT $limit START $offset
        FETCH 
            service_ticket, 
            service_ticket.computer, 
            service_ticket.customer
    "#;
    
    let start_query = web_time::Instant::now(); // Start timing the query
    gloo_console::warn!(format!("Querying at offset: {offset}"));

    let query_results: Vec<TaskPayload> = DATABASE
        .query(query)
        // .bind(("store", store.clone()))
        .bind(("limit", limit))
        .bind(("offset", offset))
        .await?
        .take(0)?;

    let query_duration = start_query.elapsed(); // Measure query duration
    gloo_console::warn!(format!("Query execution time for chunk (offset: {offset}): {query_duration:?}\ntask len: {}", query_results.len()));

    Ok(query_results)
}