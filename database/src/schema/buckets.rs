use std::collections::HashMap;
use anyhow::{Error, Result};

use super::Node;

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
                log::debug!("Merging folder. Prefix: '{}', Normalized: '{}'", new_prefix, normalized_prefix);

                // Use `find_or_create_folder_mut` to ensure the hierarchy exists.
                let target_folder = self.find_or_create_folder_mut(&normalized_prefix);

                match target_folder {
                    Node::Folder(_, children) => {
                        for (key, node) in new_map {
                            let clean_key = key.trim_end_matches('/').to_string(); // Trim trailing slashes
                            if !clean_key.is_empty() {
                                children.insert(clean_key.clone(), node);
                                log::debug!(
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
        log::debug!("find_folder_mut -> Looking for prefix: '{}', Normalized: '{}'", prefix, normalized_prefix);
    
        if normalized_prefix == "/" || normalized_prefix.ends_with(":/") {
            log::debug!("find_folder_mut -> Resolved to root for prefix: '{}'", normalized_prefix);
            return Some(self); // Root directory or drive root (e.g., "C:/")
        }
    
        let parts: Vec<&str> = normalized_prefix.split('/').filter(|p| !p.is_empty()).collect();
        log::debug!("find_folder_mut -> Parts after splitting: {:?}", parts);
    
        let mut current = self;
    
        for part in parts {
            match current {
                Node::Folder(_, children) => {
                    if let Some(node) = children.get_mut(part) {
                        log::debug!("find_folder_mut -> Found part: '{}', Node: {:?}", part, node);
                        current = node;
                    } else {
                        log::debug!("find_folder_mut -> Part not found: '{}'", part);
                        return None;
                    }
                }
                _ => {
                    log::debug!("find_folder_mut -> Current node is not a folder: {:?}", current);
                    return None;
                }
            }
        }
    
        log::debug!("find_folder_mut -> Resolved folder for prefix: '{}'", normalized_prefix);
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


