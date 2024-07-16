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
pub struct LiveOutput<T>{
    pub data: T,
}

pub struct LiveWorker;

impl <T> gloo_worker::Worker for LiveWorker<T> {
    type Message = Message;
    type Input = LiveInput;
    type Output = LiveOutput<T>;

    fn create(_scope: &WorkerScope<Self>) -> Self {
        info!("create");
        LiveWorker::<T>
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
        let scope: WorkerScope<LiveWorker<T>> = scope.clone();
        spawn_local(async move {
            let result = listen_data("task_note", scope, id).await;
            match result {
                Ok(_) => info!("Got data from worker"),
                Err(e) => info!("Error: {e:?}"),
            }
        });
    }
}

pub async fn listen_data<T>(resource: &str, scope: WorkerScope<LiveWorker<T>>, id: HandlerId) -> anyhow::Result<(), anyhow::Error> 
    where T: DeserializeOwned + Serialize + 'static + Debug + std::marker::Unpin 
{
    let client_stream: Stream<SurrealClient, Vec<T>> = DATABASE.select(resource).live().await?;
    handle_streams(client_stream, scope, id).await?;
    Ok(())
}


async fn handle_streams<T>(
    mut notification_stream: impl futures::Stream<Item = Result<Notification<T>, surrealdb::Error>> + Unpin,
    scope: WorkerScope<LiveWorker<T>>, 
    id: HandlerId
) -> anyhow::Result<(), anyhow::Error> 
    where T: Serialize + Deserialize<'static> + Debug + 'static
{
    while let Some(notification) = notification_stream.next().await {
        let notif: Notification<T> = notification?;
        let data = notif.data;
        let action = notif.action;
        info!("Data: {:?}", action);
        scope.respond(id, LiveOutput { data })
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