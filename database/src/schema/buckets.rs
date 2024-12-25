use std::collections::{HashMap, VecDeque};

use log::info;
use rusty_s3::{actions::{list_objects_v2::ListObjectsContent, ListObjectsV2}, Bucket, Credentials, S3Action, UrlStyle::{self, Path}};
use reqwest::{header::ACCEPT_ENCODING, Url};
use anyhow::{Error, Result};
use web_time::Duration;

use super::Node;


// pub async fn list_buckets(url: String, access_key: String, secret_key: String, name: String) -> Result<Node, Error> {
//     const ONE_HOUR: Duration = Duration::from_secs(3600);

//     let bucket = Bucket::new(
//         url.parse::<Url>().unwrap(), 
//         Path, 
//         name.to_lowercase(), 
//         "us-west"
//     )?;
    
//     let credentials = Credentials::new(access_key, secret_key);
    
//     let mut list_objects_action = ListObjectsV2::new(&bucket, Some(&credentials));
//     list_objects_action.query_mut().insert("delimiter", "/");

//     let signed_url = list_objects_action.sign(ONE_HOUR);
    
//     let client = Client::new();

//     let resp = client
//         .get(signed_url)
//         .header(ACCEPT_ENCODING, "br")
//         .send()
//         .await?
//         .error_for_status()?;

//     let text = resp.text().await?;
    
//     let parsed = ListObjectsV2::parse_response(&text)?;

//     log::info!("parsed: {:?}", parsed);

//     let mut vec = Vec::new();
//     let mut root = Node::Folder(String::new(), HashMap::new());
//     let mut current_path = String::new();

//     for prefix in parsed.common_prefixes {
//         log::info!("Getting strings from prefix: {prefix:?}");
//         let mut list_objs = ListObjectsV2::new(&bucket, Some(&credentials));
//         list_objs.query_mut().insert("prefix", prefix.prefix);
//         list_objs.query_mut().insert("delimiter", "/");

//         if let Node::Folder(_, ref mut folder) = root {
//             root = folder.entry(part.to_string()).or_insert_with(|| Node::Folder(current_path.clone(), HashMap::new()));
//         }

//         let signed_url = list_objs.sign(ONE_HOUR);

//         let resp = client
//             .get(signed_url)
//             .header(ACCEPT_ENCODING, "br")
//             .send()
//             .await?
//             .error_for_status()?;

//         let text = resp.text().await?;
        
//         let parsed_res = ListObjectsV2::parse_response(&text)?;

//         info!("Parsed Res: {:?}", parsed_res);

//         if let Some(continuation_token) = &parsed_res.next_continuation_token {
//             log::info!("We have a continuation token: {continuation_token:?}");
//         }

//         for contents in parsed_res.contents {
//             vec.push(contents.key);
//         }
//     }

//     info!("All contents: {:?}", vec.len());
//     Ok(root)
// }


/// Lists the contents of the provided prefix (or root if `prefix` is None).
///
/// - Returns only the **immediate** files/subfolders at this level.
/// - Each subfolder is returned as a `Node::Folder(prefix, HashMap::new())` 
///   so that you can lazily fetch its contents later by calling the same function
///   with `prefix = Some("sub/folder/")`.
pub async fn list_directory(
    url: String,
    access_key: String,
    secret_key: String,
    bucket_name: String,
    prefix: Option<&str>, // if None => root
) -> Result<Node, Box<dyn std::error::Error>> {
    const ONE_HOUR: Duration = Duration::from_secs(3600);

    // Create Bucket and Credentials
    let bucket = Bucket::new(
        url.parse::<Url>()?,
        UrlStyle::Path,
        bucket_name.to_lowercase(),
        "us-west", // or your actual region
    )?;
    let credentials = Credentials::new(access_key, secret_key);

    // Decide which prefix string to use
    let prefix_str = prefix.unwrap_or(""); // If None => ""

    info!("Listing objects for prefix: '{}'", prefix_str);

    // We will accumulate everything in a single folder-map.
    let mut folder_map = HashMap::new();

    // We'll handle continuation tokens in a loop, but we do *not* recurse subfolders.
    let client = reqwest::Client::new();
    let mut continuation_token: Option<String> = None;

    loop {
        let mut list_objects = ListObjectsV2::new(&bucket, Some(&credentials));

        // If prefix isn't empty, set it
        if !prefix_str.is_empty() {
            list_objects.with_prefix(prefix_str.to_string());
        }

        // Delimiter => we only get immediate subfolders in CommonPrefixes
        list_objects.query_mut().insert("delimiter", "/");

        // If we have a continuation token, use it
        if let Some(ref token) = continuation_token {
            info!("Using continuation token: {token}");
            list_objects.with_continuation_token(token.clone());
        }

        // Sign and execute
        let signed_url = list_objects.sign(ONE_HOUR);
        let resp = client
            .get(signed_url)
            .header(ACCEPT_ENCODING, "br")
            .send()
            .await?
            .error_for_status()?;
        let text = resp.text().await?;

        // Parse response
        let parsed = ListObjectsV2::parse_response(&text)?;

        // ---------- Files ----------
        for content in parsed.contents {
            info!("Found file: '{}'", content.key);
            let short_name = content
                .key
                .strip_prefix(prefix_str)
                .unwrap_or(&content.key)
                .to_string();

            // Insert a File node with (full_path, file_name)
            folder_map.insert(
                short_name.clone(),
                Node::File((content.key.clone(), short_name)),
            );
        }

        // ---------- Subfolders ----------
        for common_prefix in parsed.common_prefixes {
            // Example: "my_subfolder/"
            info!("Found subfolder: '{}'", common_prefix.prefix);

            let short_subfolder_name = common_prefix
                .prefix
                .strip_prefix(prefix_str)
                .unwrap_or(&common_prefix.prefix)
                .trim_end_matches('/')
                .to_string();

            // We do NOT recursively fetch it here. We simply store it as an empty Folder node.
            folder_map.insert(
                short_subfolder_name,
                Node::Folder(common_prefix.prefix.clone(), HashMap::new()),
            );
        }

        // ---------- Continuation ----------
        if let Some(next_token) = parsed.next_continuation_token {
            info!("Truncated result. NextContinuationToken: {}", next_token);
            continuation_token = Some(next_token);
        } else {
            // Done listing this prefix
            break;
        }
    }

    // Return a Folder node for this prefix
    // with all discovered files (Node::File) and immediate subfolders (Node::Folder(...))
    Ok(Node::Folder(prefix_str.to_string(), folder_map))
}


