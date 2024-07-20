use database::schema::{ComputerData, CustomerData, TaskNotePayload, TaskPayload, TicketData, TicketPayload, DB, NS, USER_SCOPE};
use database::{Auth, Database, DATABASE, DB_URL};
use gloo_worker::{HandlerId, WorkerScope};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use surrealdb::engine::remote::ws::{Client as SurrealClient, Wss};
use surrealdb::opt::auth::Scope;
use surrealdb::{method::Stream, Notification};
use wasm_bindgen_futures::spawn_local;
use std::fmt::Debug;
use futures::StreamExt;
use gloo_worker::Registrable;
use gloo_console::{log, error};

fn main() {
    LiveWorker::registrar().register();
}



#[derive(Debug)]
pub struct Message(pub u32);


#[derive(Debug, Serialize, Deserialize)]
pub struct LiveInput {
    pub url: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct LiveOutput{
    pub customers: Vec<CustomerData>,
    pub computers: Vec<ComputerData>,
    pub tickets: Vec<TicketData>,
}

pub struct LiveWorker;

impl gloo_worker::Worker for LiveWorker {
    type Message = Message;
    type Input = LiveInput;
    type Output = LiveOutput;

    fn create(_scope: &WorkerScope<Self>) -> Self {
        log!("create");
        LiveWorker
    }

    fn update(&mut self, _scope: &WorkerScope<Self>, msg: Self::Message) {
        let msg = format!("{:?}", msg);
        log!(msg);
    }

    fn received(
        &mut self,
        scope: &WorkerScope<Self>,
        msg: Self::Input,
        id: HandlerId,
    ) {
        let msg = format!("{:?}", msg);
        log!(msg);
        let scope: WorkerScope<LiveWorker> = scope.clone();
        // spawn_local(async move {
            // let result = get_customer_data().await;
            // match result {
            //     Ok(out) => scope.respond(id, out),
            //     Err(e) => {
            //         log!(e.to_string());
            //     },
            // }
        // });
    }
}
