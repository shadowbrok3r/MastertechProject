use std::path::PathBuf;
use tokio::fs;
use async_trait::async_trait;

#[async_trait]
pub trait FileSystemAdapter {
    // Asynchronously lists the contents of the directory at the given path.
    // Returns a vector of FileItem or an IO Error.
    async fn list_directory(&self, path: &PathBuf) -> Result<Vec<FileItem>, std::io::Error>;

    // Asynchronously copies a file from 'src' to 'dest'.
    // Returns an Ok(()) upon success or an IO Error.
    async fn copy(&self, src: &PathBuf, dest: &PathBuf) -> Result<(), std::io::Error>;
    
    // Other async methods can be defined here...
}


pub struct LocalFileSystemAdapter;

#[async_trait]
impl FileSystemAdapter for LocalFileSystemAdapter {
    // Implementation of the async list_directory method.
    // It reads the directory contents and returns a list of FileItems.
    async fn list_directory(&self, path: &PathBuf) -> Result<Vec<FileItem>, std::io::Error> {
        let mut items = Vec::new();
        let mut dir = fs::read_dir(path).await?;
        while let Some(entry) = dir.next_entry().await? {
            let path = entry.path();
            items.push(FileItem {
                name: entry.file_name().into_string().unwrap_or_default(),
                path,
                is_directory: entry.file_type().await?.is_dir(),
            });
        }
        Ok(items)
    }

    // Asynchronously copies a file from 'src' to 'dest'.
    // This is a simple wrapper around Tokio's async copy function.
    async fn copy(&self, src: &PathBuf, dest: &PathBuf) -> Result<(), std::io::Error> {
        fs::copy(src, dest).await.map(|_| ())
    }
    // Additional methods can be implemented similarly...
}
