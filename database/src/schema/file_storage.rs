//! SurrealDB File Storage API
//! 
//! This module provides file storage operations using SurrealDB's built-in bucket/file system.
//! Requires SurrealDB 3.0.0-beta.1 or later.
//!
//! ## Bucket Setup
//! Before using file operations, define a bucket:
//! ```surql
//! DEFINE BUCKET default_bucket BACKEND "file:/path/to/storage/";
//! ```
//!
//! ## Usage Examples
//! ```rust
//! // Put a file
//! file_storage::put_file("default_bucket", "/test.txt", b"Hello World".to_vec()).await?;
//! 
//! // Get a file
//! let data = file_storage::get_file("default_bucket", "/test.txt").await?;
//! 
//! // List files
//! let entries = file_storage::list_files("default_bucket", "/").await?;
//! ```

use crate::{DATABASE, ensure_connected_or_reconnect};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use chrono::{DateTime, Utc};
use surrealdb_types::{SurrealValue, Bytes as SurrealBytes};

/// File metadata returned by file::head
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileMetadata {
    pub key: String,
    pub size: Option<u64>,
    #[serde(rename = "e_tag")]
    pub etag: Option<String>,
    pub last_modified: Option<DateTime<Utc>>,
    pub version: Option<String>,
}

/// A file entry for directory listings (matches SurrealDB's file::list output)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileEntry {
    /// The file pointer (e.g., f"bucket:/path/to/file")
    pub file: Option<String>,
    /// The key/path within the bucket
    #[serde(default)]
    pub key: String,
    /// File size in bytes
    pub size: Option<u64>,
    /// Last updated timestamp
    pub updated: Option<DateTime<Utc>>,
    /// Whether this is a directory (derived from key ending with '/')
    #[serde(default)]
    pub is_directory: bool,
}

/// A file entry for directory listings (matches SurrealDB's file::list output)
#[derive(Debug, Clone, Serialize, Deserialize, Default, SurrealValue)]
pub struct SurrealFileEntry {
    /// The file pointer (e.g., f"bucket:/path/to/file")
    pub file: surrealdb_types::File,
    /// File size in bytes
    pub size: u64,
    /// Last updated timestamp
    pub updated: surrealdb_types::Datetime,
}

impl FileEntry {
    /// Extract the filename from the file pointer or key
    pub fn filename(&self) -> String {
        let path = self.file.as_deref().unwrap_or(&self.key);
        // Parse f"bucket:/path/to/file.txt" to get "file.txt"
        path.rsplit('/').next().unwrap_or(path).to_string()
    }
    
    /// Extract the full path from the file pointer
    pub fn path(&self) -> String {
        if let Some(file) = &self.file {
            // Parse f"bucket:/path/to/file.txt" to get "/path/to/file.txt"
            if let Some(pos) = file.find(":/") {
                return file[pos + 1..].trim_end_matches('"').to_string();
            }
        }
        self.key.clone()
    }
}