// pub async fn list_buckets(
//     url: String,
//     access_key: String,
//     secret_key: String,
//     bucket_name: String,
// ) -> Result<Node, Error> {
//     const ONE_HOUR: Duration = Duration::from_secs(3600);

//     // Create the Bucket and Credentials
//     let bucket = Bucket::new(
//         url.parse::<Url>()?,
//         UrlStyle::Path,
//         bucket_name.to_lowercase(),
//         "us-west", // or your actual region
//     )?;
//     let credentials = Credentials::new(access_key, secret_key);

//     // prefix_map: maps a prefix string -> Node::Folder(prefix, HashMap).
//     // The empty prefix "" is our "root" folder node.
//     let mut prefix_map: HashMap<String, Node> = HashMap::new();

//     // Insert the root folder node:
//     prefix_map.insert(
//         "".to_string(),
//         Node::Folder("root".to_string(), HashMap::new()),
//     );

//     // We'll process folders in BFS order, starting with the empty prefix
//     let mut queue = VecDeque::new();
//     queue.push_back("".to_string());

//     let client = reqwest::Client::new();

//     while let Some(prefix) = queue.pop_front() {
//         // Temporarily remove this node from prefix_map so we can mutate it safely
//         // (and so we don't hold any mutable reference to prefix_map).
//         let mut node = match prefix_map.remove(&prefix) {
//             Some(n) => n,
//             None => {
//                 log::error!("No node for prefix '{prefix}'. Skipping.");
//                 continue;
//             }
//         };

//         // We only do S3 listing if this is a folder
//         let Node::Folder(_, ref mut folder_map) = node else {
//             info!("Prefix \"{}\" is not a folder, skipping S3 listing.", prefix);
//             // Put the node back
//             prefix_map.insert(prefix.clone(), node);
//             continue;
//         };

//         info!("Processing folder prefix: \"{}\"", prefix);
//         let mut continuation_token: Option<String> = None;

//         // In a loop, list objects until no more continuation tokens
//         loop {
//             let mut list_objects = ListObjectsV2::new(&bucket, Some(&credentials));

//             // If prefix is non-empty, specify it
//             if !prefix.is_empty() {
//                 list_objects.with_prefix(prefix.clone());
//             }

//             // Use delimiter so S3 returns CommonPrefixes for subfolders
//             list_objects.query_mut().insert("delimiter", "/");

//             // If we have a continuation token from the previous page, apply it
//             if let Some(ref token) = continuation_token {
//                 info!("Using continuation token: {token}");
//                 list_objects.with_continuation_token(token.clone());
//             }

//             // Sign and execute the request
//             let signed_url = list_objects.sign(ONE_HOUR);
//             let resp = client
//                 .get(signed_url)
//                 .header(ACCEPT_ENCODING, "br")
//                 .send()
//                 .await?
//                 .error_for_status()?;

//             let text = resp.text().await?;
//             let parsed = ListObjectsV2::parse_response(&text)?;

//             // ~~~~~ FILES ~~~~~
//             for content in parsed.contents {
//                 info!("Found file: '{}'", content.key);
//                 let short_name = content
//                     .key
//                     .strip_prefix(&prefix)
//                     .unwrap_or(&content.key)
//                     .to_string();

//                 folder_map.insert(
//                     short_name.clone(),
//                     Node::File((content.key.clone(), short_name)),
//                 );
//             }

//             // ~~~~~ SUBDIRECTORIES ~~~~~
//             for cp in parsed.common_prefixes {
//                 // For example: "some/folder/"
//                 info!("Found subfolder (common prefix): '{}'", cp.prefix);

//                 // Get subfolder name relative to the current prefix
//                 let short_subfolder_name = cp
//                     .prefix
//                     .strip_prefix(&prefix)
//                     .unwrap_or(&cp.prefix)
//                     .trim_end_matches('/')
//                     .to_string();

//                 // If it's not in prefix_map yet, we create an entry for it
//                 if !prefix_map.contains_key(&cp.prefix) {
//                     prefix_map.insert(
//                         cp.prefix.clone(),
//                         Node::Folder(cp.prefix.clone(), HashMap::new()),
//                     );
//                 }

//                 // Insert a reference in the current folder
//                 folder_map.insert(
//                     short_subfolder_name,
//                     prefix_map[&cp.prefix].clone(),
//                 );

//                 // Enqueue this subfolder prefix for BFS
//                 queue.push_back(cp.prefix.clone());
//             }

//             // Check if there's another page
//             if let Some(next_token) = parsed.next_continuation_token {
//                 info!("Truncated result. Next token: {next_token}");
//                 continuation_token = Some(next_token);
//             } else {
//                 break;
//             }
//         }

//         // Now that we've updated this folder, re-insert it into prefix_map
//         prefix_map.insert(prefix.clone(), node);
//     }

//     info!("Finished building the Node structure for bucket.");

//     // Return the root node (which is stored under "")
//     let root_node = prefix_map.remove("").unwrap();
//     Ok(root_node)
// }

