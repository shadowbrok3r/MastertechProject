use std::{env, fs::create_dir, path::PathBuf, process::Stdio, time::Duration};
use chrono::Utc;
use sysinfo::{Pid, ProcessRefreshKind, RefreshKind};
use tokio::fs; //, io::{AsyncBufReadExt, BufReader}};
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

/// Progress update for a single robocopy process
#[derive(Debug, Clone)]
pub struct RobocopyProgress {
    pub pid: u32,
    pub source: String,
    pub destination: String,
    pub bytes_read: f64,
    pub bytes_written: f64,
    pub is_complete: bool,
}

/// Message type for robocopy progress channel
#[derive(Debug, Clone)]
pub enum RobocopyMessage {
    /// Progress update with (pid, bytes_read_mb, bytes_written_mb)
    Progress(RobocopyProgress),
    /// Process completed
    Complete(u32),
}

/// Directories to exclude from robocopy transfers (junction points, system folders, etc.)
const ROBOCOPY_EXCLUDED_DIRS: &[&str] = &[
    "Default",
    "Default User", 
    "All Users",
    "Application Data",
    "Local Settings",
    "NetHood",
    "PrintHood",
    "Recent",
    "SendTo",
    "Start Menu",
    "Templates",
    "Cookies",
    "My Documents",  // Junction point
    "My Music",      // Junction point
    "My Pictures",   // Junction point
    "My Videos",     // Junction point
];

pub async fn run_robocopy(
    source: &PathBuf, 
    destination: &PathBuf,
    tx: Sender<RobocopyMessage>
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
    let source_folder_name = source_user_name.to_string_lossy();
    let backup_folder = destination.join("Desktop").join("UsersBackup").join(&*source_folder_name);
    
    // Create unique log file name with source folder and timestamp
    let timestamp = Utc::now().format("%Y-%m-%d_%H-%M-%S");
    let log_filename = format!("Robocopy-{}-{}.txt", source_folder_name, timestamp);
    let log_location = destination.join("Desktop").join(log_filename);
    let log_arg = format!("/LOG:{}", log_location.display());
    
    std::fs::create_dir_all(&backup_folder)?;

    let source_display = source.display().to_string();
    let dest_display = backup_folder.display().to_string();

    log::info!(
        "Source: {:?} Dest: {:?} source_user_name: {:?}\nbackup_folder: {:?}\nlog_location: {:?}",
        source, 
        destination,
        source_user_name,
        backup_folder,
        log_location,
    );

    // Build the robocopy command with excluded directories
    let mut cmd = tokio::process::Command::new("robocopy");
    cmd.arg(source)
        .arg(&backup_folder)
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
        .arg(&log_arg)
        .arg(format!("/MT:{}", sysinfo::System::physical_core_count().unwrap_or(4)))
        // Exclude junction points and reparse points to avoid infinite loops
        .arg("/XJ")
        // Exclude system/hidden directories that cause issues
        .arg("/XD");
    
    // Add all excluded directories
    for excluded_dir in ROBOCOPY_EXCLUDED_DIRS {
        cmd.arg(excluded_dir);
    }
    
    let mut process = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let pid = process.id().unwrap_or(0);
    
    // Spawn a task to monitor the process and send progress updates
    tokio::spawn(async move {
        let mut sys = sysinfo::System::new_with_specifics(
            RefreshKind::default().with_processes(
                ProcessRefreshKind::default().with_disk_usage()
            )
        );

        // Continuously monitor while process is running
        loop {
            // Check if process is still running
            match process.try_wait() {
                Ok(Some(_status)) => {
                    // Process has completed
                    let _ = tx.try_send(RobocopyMessage::Complete(pid));
                    break;
                }
                Ok(None) => {
                    // Process is still running, send progress update
                    sys.refresh_processes(
                        sysinfo::ProcessesToUpdate::Some(&[Pid::from_u32(pid)]),
                        false
                    );
                    
                    if let Some(process_info) = sys.process(Pid::from_u32(pid)) {
                        let disk_usage = process_info.disk_usage();
                        let bytes_read = disk_usage.read_bytes as f64 / 1_048_576.0; // MB/s (since last refresh)
                        let bytes_written = disk_usage.written_bytes as f64 / 1_048_576.0;
                        
                        let progress = RobocopyProgress {
                            pid,
                            source: source_display.clone(),
                            destination: dest_display.clone(),
                            bytes_read,
                            bytes_written,
                            is_complete: false,
                        };
                        
                        let _ = tx.try_send(RobocopyMessage::Progress(progress));
                    }
                    
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
                Err(e) => {
                    log::error!("Error checking robocopy process status: {:?}", e);
                    let _ = tx.try_send(RobocopyMessage::Complete(pid));
                    break;
                }
            }
        }
        
        Ok::<(), anyhow::Error>(())
    });

    Ok(())
}
