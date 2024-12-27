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

impl Node {
    /// Merges a new `Node::Folder` into the existing tree.
    ///
    /// - **new_node**: The `Node::Folder` to merge. Its `prefix` must correspond to an existing folder in the tree.
    ///
    /// Returns `Ok(())` on success or an error if the target folder is not found.
    pub fn merge_node(&mut self, new_node: Node) -> Result<(), Error> {
        match new_node {
            Node::Folder(new_prefix, new_map) => {
                info!("Merging folder: '{}'", new_prefix);
                
                let default_node = &mut Node::Folder(String::new(), HashMap::new());
                // Find the target folder in the existing tree
                let target_folder = self.find_folder_mut(&new_prefix)
                    .ok_or_else(|| 
                        Err::<&mut Node, anyhow::Error>(
                            anyhow::anyhow!("Folder with prefix '{new_prefix}' not found")
                        )
                    ).unwrap_or(default_node);
                
                match target_folder {
                    Node::Folder(_, ref mut children) => {
                        for (_, node) in new_map {
                            match node {
                                Node::File((full_path, file_name)) => {
                                    if children.contains_key(&file_name) {
                                        info!("File '{}' already exists. Skipping.", full_path);
                                    } else {
                                        info!("Inserting file: '{}'", full_path);
                                        children.insert(file_name.clone(), Node::File((full_path, file_name.clone())));
                                    }
                                }
                                Node::Folder(sub_prefix, sub_map) => {
                                    let subfolder_name = sub_prefix.trim_end_matches('/').rsplit('/').next().unwrap_or(&sub_prefix).to_string();
                                    if children.contains_key(&subfolder_name) {
                                        info!("Folder '{}' already exists. Skipping.", sub_prefix);
                                    } else {
                                        info!("Inserting folder: '{}'", sub_prefix);
                                        children.insert(subfolder_name.clone(), Node::Folder(sub_prefix, sub_map));
                                    }
                                }
                            }
                        }
                        Ok(())
                    }
                    _ => Err(anyhow::anyhow!("Target node is not a folder.")).into(),
                }
            }
            Node::File(_) => {
                Err(anyhow::anyhow!("Expected a Folder node, got File.")).into()
            }
        }
    }

    /// Helper function to find a mutable reference to a folder node with the given prefix.
    ///
    /// - **prefix**: The full prefix path to the folder (e.g., "folder/subfolder/").
    ///
    /// Returns a mutable reference to the `Node::Folder` if found, otherwise `None`.
    pub fn find_folder_mut<'a>(&'a mut self, prefix: &str) -> Option<&'a mut Node> {
        if prefix.is_empty() || prefix == "" {
            return Some(self);
        }

        // Split the prefix into parts, e.g., "folder/subfolder/" -> ["folder", "subfolder"]
        let parts: Vec<&str> = prefix.trim_end_matches('/').split('/').collect();

        let mut current = self;

        for part in &parts {
            match current {
                Node::Folder(_, ref mut children) => {
                    if let Some(node) = children.get_mut(*part) {
                        current = node;
                    } else {
                        // Folder does not exist
                        return None;
                    }
                }
                _ => {
                    return None;
                }
            }
        }

        Some(current)
    }

    /// Finds an immutable reference to a folder node with the given prefix.
    ///
    /// - **prefix**: The full prefix path to the folder (e.g., "folder/subfolder/").
    ///
    /// Returns an immutable reference to the `Node::Folder` if found, otherwise `None`.
    pub fn find_folder(&self, prefix: &str) -> Option<&Node> {
        if prefix.is_empty() || prefix == "" {
            return Some(self);
        }

        // Split the prefix into parts, e.g., "folder/subfolder/" -> ["folder", "subfolder"]
        let parts: Vec<&str> = prefix.trim_end_matches('/').split('/').collect();

        let mut current = self;

        for part in &parts {
            match current {
                Node::Folder(_, ref children) => {
                    if let Some(node) = children.get(*part) {
                        current = node;
                    } else {
                        // Folder does not exist
                        return None;
                    }
                }
                _ => {
                    return None;
                }
            }
        }

        Some(current)
    }
}
