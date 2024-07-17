use database::schema::{ComputerData, CustomerData, TaskNotePayload, TaskPayload, TicketPayload};
use database::DATABASE;
use gloo_worker::{HandlerId, WorkerScope};
use log::info;
use rusty_s3::{actions::ListObjectsV2, Bucket, Credentials, S3Action};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use surrealdb::engine::remote::ws::Client as SurrealClient;
use surrealdb::{method::Stream, Notification};
use wasm_bindgen_futures::spawn_local;
use reqwest::{Client, Error, Url};
use web_time::Duration;
use std::fmt::Debug;
use futures::StreamExt;

const ONE_HOUR: Duration = Duration::from_secs(3600);

#[derive(Debug)]
pub struct Message(pub u32);

#[derive(Debug, Serialize, Deserialize)]
pub struct Input {
    pub url: String,
    pub access_key: String,
    pub secret_key: String,
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
        info!("create");
        WebWorker
    }

    fn update(&mut self, _scope: &WorkerScope<Self>, msg: Self::Message) {
        info!("update {msg:?}");
    }

    fn received(
        &mut self,
        scope: &WorkerScope<Self>,
        msg: Self::Input,
        id: HandlerId,
    ) {
        info!("received {msg:?}");
        let scope = scope.clone();
        spawn_local(async move {
            let result = list_buckets(msg.url, msg.access_key, msg.secret_key).await;
            match result {
                Ok(buckets) => scope.respond(id, Output { buckets }),
                Err(err) => info!("Error: {:?}", err),
            }
        });
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LiveInput {
    pub url: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LiveOutput{
    pub customers: Vec<CustomerData>,
    pub computers: Vec<ComputerData>,
    pub tickets: Vec<TicketPayload>,
    pub tasks: Vec<TaskPayload>,
}

pub struct LiveWorker;

impl gloo_worker::Worker for LiveWorker {
    type Message = Message;
    type Input = LiveInput;
    type Output = LiveOutput;

    fn create(_scope: &WorkerScope<Self>) -> Self {
        info!("create");
        LiveWorker
    }

    fn update(&mut self, _scope: &WorkerScope<Self>, msg: Self::Message) {
        info!("update {msg:?}");
    }

    fn received(
        &mut self,
        scope: &WorkerScope<Self>,
        msg: Self::Input,
        id: HandlerId,
    ) {
        info!("received {msg:?}");
        let scope: WorkerScope<LiveWorker> = scope.clone();
        spawn_local(async move {
            let result = get_customer_data().await;
            match result {
                Ok(out) => scope.respond(id, out),
                Err(e) => info!("Error: {e:?}"),
            }
        });
    }
}

pub async fn get_customer_data() -> anyhow::Result<LiveOutput, anyhow::Error> { // tx: Sender<CustomerData>
    info!("get_customers");
    let customers: Vec<CustomerData> = DATABASE.query("SELECT * FROM customer").await?.take(0)?;
    DATABASE.set("id", "value").await;
    let computers: Vec<ComputerData> = DATABASE.query("SELECT * FROM computer where customer == $id").await?.take(0)?;
    let tickets: Vec<TicketPayload> = DATABASE.query("SELECT * FROM service_order where customer == $id").await?.take(0)?;
    let tasks: Vec<TaskPayload> = DATABASE.query("SELECT * FROM task where service_order == $id").await?.take(0)?;

    let output = LiveOutput{ customers, computers, tickets, tasks };
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
        info!("Data: {:?}", action);
        // scope.respond(id, LiveOutput { data: data.into() })
    }; 
    Ok(())
}


async fn list_buckets(url: String, access_key: String, secret_key: String) -> Result<Vec<String>, Error> {
    let name = "logan";
    let region = "us-west";
    let bucket = Bucket::new(url.parse::<Url>().unwrap(), rusty_s3::UrlStyle::Path, name, region).expect("Url has a valid scheme and host");
    
    let credentials = Credentials::new(access_key, secret_key);
    
    let action = ListObjectsV2::new(&bucket, Some(&credentials));
    let signed_url = action.sign(ONE_HOUR);
    
    let client = Client::new();
    let resp = client.get(signed_url).send().await?.error_for_status()?;
    let text = resp.text().await?;
    let parsed = ListObjectsV2::parse_response(&text).unwrap();
    info!("response: {parsed:?}");

    let mut vec = Vec::new();

    for y in parsed.contents{
        vec.push(y.key);
    }

    Ok(vec)
}


// #[derive(Debug)]
// pub struct Message(pub u32);
// #[derive(Debug, serde::Serialize, serde::Deserialize)]
// pub struct Input(pub u32);
// #[derive(Debug, serde::Serialize, serde::Deserialize)]
// pub struct Output(pub u32);
// impl gloo_worker::Worker for WebWorker {
//     type Message = Message;
//     type Input = Input;
//     type Output = Output;

//     fn create(_scope: &gloo_worker::WorkerScope<Self>) -> Self {
//         log::info!("create");
//         Self {}
//     }

//     fn update(&mut self, _scope: &gloo_worker::WorkerScope<Self>, msg: Self::Message) {
//         log::info!("update {msg:?}");
//     }

//     fn received(
//         &mut self,
//         scope: &gloo_worker::WorkerScope<Self>,
//         msg: Self::Input,
//         _id: gloo_worker::HandlerId,
//     ) {
//         log::info!("received {msg:?}");
//         scope.respond(_id, Output(msg.0 + 5001));
//     }
// }