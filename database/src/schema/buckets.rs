use rusty_s3::{actions::ListObjectsV2, Bucket, Credentials, S3Action, UrlStyle::Path};
use reqwest::{header::ACCEPT_ENCODING, Client, Url};
use anyhow::{Error, Result};
use web_time::Duration;
// #[cfg(target_arch="wasm32")] 
// use gloo_console::log;

pub async fn list_buckets(url: String, access_key: String, secret_key: String, name: String) -> Result<Vec<String>, Error> {
    const ONE_HOUR: Duration = Duration::from_secs(3600);

    let bucket = Bucket::new(
        url.parse::<Url>().unwrap(), 
        Path, 
        name.to_lowercase(), 
        "us-west"
    )?;
    
    let credentials = Credentials::new(access_key, secret_key);
    
    let mut list_objects_action = ListObjectsV2::new(&bucket, Some(&credentials));
    list_objects_action.query_mut().insert("delimiter", "/");

    let signed_url = list_objects_action.sign(ONE_HOUR);
    
    let client = Client::new();

    let resp = client
        .get(signed_url)
        .header(ACCEPT_ENCODING, "br")
        .send()
        .await?
        .error_for_status()?;
    let text = resp.text().await?;
    
    let parsed = ListObjectsV2::parse_response(&text)?;

    #[cfg(target_arch="wasm32")] 
    log::info!("parsed: {parsed:?}");

    let mut vec = Vec::new();
    
    for prefix in parsed.common_prefixes{
        let mut list_objs = ListObjectsV2::new(&bucket, Some(&credentials));
        list_objs.query_mut().insert("prefix", prefix.prefix);
        list_objs.query_mut().insert("delimiter", "/");

        let signed_url = list_objs.sign(ONE_HOUR);

        let resp = client
            .get(signed_url)
            .header(ACCEPT_ENCODING, "br")
            .send()
            .await?
            .error_for_status()?;

        let text = resp.text().await?;
        
        let parsed = ListObjectsV2::parse_response(&text)?;

        for y in parsed.contents{
            vec.push(y.key);
        }
    }

    Ok(vec)
}