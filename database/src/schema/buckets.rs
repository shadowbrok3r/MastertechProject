use rusty_s3::{actions::ListObjectsV2, Bucket, Credentials, S3Action};
use reqwest::header::ACCEPT_ENCODING;
use std::collections::HashMap;
use anyhow::{Error, Result};
use web_time::Duration;
use log::info;

use super::Node;

/// Lists the contents of the provided prefix (or root if `prefix` is None).
///
/// - Returns only the **immediate** files/subfolders at this level.
/// - Each subfolder is returned as a `Node::Folder(prefix, HashMap::new())` 
///   so that you can lazily fetch its contents later by calling the same function
///   with `prefix = Some("sub/folder/")`.
pub async fn list_buckets(
    credentials: Credentials,
    bucket: Bucket,
    prefix: Option<&str>, // if None => root
) -> Result<Node, Error> {
    const ONE_HOUR: Duration = Duration::from_secs(3600);
    
    // Decide which prefix string to use
    let prefix_str = prefix.unwrap_or("").trim_end_matches('/');

    info!("database/schema/buckets.rs -> Listing objects for prefix: '{}'", prefix_str);

    // We will accumulate everything in a single folder-map.
    let mut folder_map = HashMap::new();

    // We'll handle continuation tokens in a loop, but we do *not* recurse subfolders.
    let client = reqwest::Client::new();
    let mut continuation_token: Option<String> = None;

    loop {
        let mut list_objects = ListObjectsV2::new(&bucket, Some(&credentials));

        // If prefix isn't empty, set it
        if !prefix_str.is_empty() {
            list_objects.with_prefix(format!("{}/", prefix_str));
        }

        // Delimiter => we only get immediate subfolders in CommonPrefixes
        list_objects.query_mut().insert("delimiter", "/");

        // If we have a continuation token, use it
        if let Some(ref token) = continuation_token {
            info!("database/schema/buckets.rs -> Using continuation token: {token}");
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

        for content in parsed.contents {
            info!(
                "database/schema/buckets.rs -> Found file: '{}', Parent Prefix: '{}'",
                content.key, prefix_str
            );
        
            let short_name = content
                .key
                .strip_prefix(&(prefix_str.to_string() + "/"))
                .unwrap_or(&content.key)
                .to_string();
        
            info!(
                "Processed file. Full Key: '{}', Short Name: '{}', Parent Prefix: '{}'",
                content.key, short_name, prefix_str
            );
        
            folder_map.insert(
                short_name.clone(),
                Node::File((content.key.clone(), short_name)),
            );
        }
        
        for common_prefix in parsed.common_prefixes {
            info!("database/schema/buckets.rs -> Found subfolder: '{}'", common_prefix.prefix);
        
            let short_subfolder_name = common_prefix
                .prefix
                .strip_prefix(&(prefix_str.to_string() + "/"))
                .unwrap_or(&common_prefix.prefix)
                .trim_end_matches('/')
                .to_string();
        
            info!(
                "Processed subfolder. Full Prefix: '{}', Short Name: '{}', Parent Prefix: '{}'",
                common_prefix.prefix, short_subfolder_name, prefix_str
            );
        
            folder_map.insert(
                short_subfolder_name,
                Node::Folder(common_prefix.prefix.clone(), HashMap::new()),
            );
        }
        
        

        // ---------- Continuation ----------
        if let Some(next_token) = parsed.next_continuation_token {
            info!("database/schema/buckets.rs -> Truncated result. NextContinuationToken: {}", next_token);
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

impl Node {
    /// Merges a new `Node::Folder` into the existing tree.
    ///
    /// - **new_node**: The `Node::Folder` to merge. Its `prefix` must correspond to an existing folder in the tree.
    ///
    /// Returns `Ok(())` on success or an error if the target folder is not found.
    pub fn merge_node(&mut self, new_node: Node) -> Result<(), Error> {
        match new_node {
            Node::Folder(new_prefix, new_map) => {
                let normalized_prefix = normalize_prefix(&new_prefix);
                info!("Merging folder. Prefix: '{}', Normalized: '{}'", new_prefix, normalized_prefix);

                // Use `find_or_create_folder_mut` to ensure the hierarchy exists.
                let target_folder = self.find_or_create_folder_mut(&normalized_prefix);

                match target_folder {
                    Node::Folder(_, children) => {
                        for (key, node) in new_map {
                            let clean_key = key.trim_end_matches('/').to_string(); // Trim trailing slashes
                            if !clean_key.is_empty() {
                                children.insert(clean_key.clone(), node);
                                info!(
                                    "Inserted '{}'. Current children of '{}': {:?}",
                                    clean_key, normalized_prefix, children.keys().collect::<Vec<_>>()
                                );
                            }
                        }
                        Ok(())
                    }
                    _ => Err(anyhow::anyhow!("Target node is not a folder.")),
                }
            }
            Node::File(_) => Err(anyhow::anyhow!("Expected a Folder node, got File.")),
        }
    }
    
    /// Helper function to find or create a mutable reference to a folder node with the given prefix.
    ///
    /// - **prefix**: The full prefix path to the folder (e.g., "folder/subfolder/").
    ///
    /// Returns a mutable reference to the `Node::Folder`.
    pub fn find_or_create_folder_mut(&mut self, prefix: &str) -> &mut Node {
        let normalized_prefix = normalize_prefix(prefix);

        if normalized_prefix == "/" || normalized_prefix.ends_with(":/") {
            return self; // Root directory or drive root (e.g., "C:/")
        }

        let parts: Vec<&str> = normalized_prefix.split('/').filter(|p| !p.is_empty()).collect();
        let mut current = self;

        for part in parts {
            current = match current {
                Node::Folder(_, children) => {
                    children.entry(part.to_string()).or_insert_with(|| {
                        let new_folder_prefix = format!("{}/", part);
                        Node::Folder(new_folder_prefix, HashMap::new())
                    })
                }
                _ => panic!("Tried to navigate into a non-folder node."),
            };
        }

        current
    }

    /// Helper function to find a mutable reference to a folder node with the given prefix.
    ///
    /// - **prefix**: The full prefix path to the folder (e.g., "folder/subfolder/").
    ///
    /// Returns a mutable reference to the `Node::Folder` if found, otherwise `None`.
    pub fn find_folder_mut(&mut self, prefix: &str) -> Option<&mut Node> {
        let normalized_prefix = normalize_prefix(prefix);
        info!("find_folder_mut -> Looking for prefix: '{}', Normalized: '{}'", prefix, normalized_prefix);
    
        if normalized_prefix == "/" || normalized_prefix.ends_with(":/") {
            info!("find_folder_mut -> Resolved to root for prefix: '{}'", normalized_prefix);
            return Some(self); // Root directory or drive root (e.g., "C:/")
        }
    
        let parts: Vec<&str> = normalized_prefix.split('/').filter(|p| !p.is_empty()).collect();
        info!("find_folder_mut -> Parts after splitting: {:?}", parts);
    
        let mut current = self;
    
        for part in parts {
            match current {
                Node::Folder(_, children) => {
                    if let Some(node) = children.get_mut(part) {
                        info!("find_folder_mut -> Found part: '{}', Node: {:?}", part, node);
                        current = node;
                    } else {
                        info!("find_folder_mut -> Part not found: '{}'", part);
                        return None;
                    }
                }
                _ => {
                    info!("find_folder_mut -> Current node is not a folder: {:?}", current);
                    return None;
                }
            }
        }
    
        info!("find_folder_mut -> Resolved folder for prefix: '{}'", normalized_prefix);
        Some(current)
    }
    
    /// Finds an immutable reference to a folder node with the given prefix.
    ///
    /// - **prefix**: The full prefix path to the folder (e.g., "folder/subfolder/").
    ///
    /// Returns an immutable reference to the `Node::Folder` if found, otherwise `None`.
    pub fn find_folder(&self, prefix: &str) -> Option<&Node> {
        let normalized_prefix = normalize_prefix(prefix);
    
        if normalized_prefix == "/" || normalized_prefix.ends_with(":/") {
            return Some(self); // Root directory or drive root (e.g., "C:/")
        }
    
        let parts: Vec<&str> = normalized_prefix.split('/').filter(|p| !p.is_empty()).collect();
        let mut current = self;
    
        for part in parts {
            match current {
                Node::Folder(_, children) => {
                    if let Some(node) = children.get(part) {
                        current = node;
                    } else {
                        return None;
                    }
                }
                _ => return None,
            }
        }
    
        Some(current)
    }
  
}

pub fn normalize_prefix(path: &str) -> String {
    let mut normalized = path.replace('\\', "/"); // Convert backslashes to forward slashes

    if normalized.len() >= 2 && normalized.chars().nth(1) == Some(':') {
        // Handle drive letters (e.g., "C:/path/to/file")
        if !normalized.ends_with('/') {
            normalized.push('/');
        }
        normalized
    } else {
        // Non-Windows paths
        if normalized.starts_with('/') {
            // Absolute paths
            if normalized == "/" {
                "/".to_string()
            } else {
                normalized.trim_end_matches('/').to_string()
            }
        } else {
            // Relative paths
            normalized.trim_matches('/').to_string()
        }
    }
}


