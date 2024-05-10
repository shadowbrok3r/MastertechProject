#[derive(Debug)]
pub enum FileBrowserError {
    IoError(std::io::Error),
    // Other error types as needed
}

impl From<std::io::Error> for FileBrowserError {
    fn from(err: std::io::Error) -> Self {
        FileBrowserError::IoError(err)
    }
}
