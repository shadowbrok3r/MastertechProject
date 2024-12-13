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
pub struct Input(pub String, pub String);

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
            scope.respond(id, Self::Output {
                tasks: get_completed_tasks_for_store(msg).await.unwrap(),
            });
        });
    }
}



pub async fn get_completed_tasks_for_store(input: Input) -> anyhow::Result<Vec<TaskPayload>, anyhow::Error> {
    gloo_console::debug!("get_completed_tasks");
    let query = r#"
        SELECT *, (
            SELECT * FROM task_note 
                WHERE task_id == $parent.id
        ) AS task_note 
        FROM task 
        WHERE $this.assignee.store == 'RIV' AND $this.completed IS true
        FETCH 
            service_ticket, 
            service_ticket.computer, 
            service_ticket.customer
        PARALLEL
    "#;
    
    let x = Database::new(input.0, input.1, None).await?;
    gloo_console::warn!(format!("user {:?}", x.user));

    let start_query = web_time::Instant::now(); // Start timing the query
    gloo_console::warn!(format!("{start_query:?}"));

    let query_results: Vec<TaskPayload> = DATABASE
        .query(query)
        .await?
        .take(0)?;

    let query_duration = start_query.elapsed(); // Measure query duration
    gloo_console::warn!(format!("Query execution time {query_duration:?}\ntask len: {}", query_results.len()));

    Ok(query_results)
}
