use database::schema::utilities::decompress_data;
use gloo_worker::{HandlerId, WorkerScope};
use wasm_bindgen_futures::spawn_local;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use gloo_console::log;

use gloo_worker::Registrable;
fn main() {
    DeserWorker::registrar().register();
}

#[derive(Debug)]
pub struct Message(pub u32);

#[derive(Debug, Serialize, Deserialize)]
pub struct Input(pub Vec<u8>);

#[derive(Debug, Serialize, Deserialize)]
pub struct Output(pub Vec<u8>);

pub struct DeserWorker; // <T: for <'de> Deserialize <'de> + Serialize>;

impl gloo_worker::Worker for DeserWorker {
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
        // log!(format!("received {msg:?}"));
        let scope = scope.clone();
        spawn_local(async move {
            match decompress_data(&msg.0) {
                Ok(bin) => scope.respond(id, Output(bin)),
                Err(e) => gloo_console::info!(format!("Error decompressing data: {e:?}"))
            }
        });
    }
}
