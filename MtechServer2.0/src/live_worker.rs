use database::schema::{ComputerData, CustomerData, TicketData};
use gloo_worker::{HandlerId, WorkerScope};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use gloo_worker::Registrable;
use gloo_console::log;

#[allow(dead_code)]
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
        _id: HandlerId,
    ) {
        let msg = format!("{:?}", msg);
        log!(msg);
        let _scope: WorkerScope<LiveWorker> = scope.clone();
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
