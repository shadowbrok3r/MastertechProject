//! SurrealDB File Storage API
//! 
//! This module provides file storage operations using SurrealDB's built-in bucket/file system.
//! Requires SurrealDB 3.0.0-beta.1 or later.

use crate::DATABASE;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// File metadata returned by file::head
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileMetadata {
    pub key: String,
    pub size: Option<u64>,
    pub etag: Option<String>,
    pub content_type: Option<String>,
    pub last_modified: Option<String>,
}

/// A file entry for directory listings
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileEntry {
    pub key: String,
    pub is_directory: bool,
    pub size: Option<u64>,
}

/// Initialize a bucket for a user (should be called when user signs up or first uses storage)
pub async fn init_user_bucket(username: &str) -> anyhow::Result<(), anyhow::Error> {
    let bucket_name = sanitize_bucket_name(username);
    let query = format!("DEFINE BUCKET IF NOT EXISTS {}", bucket_name);
    DATABASE.query(&query).await?;
    log::info!("Initialized bucket: {}", bucket_name);
    Ok(())
}

/// Put a file into the user's bucket
/// 
/// # Arguments
/// * `bucket` - The bucket name (typically the username)
/// * `path` - The path within the bucket (e.g., "Scripts/myscript.ps1")
/// * `data` - The file contents as bytes
pub async fn put_file(bucket: &str, path: &str, data: Vec<u8>) -> anyhow::Result<(), anyhow::Error> {
    let bucket_name = sanitize_bucket_name(bucket);
    let normalized_path = normalize_path(path);
    
    // Create the file reference string
    let file_ref = format!(r#"f"{}:{}""#, bucket_name, normalized_path);
    
    // Use query with parameters to safely pass the binary data
    let query = format!("RETURN file::put({}, $data)", file_ref);
    DATABASE
        .query(&query)
        .bind(("data", data))
        .await?;
    
    log::info!("Put file: {}:{}", bucket_name, normalized_path);
    Ok(())
}

/// Put a file only if it doesn't already exist
pub async fn put_file_if_not_exists(bucket: &str, path: &str, data: Vec<u8>) -> anyhow::Result<bool, anyhow::Error> {
    let bucket_name = sanitize_bucket_name(bucket);
    let normalized_path = normalize_path(path);
    
    let file_ref = format!(r#"f"{}:{}""#, bucket_name, normalized_path);
    let query = format!("RETURN file::put_if_not_exists({}, $data)", file_ref);
    
    DATABASE
        .query(&query)
        .bind(("data", data))
        .await?;
    
    Ok(true)
}

/// Get a file from the user's bucket
/// 
/// Returns None if the file doesn't exist
pub async fn get_file(bucket: &str, path: &str) -> anyhow::Result<Option<Vec<u8>>, anyhow::Error> {
    let bucket_name = sanitize_bucket_name(bucket);
    let normalized_path = normalize_path(path);
    
    let file_ref = format!(r#"f"{}:{}""#, bucket_name, normalized_path);
    let query = format!("RETURN file::get({})", file_ref);
    
    let mut response = DATABASE.query(&query).await?;
    let data: Option<Vec<u8>> = response.take(0)?;
    
    Ok(data)
}

/// Get file metadata without downloading the content
pub async fn head_file(bucket: &str, path: &str) -> anyhow::Result<Option<FileMetadata>, anyhow::Error> {
    let bucket_name = sanitize_bucket_name(bucket);
    let normalized_path = normalize_path(path);
    
    let file_ref = format!(r#"f"{}:{}""#, bucket_name, normalized_path);
    let query = format!("RETURN file::head({})", file_ref);
    
    let mut response = DATABASE.query(&query).await?;
    let metadata: Option<Value> = response.take(0)?;
    
    match metadata {
        Some(val) => {
            let meta: FileMetadata = serde_json::from_value(val).unwrap_or_default();
            Ok(Some(meta))
        }
        None => Ok(None)
    }
}

/// Delete a file from the user's bucket
pub async fn delete_file(bucket: &str, path: &str) -> anyhow::Result<(), anyhow::Error> {
    let bucket_name = sanitize_bucket_name(bucket);
    let normalized_path = normalize_path(path);
    
    let file_ref = format!(r#"f"{}:{}""#, bucket_name, normalized_path);
    let query = format!("RETURN file::delete({})", file_ref);
    
    DATABASE.query(&query).await?;
    
    log::info!("Deleted file: {}:{}", bucket_name, normalized_path);
    Ok(())
}

/// Check if a file exists
pub async fn file_exists(bucket: &str, path: &str) -> anyhow::Result<bool, anyhow::Error> {
    let bucket_name = sanitize_bucket_name(bucket);
    let normalized_path = normalize_path(path);
    
    let file_ref = format!(r#"f"{}:{}""#, bucket_name, normalized_path);
    let query = format!("RETURN file::exists({})", file_ref);
    
    let mut response = DATABASE.query(&query).await?;
    let exists: Option<bool> = response.take(0)?;
    
    Ok(exists.unwrap_or(false))
}

/// List files in a directory/prefix
/// 
/// # Arguments
/// * `bucket` - The bucket name
/// * `prefix` - The directory prefix to list (e.g., "Scripts/" or "")
/// 
/// Returns a list of file entries
pub async fn list_files(bucket: &str, prefix: &str) -> anyhow::Result<Vec<FileEntry>, anyhow::Error> {
    let bucket_name = sanitize_bucket_name(bucket);
    let normalized_prefix = if prefix.is_empty() {
        "/".to_string()
    } else {
        normalize_path(prefix)
    };
    
    let file_ref = format!(r#"f"{}:{}""#, bucket_name, normalized_prefix);
    let query = format!("RETURN file::list({})", file_ref);
    
    let mut response = DATABASE.query(&query).await?;
    let entries: Option<Vec<Value>> = response.take(0)?;
    
    match entries {
        Some(list) => {
            let file_entries: Vec<FileEntry> = list
                .into_iter()
                .filter_map(|v| {
                    // The list returns file references, we need to parse them
                    if let Some(key) = v.as_str() {
                        Some(FileEntry {
                            key: key.to_string(),
                            is_directory: key.ends_with('/'),
                            size: None,
                        })
                    } else if let Ok(entry) = serde_json::from_value::<FileEntry>(v) {
                        Some(entry)
                    } else {
                        None
                    }
                })
                .collect();
            Ok(file_entries)
        }
        None => Ok(Vec::new())
    }
}

/// Copy a file within or between buckets
pub async fn copy_file(
    src_bucket: &str, 
    src_path: &str, 
    dst_bucket: &str, 
    dst_path: &str
) -> anyhow::Result<(), anyhow::Error> {
    let src_bucket_name = sanitize_bucket_name(src_bucket);
    let dst_bucket_name = sanitize_bucket_name(dst_bucket);
    let src_normalized = normalize_path(src_path);
    let dst_normalized = normalize_path(dst_path);
    
    let src_ref = format!(r#"f"{}:{}""#, src_bucket_name, src_normalized);
    let dst_ref = format!(r#"f"{}:{}""#, dst_bucket_name, dst_normalized);
    let query = format!("RETURN file::copy({}, {})", src_ref, dst_ref);
    
    DATABASE.query(&query).await?;
    
    log::info!("Copied file from {}:{} to {}:{}", 
        src_bucket_name, src_normalized, 
        dst_bucket_name, dst_normalized);
    Ok(())
}

/// Rename/move a file
pub async fn rename_file(
    bucket: &str,
    old_path: &str,
    new_path: &str
) -> anyhow::Result<(), anyhow::Error> {
    let bucket_name = sanitize_bucket_name(bucket);
    let old_normalized = normalize_path(old_path);
    let new_normalized = normalize_path(new_path);
    
    let old_ref = format!(r#"f"{}:{}""#, bucket_name, old_normalized);
    let new_ref = format!(r#"f"{}:{}""#, bucket_name, new_normalized);
    let query = format!("RETURN file::rename({}, {})", old_ref, new_ref);
    
    DATABASE.query(&query).await?;
    
    log::info!("Renamed file from {}:{} to {}:{}", 
        bucket_name, old_normalized, 
        bucket_name, new_normalized);
    Ok(())
}

/// Sanitize a bucket name to ensure it's valid
fn sanitize_bucket_name(name: &str) -> String {
    // Remove @ and domain parts from email-style names
    let clean = name.split('@').next().unwrap_or(name);
    // Replace spaces and special characters with underscores
    clean
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}

/// Normalize a file path
fn normalize_path(path: &str) -> String {
    let mut normalized = path.replace('\\', "/");
    
    // Ensure path starts with /
    if !normalized.starts_with('/') {
        normalized = format!("/{}", normalized);
    }
    
    // Remove double slashes
    while normalized.contains("//") {
        normalized = normalized.replace("//", "/");
    }
    
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_sanitize_bucket_name() {
        assert_eq!(sanitize_bucket_name("john@example.com"), "john");
        assert_eq!(sanitize_bucket_name("John Doe"), "john_doe");
        assert_eq!(sanitize_bucket_name("user-name"), "user-name");
    }
    
    #[test]
    fn test_normalize_path() {
        assert_eq!(normalize_path("scripts/test.ps1"), "/scripts/test.ps1");
        assert_eq!(normalize_path("/scripts/test.ps1"), "/scripts/test.ps1");
        assert_eq!(normalize_path("scripts\\test.ps1"), "/scripts/test.ps1");
        assert_eq!(normalize_path("scripts//test.ps1"), "/scripts/test.ps1");
    }
}
