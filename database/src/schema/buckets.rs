use rusty_s3::{actions::ListObjectsV2, Bucket, Credentials, S3Action, UrlStyle::Path};
use web_time::Duration;
use reqwest::{header::ACCEPT_ENCODING, Client, Url};
use anyhow::{Error, Result};
use log::info;


pub async fn list_buckets(url: String, access_key: String, secret_key: String, name: String) -> Result<Vec<String>, Error> {
    const ONE_HOUR: Duration = Duration::from_secs(3600);

    let bucket = Bucket::new(
        url.parse::<Url>().unwrap(), 
        Path, 
        name.to_lowercase(), 
        "us-west"
    )?;
    
    let credentials = Credentials::new(access_key, secret_key);
    
    let action = ListObjectsV2::new(&bucket, Some(&credentials));
    let signed_url = action.sign(ONE_HOUR);
    
    let client = Client::new();

    let resp = client
        .get(signed_url)
        .header(ACCEPT_ENCODING, "br")
        .send()
        .await?
        .error_for_status()?;
    info!("response: {resp:?}");
    let text = resp.text().await?;

    let parsed = ListObjectsV2::parse_response(&text)?;
    

    let mut vec = Vec::new();

    for y in parsed.contents{
        vec.push(y.key);
    }

    Ok(vec)
}