use rusty_s3::{actions::ListObjectsV2, Bucket, Credentials, S3Action, UrlStyle::Path};
use gloo_worker::{HandlerId, WorkerScope};
use wasm_bindgen_futures::spawn_local;
use serde::{Deserialize, Serialize};
use reqwest::{Client, Error, Url};
use web_time::Duration;
use std::fmt::Debug;
use log::info;

const ONE_HOUR: Duration = Duration::from_secs(3600);

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
            let result = list_buckets(msg.url, msg.access_key, msg.secret_key, msg.name).await;
            match result {
                Ok(buckets) => scope.respond(id, Output { buckets }),
                Err(err) => info!("Error: {:?}", err),
            }
        });
    }
}


pub async fn list_buckets(url: String, access_key: String, secret_key: String, name: String) -> Result<Vec<String>, Error> {
    const ONE_HOUR: Duration = Duration::from_secs(3600);

    let bucket = Bucket::new(
        url.parse::<Url>().unwrap(), 
        Path, 
        name.to_lowercase(), 
        "us-west"
    ).expect("Couldnt get buckets");
    
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