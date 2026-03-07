use anyhow::Context;
use database::{schema::LiveTaskPayload, Database, DATABASE};
use gloo_worker::{HandlerId, WorkerScope};
use wasm_bindgen_futures::spawn_local;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use gloo_console::log;

use gloo_worker::Registrable;
fn main() { WebWorker::registrar().register(); }

#[derive(Debug)]
pub struct Message(pub u32);

#[derive(Debug, Serialize, Deserialize)]
pub struct Input(pub String);

#[derive(Debug, Serialize, Deserialize)]
pub struct Output {
    pub tasks: Vec<u8>,
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
            let res = get_completed_tasks(msg, scope, id).await;
            gloo_console::info!(format!("Result: {res:?}"));
        });
    }
}

pub async fn get_completed_tasks(input: Input, scope: WorkerScope<WebWorker>, id: HandlerId) -> anyhow::Result<(), anyhow::Error> {
    gloo_console::debug!("get_completed_tasks");
    let _ = Database::new("".to_string(), "".to_string(), Some(input.0.clone())).await?;
    let tasks = get_completed_tasks_for_store().await?;
    scope.respond(id, Output { tasks: encode_task_payload(&tasks)? });
    Ok(())
}

pub fn encode_task_payload(message: &Vec<LiveTaskPayload>) -> anyhow::Result<Vec<u8>> {
    let bincoded = serde_json::to_vec(message)?;
    let compressed = zstd::encode_all(std::io::Cursor::new(&bincoded), 5).context("zstd")?;
    Ok(compressed)
}

pub fn decode_task_payload(packet: &[u8]) -> anyhow::Result<Vec<LiveTaskPayload>> {
    let bincoded = zstd::decode_all(packet).context("zstd")?;
    let message = serde_json::from_slice(&bincoded).context("bincode")?;
    Ok(message)
}

pub async fn get_completed_tasks_for_store() -> anyhow::Result<Vec<LiveTaskPayload>, anyhow::Error> {
    let query = r#"
        SELECT * FROM task WHERE $this.assignee.store == $auth.store AND $this.completed IS true AND $this.assignee.active == true 
    "#;

    let start_query = web_time::Instant::now();

    let query_results: Vec<LiveTaskPayload> = DATABASE
        .query(query)   
        .await?
        .take(0)?;

    let query_duration = start_query.elapsed();
    log!(format!("Query execution time for completed tasks {query_duration:?}"));

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