use database::schema::buckets::list_buckets;
use gloo_worker::{HandlerId, WorkerScope};
use wasm_bindgen_futures::spawn_local;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use gloo_console::log;

#[derive(Debug)]
pub struct Message(pub u32);

#[derive(Debug, Serialize, Deserialize)]
pub struct Input {
    pub url: String,
    pub access_key: String,
    pub secret_key: String,
    pub name: String
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Output {
    pub buckets: Vec<String>,
}

pub struct WebWorker;

impl gloo_worker::Worker for WebWorker {
    type Message = Message;
    type Input = Input;
    type Output = Output;

    fn create(_scope: &WorkerScope<Self>) -> Self {
        log!("create");
        WebWorker
    }

    fn update(&mut self, _scope: &WorkerScope<Self>, msg: Self::Message) {
        log!("update {msg:?}");
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
            let result = list_buckets(msg.url, msg.access_key, msg.secret_key, msg.name).await;
            match result {
                Ok(buckets) => scope.respond(id, Output { buckets }),
                Err(err) => log!(format!("Error: {:?}", err.to_string())),
            }
        });
    }
}