use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct FileItem {
    pub name: String,
    pub path: PathBuf,
    pub is_directory: bool,
    // Other relevant fields like size, date modified, etc.
}
