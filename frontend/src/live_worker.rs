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
        spawn_local(async move {
            let result = get_customer_data().await;
            match result {
                Ok(out) => scope.respond(id, out),
                Err(e) => {
                    log!(e.to_string());
                },
            }
        });
    }
}

pub async fn get_customer_data() -> anyhow::Result<LiveOutput, anyhow::Error> { // tx: Sender<CustomerData>
    log!("get_customers");
    match DATABASE.health().await{
        Ok(_) => log!("have a connection"),
        Err(e) => {
            error!(e.to_string());
        }
    }
    let customers: Vec<CustomerData> = DATABASE.query("SELECT * FROM customer").await?.take(0)?;
    let msg = format!("{:?}", customers);
    log!(msg);
    DATABASE.set("id", "value").await?;
    let computers: Vec<ComputerData> = DATABASE.query("SELECT * FROM computer").await?.take(0)?;
    let tickets: Vec<TicketData> = DATABASE.query("SELECT * FROM service_order").await?.take(0)?;

    let output = LiveOutput{ customers, computers, tickets };
    Ok(output)
}

pub async fn listen_data<T>(resource: &str, scope: WorkerScope<LiveWorker>, id: HandlerId) -> anyhow::Result<(), anyhow::Error> 
    where T: DeserializeOwned + Serialize + 'static + Debug + std::marker::Unpin, TaskNotePayload: From<T>
{
    let client_stream: Stream<SurrealClient, Vec<T>> = DATABASE.select(resource).live().await?;
    handle_streams(client_stream, scope, id).await?;
    Ok(())
}


async fn handle_streams<T>(
    mut notification_stream: impl futures::Stream<Item = Result<Notification<T>, surrealdb::Error>> + Unpin,
    scope: WorkerScope<LiveWorker>, 
    id: HandlerId
) -> anyhow::Result<(), anyhow::Error> 
    where T: Serialize + Deserialize<'static> + Debug + 'static, TaskNotePayload: From<T>
{
    while let Some(notification) = notification_stream.next().await {
        let notif: Notification<T> = notification?;
        let data = notif.data;
        let action = notif.action;
        log!(format!("Data: {:?}", action));
        // scope.respond(id, LiveOutput { data: data.into() })
    }; 
    Ok(())
}
