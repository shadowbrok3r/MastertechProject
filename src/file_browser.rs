use std::io::{stdin, stdout, Write};
use std::path::PathBuf;
use tokio::{task, fs};
use walkdir::WalkDir;
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel, UnboundedReceiver};
use std::io::{Error, Result};
use thiserror::Error;
use std::future::Future;
use std::pin::Pin;
use async_recursion::async_recursion;

#[derive(Debug)]
pub enum Response {
    DirectoryListing(Directory),
    Success(String),
    Error(FileBrowserError),
}

#[derive(Error, Debug)]
pub enum FileBrowserError {
    #[error("I/O error")]
    Io(#[from] Error),
    #[error("WalkDir error")]
    WalkDir(#[from] walkdir::Error),
    #[error("Other error: {0}")]
    Other(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Directory {
    pub path: PathBuf,
    pub children: Vec<Directory>,
    pub files: Vec<PathBuf>,
}

#[derive(Debug)]
pub enum Command {
    Copy(PathBuf, PathBuf),
    Move(PathBuf, PathBuf),
    Delete(PathBuf),
    Rename(PathBuf, PathBuf),
    ListDir(PathBuf, usize),
}

pub struct CommandControl {
    command_sender: UnboundedSender<Command>,
    response_receiver: UnboundedReceiver<Response>,
}

impl CommandControl {
    pub fn new() -> Self {
        let (command_sender, command_receiver) = unbounded_channel::<Command>();
        let (response_sender, response_receiver) = unbounded_channel::<Response>();

        tokio::spawn(async move {
            file_browsing(command_receiver, response_sender).await;
        });

        Self {
            command_sender,
            response_receiver,
        }
    }

    pub fn get_sender(&self) -> UnboundedSender<Command> {
        self.command_sender.clone()
    }

    pub fn get_receiver(&mut self) -> &mut UnboundedReceiver<Response> {
        &mut self.response_receiver
    }
}

pub async fn file_browsing(mut command_receiver: UnboundedReceiver<Command>, response_sender: UnboundedSender<Response>) {
    while let Some(command) = command_receiver.recv().await {
        match command {
            Command::ListDir(path, depth) => {
                match read_directory_boxed(path.clone(), depth).await {
                    Ok(directory) => {
                        let _ = response_sender.send(Response::DirectoryListing(directory));
                    },
                    Err(err) => {
                        let _ = response_sender.send(Response::Error(FileBrowserError::Other(err.to_string())));
                    }
                }
            },
            Command::Copy(source, destination) => {
                if let Err(err) = fs::copy(&source, &destination).await {
                    let _ = response_sender.send(Response::Error(FileBrowserError::Io(err)));
                } else {
                    let _ = response_sender.send(Response::Success(format!("Successfully copied from {:?} to {:?}", source, destination)));
                }
            },
            Command::Move(source, destination) => {
                if let Err(err) = fs::rename(&source, &destination).await {
                    let _ = response_sender.send(Response::Error(FileBrowserError::Io(err)));
                } else {
                    let _ = response_sender.send(Response::Success(format!("Successfully moved from {:?} to {:?}", source, destination)));
                }
            },
            Command::Delete(path) => {
                if let Err(err) = fs::remove_dir_all(&path).await {
                    let _ = response_sender.send(Response::Error(FileBrowserError::Io(err)));
                } else {
                    let _ = response_sender.send(Response::Success(format!("Successfully deleted {:?}", path)));
                }
            },
            Command::Rename(source, destination) => {
                if let Err(err) = fs::rename(&source, &destination).await {
                    let _ = response_sender.send(Response::Error(FileBrowserError::Io(err)));
                } else {
                    let _ = response_sender.send(Response::Success(format!("Successfully renamed from {:?} to {:?}", source, destination)));
                }
            },
        }
    }
}





pub fn read_directory_boxed(path: PathBuf, depth: usize) -> Pin<Box<dyn Future<Output = std::result::Result<Directory, Error>> + Send>> {
    let path_clone = path.clone();
    Box::pin(async move {
        let mut children = Vec::new();
        let mut files = Vec::new();
        if depth > 0 {
            let mut dir = fs::read_dir(path_clone).await?;
            while let Some(child) = dir.next_entry().await? {
                let child_path = child.path();
                if child.metadata().await?.is_dir() {
                    let child_dir = read_directory_boxed(child_path, depth - 1).await?;
                    children.push(child_dir);
                } else {
                    files.push(child_path);
                }
            }
        }
        Ok(Directory { path, children, files })
    })
}

