use std::{collections::HashMap, future::Future, pin::Pin};

use log::info;
use rusty_s3::{actions::{list_objects_v2::ListObjectsContent, ListObjectsV2}, Bucket, Credentials, S3Action, UrlStyle::Path};
use reqwest::{header::ACCEPT_ENCODING, Client, Url};
use anyhow::{Error, Result};
use web_time::Duration;

use super::Node;
// #[cfg(target_arch="wasm32")] 
// use gloo_console::log;
/* 
pub async fn list_buckets(url: String, access_key: String, secret_key: String, name: String) -> Result<HashMap<String, Vec<String>>, Error> {
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

    log::info!("parsed: {:?}", parsed.contents);

    let mut vec = Vec::new();
    
    for prefix in parsed.common_prefixes {
        log::info!("Getting strings from prefix: {prefix:?}");
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
        
        let parsed_res = ListObjectsV2::parse_response(&text)?;

        info!("Parsed Res: {:?}", parsed_res);

        if let Some(continuation_token) = &parsed_res.next_continuation_token {
            log::info!("We have a continuation token: {continuation_token:?}");
        }

        for contents in parsed_res.contents {
            vec.push(contents.key);
        }
    }

    info!("All contents: {:?}", vec.len());
    Ok(vec)
}

*/

pub async fn list_buckets(url: String, access_key: String, secret_key: String, name: String) -> Result<Node, Error> {
    const ONE_HOUR: Duration = Duration::from_secs(3600);

    let bucket = Bucket::new(
        url.parse::<Url>().unwrap(), 
        Path, 
        name.to_lowercase(), 
        "us-west"
    )?;
    
    let credentials = Credentials::new(access_key, secret_key);
    
    let new_root = build_node_from_s3(&bucket, &credentials, "").await.await?;
    Ok(new_root)
}

pub async fn build_node_from_s3<'a>(
    bucket: &'a Bucket,
    credentials: &'a Credentials,
    prefix: &'a str,
) -> Pin<Box<dyn Future<Output = Result<Node, Error>> + 'a>> {
    // Box::pin(async move {
        let (all_contents, all_prefixes) = fetch_all_pages(bucket, credentials, prefix).await?;

        // We'll collect the immediate children in one map. Deeper nested folders
        // you can get by recursively calling this function for each `subprefix`.
        let mut map = HashMap::new();

        // Insert files as Node::File
        for content in all_contents {
            let full_path = content.key; // e.g. "folder/subfolder/file.txt"
            let short_name = full_path
                .trim_start_matches(prefix) // remove leading prefix (if present)
                .trim_start_matches('/')
                .to_string();

            map.insert(short_name.clone(), Node::File((full_path, short_name)));
        }

        // Insert subfolders as Node::Folder by calling build_node_from_s3 recursively
        for subprefix in all_prefixes {
            // Example: if `prefix = "folder/"` and `subprefix = "folder/sub1/"`,
            // short_name might be "sub1".
            let short_name = subprefix
                .trim_start_matches(prefix)
                .trim_matches('/')
                .to_string();

            // Recurse:
            let child_node = build_node_from_s3(bucket, credentials, &subprefix).await.await?;
            map.insert(short_name, child_node);
        }

        // Return a folder node containing all the items we discovered
        Ok(Node::Folder(prefix.to_string(), map))
    // })
}


/// Gathers *all* contents and *all* subfolder prefixes from S3 for a given prefix,
/// continuing until there's no more `next_continuation_token`.
pub async fn fetch_all_pages(
    bucket: &Bucket,
    credentials: &Credentials,
    prefix: &str,
) -> Result<(Vec<ListObjectsContent>, Vec<String>), Error> {
    let mut all_contents = Vec::new();
    let mut all_prefixes = Vec::new();

    let mut next_continuation_token: Option<String> = None;
    const ONE_HOUR: Duration = Duration::from_secs(3600);

    loop {
        let mut list_objects_action = ListObjectsV2::new(bucket, Some(credentials));
        
        // If you want folders to appear in common_prefixes, you need a delimiter:
        list_objects_action.query_mut().insert("delimiter", "/");

        // Provide a prefix if desired
        if !prefix.is_empty() {
            list_objects_action.query_mut().insert("prefix", prefix);
        }

        // If we got a continuation token from a previous call, supply it
        if let Some(token) = &next_continuation_token {
            list_objects_action
                .query_mut()
                .insert("continuation-token", token);
        }

        // Sign and send the request
        let signed_url = list_objects_action.sign(ONE_HOUR);
        let response_text = reqwest::Client::new()
            .get(signed_url)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;

        // Parse via `ListObjectsV2::parse_response` (rusty_s3 built-in)
        let parsed_res = ListObjectsV2::parse_response(&response_text)?;

        // Extend the full list of contents (files) with the current page's contents
        all_contents.extend(parsed_res.contents);

        // Extend the full list of common_prefixes (folders)
        for cprefix in parsed_res.common_prefixes {
            all_prefixes.push(cprefix.prefix);
        }

        // If there's a `next_continuation_token`, we keep going. Otherwise, we're done.
        if let Some(token) = parsed_res.next_continuation_token {
            next_continuation_token = Some(token);
        } else {
            break;
        }
    }

    Ok((all_contents, all_prefixes))
}