/// Define or initialize a bucket
/// 
/// # Arguments
/// * `bucket_name` - The bucket name
/// * `backend` - The backend path (e.g., "file:/path/to/storage/" or "memory")
pub async fn define_bucket(bucket_name: &str, backend: &str) -> anyhow::Result<(), anyhow::Error> {
    let sanitized = sanitize_bucket_name(bucket_name);
    log::info!("About to query DB. DATABASE ptr: {:p}", &*DATABASE);
    let query = format!(r#"DEFINE BUCKET IF NOT EXISTS {} BACKEND "{}""#, sanitized, backend);
    DATABASE.query(&query).await?;
    log::info!("Defined bucket: {} with backend: {}", sanitized, backend);
    Ok(())
}

/// Initialize a bucket for a user (should be called when user signs up or first uses storage)
/// Uses a file backend at the default storage location
pub async fn init_user_bucket(username: &str) -> anyhow::Result<(), anyhow::Error> {
    let bucket_name = sanitize_bucket_name(username);
    log::info!("About to query DB. DATABASE ptr: {:p}", &*DATABASE);
    // Use memory backend for user buckets by default (can be changed to file backend)
    let query = format!(r#"DEFINE BUCKET IF NOT EXISTS {} "#, bucket_name);
    DATABASE.query(&query).await?;
    log::info!("Initialized user bucket: {}", bucket_name);
    Ok(())
}

/// Put a file into the bucket
/// 
/// # Arguments
/// * `bucket` - The bucket name (e.g., "default_bucket" or username)
/// * `path` - The path within the bucket (e.g., "/Scripts/myscript.ps1")
/// * `data` - The file contents as bytes
pub async fn put_file(bucket: &str, path: &str, data: Vec<u8>) -> anyhow::Result<(), anyhow::Error> {
    let bucket_name = sanitize_bucket_name(bucket);
    let normalized_path = normalize_path(path);
    let data_len = data.len();
    log::info!("file_storage::put_file -> Starting upload: {}:{} ({} bytes)", bucket_name, normalized_path, data_len);
    
    // Ensure connection is alive before querying
    if let Err(e) = ensure_connected_or_reconnect().await {
        log::warn!("file_storage::put_file -> Connection check failed: {e}");
    }
    
    // Use tokio::time::timeout to prevent hanging on large files
    let timeout_duration = std::time::Duration::from_secs(60); // 60 second timeout for uploads
    
    // SurrealQL method syntax: f"bucket:/path".put(data)
    let query = format!(r#"f"{}:{}".put($data)"#, bucket_name, normalized_path);
    log::debug!("file_storage::put_file -> Query: {}", query);
    
    let result = tokio::time::timeout(
        timeout_duration,
        DATABASE
            .query(&query)
            .bind(("data", data))
    ).await;
    
    match result {
        Ok(Ok(_)) => {
            log::info!("file_storage::put_file -> SUCCESS: {}:{} ({} bytes)", bucket_name, normalized_path, data_len);
            Ok(())
        }
        Ok(Err(e)) => {
            log::error!("file_storage::put_file -> FAILED: {}:{} - {}", bucket_name, normalized_path, e);
            Err(e.into())
        }
        Err(_) => {
            log::error!("file_storage::put_file -> TIMEOUT after {} seconds: {}:{} ({} bytes)", 
                timeout_duration.as_secs(), bucket_name, normalized_path, data_len);
            Err(anyhow::anyhow!("Upload timeout after {} seconds for file {} ({} bytes)", 
                timeout_duration.as_secs(), normalized_path, data_len))
        }
    }
}

/// Put a file only if it doesn't already exist
pub async fn put_file_if_not_exists(bucket: &str, path: &str, data: Vec<u8>) -> anyhow::Result<bool, anyhow::Error> {
    let bucket_name = sanitize_bucket_name(bucket);
    let normalized_path = normalize_path(path);
    
    // SurrealQL method syntax: f"bucket:/path".put_if_not_exists(data)
    let query = format!(r#"f"{}:{}".put_if_not_exists($data)"#, bucket_name, normalized_path);
    log::info!("About to query DB. DATABASE ptr: {:p}", &*DATABASE);
    DATABASE
        .query(&query)
        .bind(("data", data))
        .await?;
    
    log::info!("file_storage::put_file_if_not_exists -> {}:{}", bucket_name, normalized_path);
    Ok(true)
}

/// Get a file from the bucket
/// 
/// Returns None if the file doesn't exist
pub async fn get_file(bucket: &str, path: &str) -> anyhow::Result<Option<Vec<u8>>, anyhow::Error> {
    let bucket_name = sanitize_bucket_name(bucket);
    let normalized_path = normalize_path(path);
    log::info!("About to query DB. DATABASE ptr: {:p}", &*DATABASE);
    
    // Ensure connection is alive before querying
    if let Err(e) = ensure_connected_or_reconnect().await {
        log::warn!("file_storage::get_file -> Connection check failed: {e}");
    }
    
    // SurrealQL method syntax: f"bucket:/path".get() returns bytes type
    // Use RETURN to wrap the result properly
    let query = format!(r#"RETURN f"{}:{}".get()"#, bucket_name, normalized_path);
    
    let mut response = DATABASE.query(&query).await?;
    // Use surrealdb_types::Bytes for proper deserialization of SurrealDB bytes type
    let data: Option<SurrealBytes> = response.take(0)?;
    
    // Convert SurrealBytes to Vec<u8>
    let result = data.map(|b| b.into_inner().to_vec());
    
    log::debug!("file_storage::get_file -> {}:{} (found: {})", bucket_name, normalized_path, result.is_some());
    Ok(result)
}

/// Get a file as a string (convenience method for text files)
pub async fn get_file_as_string(bucket: &str, path: &str) -> anyhow::Result<Option<String>, anyhow::Error> {
    let bucket_name = sanitize_bucket_name(bucket);
    let normalized_path = normalize_path(path);
    log::info!("About to query DB. DATABASE ptr: {:p}", &*DATABASE);
    
    // Ensure connection is alive before querying
    if let Err(e) = ensure_connected_or_reconnect().await {
        log::warn!("file_storage::get_file_as_string -> Connection check failed: {e}");
    }
    
    // SurrealQL: <string>f"bucket:/path".get() casts bytes to string
    let query = format!(r#"<string>f"{}:{}".get()"#, bucket_name, normalized_path);
    
    let mut response = DATABASE.query(&query).await?;
    let data: Option<String> = response.take(0)?;
    
    Ok(data)
}

/// Get file metadata without downloading the content
pub async fn head_file(bucket: &str, path: &str) -> anyhow::Result<Option<FileMetadata>, anyhow::Error> {
    let bucket_name = sanitize_bucket_name(bucket);
    let normalized_path = normalize_path(path);
    log::info!("About to query DB. DATABASE ptr: {:p}", &*DATABASE);
    // SurrealQL method syntax: f"bucket:/path".head()
    let query = format!(r#"f"{}:{}".head()"#, bucket_name, normalized_path);
    
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

/// Delete a file from the bucket
pub async fn delete_file(bucket: &str, path: &str) -> anyhow::Result<(), anyhow::Error> {
    let bucket_name = sanitize_bucket_name(bucket);
    let normalized_path = normalize_path(path);
    log::info!("About to query DB. DATABASE ptr: {:p}", &*DATABASE);
    // SurrealQL method syntax: f"bucket:/path".delete()
    let query = format!(r#"f"{}:{}".delete()"#, bucket_name, normalized_path);
    
    DATABASE.query(&query).await?;
    
    log::info!("file_storage::delete_file -> {}:{}", bucket_name, normalized_path);
    Ok(())
}

/// Check if a file exists
pub async fn file_exists(bucket: &str, path: &str) -> anyhow::Result<bool, anyhow::Error> {
    let bucket_name = sanitize_bucket_name(bucket);
    let normalized_path = normalize_path(path);
    log::info!("About to query DB. DATABASE ptr: {:p}", &*DATABASE);
    // SurrealQL method syntax: f"bucket:/path".exists()
    let query = format!(r#"f"{}:{}".exists()"#, bucket_name, normalized_path);
    
    let mut response = DATABASE.query(&query).await?;
    let exists: Option<bool> = response.take(0)?;
    
    Ok(exists.unwrap_or(false))
}

/// List files in a bucket
/// 
/// # Arguments
/// * `bucket` - The bucket name
/// * `prefix` - Optional prefix to filter results (e.g., "scripts" to only show files starting with "scripts")
/// 
/// # Returns
/// A list of file entries with metadata (file pointer, size, updated timestamp)
/// 
/// # Example
/// ```rust
/// let files = file_storage::list_files("default_bucket", "").await?;
/// for entry in files {
///     println!("File: {} ({} bytes)", entry.filename(), entry.size.unwrap_or(0));
/// }
/// ```
pub async fn list_files(bucket: &str, prefix: &str) -> anyhow::Result<Vec<FileEntry>, anyhow::Error> {
    let bucket_name = sanitize_bucket_name(bucket);
    
    // Ensure connection is alive before querying
    if let Err(e) = ensure_connected_or_reconnect().await {
        log::warn!("file_storage::list_files -> Connection check failed: {e}");
    }
    
    // file::list returns array<object> with: { file: File, size: u64, updated: Datetime }
    // Using SurrealFileEntry with native surrealdb_types for proper deserialization
    // See: https://surrealdb.com/docs/3.x/surrealql/functions/database/file#filelist
    let query = if prefix.is_empty() || prefix == "/" {
        format!(r#"RETURN file::list("{}")"#, bucket_name)
    } else {
        let clean_prefix = prefix.trim_matches('/');
        format!(r#"RETURN file::list("{}", {{ prefix: "{}" }})"#, bucket_name, clean_prefix)
    };
    
    log::debug!("file_storage::list_files -> query: {}", query);
    
    let mut response = DATABASE.query(&query).await?;
    let entries: Vec<SurrealFileEntry> = response.take(0)?;
    
    if entries.is_empty() {
        log::info!("file_storage::list_files -> No files in bucket '{}' (prefix: '{}')", 
            bucket_name, prefix);
        return Ok(Vec::new());
    }
    
    // Convert SurrealFileEntry to FileEntry
    let file_entries: Vec<FileEntry> = entries
        .into_iter()
        .map(|entry| {
            let key = entry.file.key.clone();
            let is_directory = key.ends_with('/');
            let file_ptr = format!("{}:{}", entry.file.bucket, entry.file.key);
            FileEntry {
                file: Some(file_ptr),
                key,
                size: Some(entry.size),
                updated: Some(entry.updated.into_inner()),
                is_directory,
            }
        })
        .collect();
    
    log::info!("file_storage::list_files -> {} files in bucket '{}' (prefix: '{}')", 
        file_entries.len(), bucket_name, prefix);
    Ok(file_entries)
}

/// List files with additional options (limit, start cursor)
pub async fn list_files_with_options(
    bucket: &str, 
    prefix: Option<&str>,
    limit: Option<u32>,
    start: Option<&str>,
) -> anyhow::Result<Vec<FileEntry>, anyhow::Error> {
    let bucket_name = sanitize_bucket_name(bucket);
    
    // Build the options object
    let mut options = Vec::new();
    if let Some(p) = prefix {
        let clean = p.trim_matches('/');
        if !clean.is_empty() {
            options.push(format!(r#"prefix: "{}""#, clean));
        }
    }
    if let Some(l) = limit {
        options.push(format!("limit: {}", l));
    }
    if let Some(s) = start {
        options.push(format!(r#"start: "{}""#, s));
    }
    
    // Use native SurrealDB types for proper deserialization
    let query = if options.is_empty() {
        format!(r#"RETURN file::list("{}")"#, bucket_name)
    } else {
        format!(r#"RETURN file::list("{}", {{ {} }})"#, bucket_name, options.join(", "))
    };
    
    log::debug!("file_storage::list_files_with_options -> query: {}", query);
    
    let mut response = DATABASE.query(&query).await?;
    let entries: Vec<SurrealFileEntry> = response.take(0)?;
    
    if entries.is_empty() {
        return Ok(Vec::new());
    }
    
    // Convert SurrealFileEntry to FileEntry
    let file_entries: Vec<FileEntry> = entries
        .into_iter()
        .map(|entry| {
            let key = entry.file.key.clone();
            let is_directory = key.ends_with('/');
            let file_ptr = format!("{}:{}", entry.file.bucket, entry.file.key);
            FileEntry {
                file: Some(file_ptr),
                key,
                size: Some(entry.size),
                updated: Some(entry.updated.into_inner()),
                is_directory,
            }
        })
        .collect();
    Ok(file_entries)
}

/// Extract the key/path from a SurrealDB file pointer string
/// Handles both formats:
/// - Raw file pointer: `f"bucket:/path/to/file.txt"` -> `/path/to/file.txt`
/// - String-cast file: `bucket:/path/to/file.txt` -> `/path/to/file.txt`
#[cfg(test)]
fn extract_key_from_file_pointer(file_ptr: &str) -> String {
    // Remove the f" prefix and trailing quotes if present (raw file pointer)
    let clean = file_ptr
        .trim_start_matches("f\"")
        .trim_start_matches("f'")
        .trim_end_matches('"')
        .trim_end_matches('\'');
    
    // Find the :/ separator and extract path after it
    // e.g., "logan_lees:/test.txt" -> "/test.txt"
    if let Some(pos) = clean.find(":/") {
        clean[pos + 1..].to_string()
    } else {
        clean.to_string()
    }
}

/// Copy a file to a new location (within the same bucket)
/// Note: The destination is just the new filename/path, not a full file reference
pub async fn copy_file(
    bucket: &str, 
    src_path: &str, 
    dst_path: &str
) -> anyhow::Result<(), anyhow::Error> {
    let bucket_name = sanitize_bucket_name(bucket);
    let src_normalized = normalize_path(src_path);
    // For copy, the destination is just the new key name (without bucket prefix)
    let dst_key = dst_path.trim_start_matches('/');
    
    // SurrealQL method syntax: f"bucket:/path".copy("new_name")
    let query = format!(r#"f"{}:{}".copy("{}")"#, bucket_name, src_normalized, dst_key);
    
    DATABASE.query(&query).await?;
    
    log::info!("file_storage::copy_file -> {}:{} to {}", bucket_name, src_normalized, dst_key);
    Ok(())
}

/// Copy a file only if the destination doesn't exist
pub async fn copy_file_if_not_exists(
    bucket: &str, 
    src_path: &str, 
    dst_path: &str
) -> anyhow::Result<(), anyhow::Error> {
    let bucket_name = sanitize_bucket_name(bucket);
    let src_normalized = normalize_path(src_path);
    let dst_key = dst_path.trim_start_matches('/');
    
    // SurrealQL method syntax: f"bucket:/path".copy_if_not_exists("new_name")
    let query = format!(r#"f"{}:{}".copy_if_not_exists("{}")"#, bucket_name, src_normalized, dst_key);
    
    DATABASE.query(&query).await?;
    
    log::info!("file_storage::copy_file_if_not_exists -> {}:{} to {}", bucket_name, src_normalized, dst_key);
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
    // For rename, the destination is just the new key name (without bucket prefix)
    let new_key = new_path.trim_start_matches('/');
    
    // SurrealQL method syntax: f"bucket:/path".rename("new_name")
    let query = format!(r#"f"{}:{}".rename("{}")"#, bucket_name, old_normalized, new_key);
    
    DATABASE.query(&query).await?;
    
    log::info!("file_storage::rename_file -> {}:{} to {}", bucket_name, old_normalized, new_key);
    Ok(())
}

/// Rename a file only if the destination doesn't exist
pub async fn rename_file_if_not_exists(
    bucket: &str,
    old_path: &str,
    new_path: &str
) -> anyhow::Result<(), anyhow::Error> {
    let bucket_name = sanitize_bucket_name(bucket);
    let old_normalized = normalize_path(old_path);
    let new_key = new_path.trim_start_matches('/');
    
    // SurrealQL method syntax: f"bucket:/path".rename_if_not_exists("new_name")
    let query = format!(r#"f"{}:{}".rename_if_not_exists("{}")"#, bucket_name, old_normalized, new_key);
    
    DATABASE.query(&query).await?;
    
    log::info!("file_storage::rename_file_if_not_exists -> {}:{} to {}", bucket_name, old_normalized, new_key);
    Ok(())
}

/// Get the bucket name from a file pointer
pub async fn get_bucket_name(file_pointer: &str) -> anyhow::Result<String, anyhow::Error> {
    let query = format!(r#"file::bucket({})"#, file_pointer);
    let mut response = DATABASE.query(&query).await?;
    let bucket: Option<String> = response.take(0)?;
    bucket.ok_or_else(|| anyhow::anyhow!("Could not get bucket name from file pointer"))
}

/// Get the key (path) from a file pointer
pub async fn get_file_key(file_pointer: &str) -> anyhow::Result<String, anyhow::Error> {
    let query = format!(r#"file::key({})"#, file_pointer);
    let mut response = DATABASE.query(&query).await?;
    let key: Option<String> = response.take(0)?;
    key.ok_or_else(|| anyhow::anyhow!("Could not get key from file pointer"))
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

// ============================================================================
// SurrealFile - Convenient file handle for easy operations
// ============================================================================

/// A handle to a file in a SurrealDB bucket, providing a convenient API for file operations
/// 
/// # Example
/// ```rust
/// let file = SurrealFile::new("default_bucket", "/scripts/myscript.ps1");
/// 
/// // Write content
/// file.put(b"Write-Host 'Hello World'".to_vec()).await?;
/// 
/// // Read content
/// if let Some(data) = file.get().await? {
///     let content = String::from_utf8_lossy(&data);
///     println!("Content: {}", content);
/// }
/// 
/// // Check if exists
/// if file.exists().await? {
///     println!("File exists!");
/// }
/// ```
#[derive(Debug, Clone)]
pub struct SurrealFile {
    bucket: String,
    path: String,
}

impl SurrealFile {
    /// Create a new file handle
    pub fn new(bucket: &str, path: &str) -> Self {
        Self {
            bucket: sanitize_bucket_name(bucket),
            path: normalize_path(path),
        }
    }
    
    /// Get the bucket name
    pub fn bucket(&self) -> &str {
        &self.bucket
    }
    
    /// Get the file path
    pub fn path(&self) -> &str {
        &self.path
    }
    
    /// Get the filename (last component of the path)
    pub fn filename(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }
    
    /// Get the SurrealQL file reference string (e.g., `f"bucket:/path"`)
    pub fn file_ref(&self) -> String {
        format!(r#"f"{}:{}""#, self.bucket, self.path)
    }
    
    /// Put data into the file (overwrites if exists)
    pub async fn put(&self, data: Vec<u8>) -> anyhow::Result<()> {
        put_file(&self.bucket, &self.path, data).await
    }
    
    /// Put string data into the file
    pub async fn put_string(&self, content: &str) -> anyhow::Result<()> {
        put_file(&self.bucket, &self.path, content.as_bytes().to_vec()).await
    }
    
    /// Put data only if file doesn't exist
    pub async fn put_if_not_exists(&self, data: Vec<u8>) -> anyhow::Result<bool> {
        put_file_if_not_exists(&self.bucket, &self.path, data).await
    }
    
    /// Get file contents as bytes
    pub async fn get(&self) -> anyhow::Result<Option<Vec<u8>>> {
        get_file(&self.bucket, &self.path).await
    }
    
    /// Get file contents as string
    pub async fn get_string(&self) -> anyhow::Result<Option<String>> {
        get_file_as_string(&self.bucket, &self.path).await
    }
    
    /// Delete the file
    pub async fn delete(&self) -> anyhow::Result<()> {
        delete_file(&self.bucket, &self.path).await
    }
    
    /// Check if file exists
    pub async fn exists(&self) -> anyhow::Result<bool> {
        file_exists(&self.bucket, &self.path).await
    }
    
    /// Get file metadata
    pub async fn head(&self) -> anyhow::Result<Option<FileMetadata>> {
        head_file(&self.bucket, &self.path).await
    }
    
    /// Copy to a new path
    pub async fn copy_to(&self, new_path: &str) -> anyhow::Result<()> {
        copy_file(&self.bucket, &self.path, new_path).await
    }
    
    /// Rename/move to a new path
    pub async fn rename_to(&self, new_path: &str) -> anyhow::Result<()> {
        rename_file(&self.bucket, &self.path, new_path).await
    }
}

// ============================================================================
// SurrealBucket - Convenient bucket handle for directory operations
// ============================================================================

/// A handle to a SurrealDB bucket for listing and managing files
/// 
/// # Example
/// ```rust
/// let bucket = SurrealBucket::new("default_bucket");
/// 
/// // List all files
/// for entry in bucket.list("").await? {
///     println!("{}: {} bytes", entry.filename(), entry.size.unwrap_or(0));
/// }
/// 
/// // Get a file handle
/// let file = bucket.file("/test.txt");
/// file.put_string("Hello World").await?;
/// ```
#[derive(Debug, Clone)]
pub struct SurrealBucket {
    name: String,
}

impl SurrealBucket {
    /// Create a new bucket handle
    pub fn new(name: &str) -> Self {
        Self {
            name: sanitize_bucket_name(name),
        }
    }
    
    /// Get the bucket name
    pub fn name(&self) -> &str {
        &self.name
    }
    
    /// Define/initialize this bucket with the given backend
    pub async fn define(&self, backend: &str) -> anyhow::Result<()> {
        define_bucket(&self.name, backend).await
    }
    
    /// Define this bucket with a memory backend
    pub async fn define_memory(&self) -> anyhow::Result<()> {
        define_bucket(&self.name, "memory").await
    }
    
    /// Define this bucket with a file backend at the given path
    pub async fn define_file(&self, path: &str) -> anyhow::Result<()> {
        define_bucket(&self.name, &format!("file:{}", path)).await
    }
    
    /// Get a file handle for a path within this bucket
    pub fn file(&self, path: &str) -> SurrealFile {
        SurrealFile::new(&self.name, path)
    }
    
    /// List files in this bucket
    pub async fn list(&self, prefix: &str) -> anyhow::Result<Vec<FileEntry>> {
        list_files(&self.name, prefix).await
    }
    
    /// List files with options
    pub async fn list_with_options(
        &self,
        prefix: Option<&str>,
        limit: Option<u32>,
        start: Option<&str>,
    ) -> anyhow::Result<Vec<FileEntry>> {
        list_files_with_options(&self.name, prefix, limit, start).await
    }
}

// ============================================================================
// Default bucket constant
// ============================================================================

/// The default bucket name used for general file storage
pub const DEFAULT_BUCKET: &str = "default_bucket";

/// Get a handle to the default bucket
pub fn default_bucket() -> SurrealBucket {
    SurrealBucket::new(DEFAULT_BUCKET)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_sanitize_bucket_name() {
        assert_eq!(sanitize_bucket_name("john@example.com"), "john");
        assert_eq!(sanitize_bucket_name("John Doe"), "john_doe");
        assert_eq!(sanitize_bucket_name("user-name"), "user-name");
        assert_eq!(sanitize_bucket_name("default_bucket"), "default_bucket");
    }
    
    #[test]
    fn test_normalize_path() {
        assert_eq!(normalize_path("scripts/test.ps1"), "/scripts/test.ps1");
        assert_eq!(normalize_path("/scripts/test.ps1"), "/scripts/test.ps1");
        assert_eq!(normalize_path("scripts\\test.ps1"), "/scripts/test.ps1");
        assert_eq!(normalize_path("scripts//test.ps1"), "/scripts/test.ps1");
        assert_eq!(normalize_path("test.txt"), "/test.txt");
    }
    
    #[test]
    fn test_extract_key_from_file_pointer() {
        assert_eq!(
            extract_key_from_file_pointer(r#"f"bucket:/path/to/file.txt""#), 
            "/path/to/file.txt"
        );
        assert_eq!(
            extract_key_from_file_pointer(r#"f"default_bucket:/test.txt""#), 
            "/test.txt"
        );
        assert_eq!(
            extract_key_from_file_pointer("bucket:/simple.txt"), 
            "/simple.txt"
        );
    }
    
    #[test]
    fn test_surreal_file_creation() {
        let file = SurrealFile::new("default_bucket", "/test/file.txt");
        assert_eq!(file.bucket(), "default_bucket");
        assert_eq!(file.path(), "/test/file.txt");
        assert_eq!(file.filename(), "file.txt");
        assert_eq!(file.file_ref(), r#"f"default_bucket:/test/file.txt""#);
    }
    
    #[test]
    fn test_file_entry_methods() {
        let entry = FileEntry {
            file: Some(r#"f"bucket:/path/to/document.pdf""#.to_string()),
            key: String::new(),
            size: Some(1024),
            updated: None,
            is_directory: false,
        };
        assert_eq!(entry.filename(), "document.pdf");
        assert_eq!(entry.path(), "/path/to/document.pdf");
    }
}
