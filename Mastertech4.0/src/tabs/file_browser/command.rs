use std::{env, fs::create_dir, path::PathBuf, process::Stdio};
use chrono::Utc;
use tokio::{fs, io::{AsyncBufReadExt, BufReader}};
use crossbeam::channel::Sender;
use fs_extra::dir::get_size;
use log::{debug, error};
// use sysinfo::System;
use tracing::info;

use crate::tabs::file_browser::{io::MetaData, read_folder};

use super::{file_copy::CopyBuilder, FileBrowser};

#[derive(Debug)]
pub enum Command {
    Copy(Vec<PathBuf>, PathBuf, Sender<u64>, Sender<String>),
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
                    Err(err) => log::error!("Error while creating directory: {err}"),
                }
            },

            Command::Copy(source, destination, progress_tx, copied_items_tx) => {
                info!("Source: {source:?} // Destination: {destination:?}");
                for path in source.iter() {
                    let mut current_destination = destination.clone();
                    if let Some(folder_name) = path.file_name(){
                        info!("Single path: {:?}", folder_name);
                        current_destination = current_destination.join(folder_name);
                        if !current_destination.exists() {
                            info!("Making a directory here: {:?}", current_destination.clone());
                            info!("Creating destination directory");
                            let create_dest_dir = create_dir(current_destination.clone());
                            if let Err(e) = create_dest_dir {
                                error!("Error creating dir: {e:?}");
                            }
                        }
                    }
                }

                info!("Running copy operations, pasting here: {:?}", destination.clone());
                // spawn(async move {
                //     run_robocopy(source.get(0).unwrap(), &destination, log_output).await;
                // });
                let destinations_to_create = source.clone().iter().map(|d| 
                    if let Some(folder_name) = d.file_name() { destination.join(folder_name) } else { destination.clone() }
                ).collect::<Vec<PathBuf>>();
                
                std::thread::spawn(move ||{
                    for (src, dest) in source.iter().zip(destinations_to_create.iter()) {
                        CopyBuilder::new(src, dest.clone())
                            .overwrite_if_newer(true)
                            .overwrite_if_size_differs(true)
                            .with_exclude_filter(".sys")
                            .with_exclude_filter(".dat")
                            .run(progress_tx.clone(), copied_items_tx.clone())
                            .unwrap_or(());
                    }
                }); // copy_files(source, &destination, progress_tx).await.unwrap();
            },

            Command::Move(source, destination) => {
                log::info!("Command::Move");
                if let Err(err) = fs::rename(&source, &destination).await {
                    debug!("error: {err:?}");
                    //let _ = response_sender.try_send(Response::Error(FileBrowserError::Io(err)));
                } else {
                    //let _ = response_sender.try_send(Response::Success(format!("Successfully moved from {:?} to {:?}", source, destination)));
                }
            
            },

            Command::Delete(path) => {
                log::info!("Command::Delete");
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
                    Err(err) => log::error!("Error while renaming: {err}"),
                }
            },

            Command::OpenPath(path) => {
                self.select(path);
                self.open_path();
            },

            Command::ReadDirectory(path) => {
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
                                if sender.try_send(path_size).is_err() { log::error!("Error sending metadata");}
                                
                                if path.is_dir() { // Insert the metadata into the appropriate HashMap.
                                    self.folder_metadata.borrow_mut().insert(clone_path1.clone(),
                                        MetaData { path_size });
                                } else {
                                    self.file_metadata.borrow_mut().insert(clone_path1.clone(),
                                        MetaData { path_size });
                                }
                            },
                            Err(e) => log::error!("Error reading metadata: {:?}", e),
                        }
                    }
                }
            },
            Command::GetDrives => self.get_drives()
        }
    }

}

pub async fn run_robocopy(
    source: &PathBuf, 
    destination: &PathBuf,
    tx: Sender<Vec<u8>>
) -> anyhow::Result<(), anyhow::Error> {
    if !source.exists() && !destination.exists() {
        return Err(
            anyhow::anyhow!(
                "Either the source or the destination do not exist: Source: {:?} Dest: {:?}",
                !source.exists(), 
                !destination.exists()
            )
        );
    }

    let source_user_name = source.file_name().clone().unwrap_or_default();
    let backup_folder = destination.join("Desktop").join("UsersBackup").join(source_user_name);
    std::fs::create_dir_all(&backup_folder)?;

    log::info!(
        "Source: {:?} Dest: {:?}source_user_name: {:?}\nbackup_folder: {:?}",
        source, 
        destination,
        source_user_name,
        backup_folder,
    );

    let mut process = tokio::process::Command::new("robocopy")
        .arg(source)
        .arg(backup_folder)
        .arg("*.*")
        .arg("/S")
        .arg("/E")
        .arg("/COPY:DAT")
        .arg("/DCOPY:DAT")
        .arg("/R:0")
        .arg("/W:30")
        .arg("/ZB")
        .arg("/bytes")
        .arg("/np")
        .arg(format!("/LOG:Robocopy-{}", Utc::now().date_naive().format("%Y-%m-%d")))
        .arg(format!("/MT:{}", sysinfo::System::physical_core_count().unwrap_or(4)))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    // Create a Tokio stream for stdout
    let stdout = process.stdout.take().expect("Failed to get stdout");
    // Create a Tokio stream for stderr
    let stderr = process.stderr.take().expect("Failed to get stderr");

    let mut stdout_reader = BufReader::new(stdout).lines();
    let mut stderr_reader = BufReader::new(stderr).lines();
    
    let tx_clone = tx.clone();
    tokio::spawn(async move {
        while let Some(line) = stderr_reader.next_line().await? {
            tx_clone.try_send(line.into_bytes()).ok();
        }
        Ok::<(), anyhow::Error>(())
    });

    let tx_clone = tx.clone();
    tokio::spawn(async move {
        while let Some(line) = stdout_reader.next_line().await? {
            tx_clone.try_send(line.into_bytes()).ok();
        }
        Ok::<(), anyhow::Error>(())
    });

    let output = process.wait_with_output().await?;
    info!("robocopy -> output: {:?}", output);

    let tx_clone = tx.clone();
    if !output.status.success() {
        info!("robocopy -> output status not successful");
        tx_clone.try_send(output.stderr).ok();
    }

    Ok(())
}
