use std::{env, path::PathBuf};

use crossbeam::channel;
use fs_extra::dir::get_size;
use log::debug;
use tokio::fs;

use crate::tabs::file_browser::{io::MetaData, read_folder};

use super::{file_copy::CopyBuilder, FileBrowser};

#[derive(Debug)]
pub enum Command {
    Copy(Vec<PathBuf>, PathBuf, channel::Sender<u64>),
    Move(PathBuf, PathBuf),
    Delete(PathBuf),
    Rename(PathBuf, PathBuf),
    CreateDirectory,
    Folder,
    Refresh,
    Select(PathBuf),
    UpDirectory,
    OpenPath(PathBuf),
    ReadDirectory(PathBuf),
    ReadMetadata(PathBuf),
    Home,
    GetDrives,
}

impl FileBrowser{
    pub async fn run_command(&mut self, command: Command) {
        match command{
            Command::Select(file) => self.select(file),

            Command::Folder => self.selected_item = Some(self.get_folder().to_owned()),
            
            Command::Refresh => self.refresh_contents(),

            Command::UpDirectory => {if self.path.pop() {self.refresh_contents()}},

            Command::Home => {
                self.path = env::current_dir().unwrap_or_default();
                self.refresh_contents();
            },

            Command::CreateDirectory => {
                let mut path = self.path.clone();
                let name = match self.filename_edit.is_empty() {
                    true => "New folder",
                    false => &self.filename_edit,
                };
                path.push(name);

                match fs::create_dir(&path).await {
                    Ok(_) => {
                        self.refresh_contents();
                        self.select(path);
                    }
                    Err(err) => println!("Error while creating directory: {err}"),
                }
            },

            Command::Copy(source, destination, progress_tx) => {
                std::thread::spawn(move ||{
                    for entry in source{
                        CopyBuilder::new(entry, destination.clone())
                            .overwrite_if_newer(true)
                            .overwrite_if_size_differs(true)
                            .with_exclude_filter(".sys")
                            .with_exclude_filter(".dat")
                            .run(progress_tx.clone())
                            .unwrap_or(());
                    }
                }); // copy_files(source, &destination, progress_tx).await.unwrap();
            },

            Command::Move(source, destination) => {
                println!("Command::Move");
                if let Err(err) = fs::rename(&source, &destination).await {
                    debug!("error: {err:?}");
                    //let _ = response_sender.try_send(Response::Error(FileBrowserError::Io(err)));
                } else {
                    //let _ = response_sender.try_send(Response::Success(format!("Successfully moved from {:?} to {:?}", source, destination)));
                }
            
            },

            Command::Delete(path) => {
                println!("Command::Delete");
                if let Err(_err) = fs::remove_dir_all(&path).await {
                    //let _ = response_sender.try_send(Response::Error(FileBrowserError::Io(err)));
                } else {
                    //let _ = response_sender.try_send(Response::Success(format!("Successfully deleted {:?}", path)));
                }
            },

            Command::Rename(from, to) => {
                match fs::rename(from, &to).await {
                    Ok(_) => {
                        self.refresh_contents();
                        self.select(to);
                    }
                    Err(err) => println!("Error while renaming: {err}"),
                }
            },

            Command::OpenPath(path) => {
                self.select(path);
                self.open_path();
            },

            Command::ReadDirectory(path) => {
                puffin::profile_scope!("Command::ReadDirectory");
                let new_contents = read_folder(
                    &path,
                    self.depth,
                    self.show_hidden,
                );
                self.dir_contents.borrow_mut().insert(path, new_contents);
            }

            Command::ReadMetadata(path) => {
                let sender = self.metadata_tx.clone();
                let cloned_path = path.clone();
                let clone_path1 = path.clone();

                // Spawn the appropriate async task depending on whether the path is a directory or a file.
                let read_metadata_task = if path.is_dir() {
                    tokio::spawn(async move {
                        get_size(cloned_path).unwrap_or(0)
                    })
                } else if path.is_file() {
                    tokio::spawn(async move {
                        tokio::fs::metadata(&cloned_path).await.unwrap().len()
                    })
                } else {return;};

                tokio::select! {
                    result = read_metadata_task => {
                        match result {
                            Ok(path_size) => { // Send the result through the channel.
                                if sender.try_send(path_size).is_err() { println!("Error sending metadata");}
                                
                                if path.is_dir() { // Insert the metadata into the appropriate HashMap.
                                    self.folder_metadata.borrow_mut().insert(clone_path1.clone(),
                                        MetaData { path_size });
                                } else {
                                    self.file_metadata.borrow_mut().insert(clone_path1.clone(),
                                        MetaData { path_size });
                                }
                            },
                            Err(e) => println!("Error reading metadata: {:?}", e),
                        }
                    }
                }
            },
            Command::GetDrives => self.get_drives()
        }
    }

}