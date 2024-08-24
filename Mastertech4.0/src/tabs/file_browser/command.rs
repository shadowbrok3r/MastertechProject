use std::{env, fs::create_dir, path::PathBuf};

use crossbeam::channel;
use fs_extra::dir::get_size;
use log::debug;
use tokio::fs;
use tracing::info;

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

            Command::Copy(source, mut destination, progress_tx) => {
                info!("Source: {source:?} // Destination: {destination:?}");
                if source.len() == 1 {
                    if let Some(path) = source[0].file_name(){
                        info!("Single path: {:?}", path);
                        info!("Making a directory here: {:?}", destination.join(path));
                        destination = destination.join(path);
                        if !destination.exists() {
                            info!("Creating destination directory");
                            let create_dest_dir = create_dir(destination.clone());
                            if let Err(e) = create_dest_dir {
                                info!("Error creating dir: {e:?}");
                            }
                        }
                    }
                }
                info!("Running copy operations, pasting here: {:?}", destination.clone());
                // spawn(async move {
                //     run_robocopy(source.get(0).unwrap(), &destination, log_output).await;
                // });
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

// async fn run_robocopy(source: &PathBuf, destination: &PathBuf, log_output: Arc<Mutex<String>>) {
//     let mut command = Command::new("robocopy")
//         .arg(source)
//         .arg(destination)
//         .arg("*.*")
//         .arg("/S")
//         .arg("/E")
//         .arg("/COPY:DAT")
//         .arg("/DCOPY:DAT")
//         .arg("/R:0")
//         .arg("/W:30")
//         .arg("/ZB")
//         .arg(format!("/MT:{}", System::new().physical_core_count().unwrap()))
//         .stdout(Stdio::piped())
//         .stderr(Stdio::piped())
//         .spawn()
//         .expect("Failed to execute robocopy command");

//     let stdout = command.stdout.take().expect("Failed to open stdout");
//     let stderr = command.stderr.take().expect("Failed to open stderr");

//     let log_output_clone = Arc::clone(&log_output);
//     let stdout_task = tokio::spawn(async move {
//         let mut reader = BufReader::new(stdout).lines();
//         while let Some(line) = reader.next_line().await.unwrap_or(None) {
//             let mut log_output = log_output_clone.lock().await;
//             log_output.push_str(&line);
//             log_output.push('\n');
//         }
//     });

//     let log_output_clone = Arc::clone(&log_output);
//     let stderr_task = tokio::spawn(async move {
//         let mut reader = BufReader::new(stderr).lines();
//         while let Some(line) = reader.next_line().await.unwrap_or(None) {
//             let mut log_output = log_output_clone.lock().await;
//             log_output.push_str(&line);
//             log_output.push('\n');
//         }
//     });

//     stdout_task.await.unwrap();
//     stderr_task.await.unwrap();
// }