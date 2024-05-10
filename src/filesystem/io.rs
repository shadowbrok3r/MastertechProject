use fs_extra::dir::get_size;
use futures_util::future::join_all;
use tokio::{fs, sync::mpsc::{error::SendError, UnboundedSender, self}, task::spawn_blocking};
use num_format::{Locale, ToFormattedString};
use eframe::egui::widgets::text_edit::*;
use std::{io::Error, sync::{Arc, Mutex}, path::PathBuf};
use rayon::prelude::*;

const KB_FROM_BYTES: u64 = 1024;
const MB_FROM_BYTES: u64 = 1024 * 1024;
const GB_FROM_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Debug)]
pub struct MetaData {
    pub path_size: u64,
}
#[derive(Debug)]
pub enum CopyError {
    IoError(Error),
    SendError(SendError<f64>),
    // Add other types of errors if needed
}

impl From<Error> for CopyError {
    fn from(err: Error) -> CopyError {
        CopyError::IoError(err)
    }
}

impl From<SendError<f64>> for CopyError {
    fn from(err: SendError<f64>) -> CopyError {
        CopyError::SendError(err)
    }
}

pub async fn copy_selected_items(
    selected_files: Vec<PathBuf>, 
    destination_dir: PathBuf, 
    progress_tx: UnboundedSender<f64>,
) -> Result<(), CopyError>{
    
    let total_size_futures = 
        selected_files
        .iter()
        .map(|path| 
            async move { 
                fs::metadata(path)
                .await
                .unwrap()
                .len() 
            }
        );
    let total_sizes: Vec<u64> = join_all(total_size_futures).await;
    let total_size: u64 = total_sizes.iter().sum();
    
    let copied_size = Arc::new(Mutex::new(0u64));

    // Prepare the source-destination pairs using Rayon
    let prepared_files: Vec<_> = selected_files.par_iter().map(|src_path| {
        let dest_path = destination_dir.join(src_path.file_name().unwrap());
        (src_path.clone(), dest_path)
    }).collect();
    
    let mut task_handles = Vec::new();  // To keep track of spawned tasks
    
    // Generate the async copy tasks and collect them into copy_futures
    for (src_path, dest_path) in prepared_files.iter() {
        let copied_size = copied_size.clone();
        let progress_tx = progress_tx.clone();
        let src_path = src_path.clone();
        let dest_path = dest_path.clone();
        
        let start_time = std::time::Instant::now();  // Record start time
        
        let task_handle = tokio::spawn(async move {
            let _ = match fs::copy(&src_path, &dest_path).await{
                Ok(size_copied) => {
                    let elapsed_time = start_time.elapsed().as_secs_f64();  // Time elapsed in seconds
                    // Update the shared counter
                    let mut copied = copied_size.lock().unwrap();
                    *copied += size_copied;

                        // Calculate the speed in MB/s
                    let speed_mbps = (*copied as f64 / 1_000_000.0) / elapsed_time;
                    println!("Speed: {:.2} MB/s", speed_mbps);
                    // Send the progress
                    let progress = (*copied as f64 / total_size as f64) * 100.0;
                    match progress_tx.send(progress){
                        Ok(_) => println!("sent ok"),
                        Err(e) => println!("progress_tx send error: {e}"),
                    }
                    Ok(())
                },
                Err(e) => Err(CopyError::IoError(e))
            };

        });
        
        task_handles.push(task_handle);
    }

    // Await all the spawned tasks to complete
    for handle in task_handles {
        handle.await.unwrap();
    }
    
    Ok(())
}


pub fn format_path_metadata(mut path_size: u64) -> String{
    let mut formatted_size = "".to_string();
    if path_size > 0
    {
        if path_size > GB_FROM_BYTES
        {
            let mut x = path_size as f32;
            x  = &x / GB_FROM_BYTES as f32;
            let two_decimal_places = (x*100.0).round() / 100.0;
            let x_as_string = two_decimal_places.to_string();
            let y: Vec<&str> = x_as_string.split(".").collect();
            let decimal = y[1].as_str();
            let new_path_size = x.clone() as u64;
            formatted_size = format!("{}.{decimal} Gb", new_path_size.to_formatted_string(&Locale::en));
        }
        else if path_size > MB_FROM_BYTES
        {
            path_size = path_size / MB_FROM_BYTES;
            formatted_size = format!("{} Mb", path_size.to_formatted_string(&Locale::en));
        } 
        else if path_size > KB_FROM_BYTES
        {
            path_size = path_size / KB_FROM_BYTES;
            formatted_size = format!("{} Kb", path_size.to_formatted_string(&Locale::en));
        }
        else{
            formatted_size = format!("{} bytes", path_size.to_formatted_string(&Locale::en));
        }
        

        
        formatted_size
    }
    else {
        format!("0b")
    }

}

// https://github.com/acidnik/ppcp/blob/master/src/copy.rs#L48

// pub async fn retrieve_metadata(path: PathBuf){
//     let sender = self.metadata_tx.clone();
//     let cloned_path = path.clone();
//     let clone_path1 = path.clone();
//     // Spawn the appropriate async task depending on whether the path is a directory or a file.
//     let read_metadata_task = if path.is_dir() {
//         tokio::spawn(async move {
//             get_size(cloned_path).unwrap_or(0)
//         })
//     } else if path.is_file() {
//         tokio::spawn(async move {
//             tokio::fs::metadata(&cloned_path).await.unwrap().len()
//         })
//     } else {
//         // Handle the case where the path is neither a directory nor a file.
//         return;
//     };
//     // Use tokio::select! to wait for the metadata task to complete.
//     tokio::select! {
//         result = read_metadata_task => {
//             match result {
//                 Ok(path_size) => {
//                     // Send the result through the channel.
//                     if sender.try_send(path_size).is_err() {
//                         println!("Error sending metadata");
//                     }   
//                     // Insert the metadata into the appropriate HashMap.
//                     if path.is_dir() {
//                         self.folder_metadata.borrow_mut().insert(clone_path1.clone(), MetaData { path_size });
//                     } else {
//                         self.file_metadata.borrow_mut().insert(clone_path1.clone(), MetaData { path_size });
//                     }
//                 },
//                 Err(e) => println!("Error reading metadata: {:?}", e),
//             }
//         }
//     }
// }

/*
pub struct TransferOptions{
    /// Sets the option true for overwrite existing files.
    pub overwrite: bool,
    /// Sets the option true for skip existing files.
    pub skip_exist: bool,
    /// Sets buffer size for copy/move work only with receipt information about process work.
    pub buffer_size: usize,
}

impl TransferOptions{
    /// Initialize struct CopyOptions with default value.
    ///
    /// ```rust,ignore
    ///
    /// overwrite: false
    ///
    /// skip_exist: false
    ///
    /// buffer_size: 64000 //64kb
    /// ```
    pub fn new() -> TransferOptions {
        TransferOptions {
            overwrite: false,
            skip_exist: false,
            buffer_size: 64000, //64kb
        }
    }

    /// Sets the option true for overwrite existing files.
    pub fn overwrite(mut self, overwrite: bool) -> Self {
        self.overwrite = overwrite;
        self
    }

    /// Sets the option true for skip existing files.
    pub fn skip_exist(mut self, skip_exist: bool) -> Self {
        self.skip_exist = skip_exist;
        self
    }

    /// Sets buffer size for copy/move work only with receipt information about process work.
    pub fn buffer_size(mut self, buffer_size: usize) -> Self {
        self.buffer_size = buffer_size;
        self
    }
}

impl Default for TransferOptions {
    fn default() -> Self {
        TransferOptions::new()
    }
}


/// A structure which stores information about the current status of a file that's copied or moved.
pub struct Progress {
    /// Copied bytes on this time.
    pub copied_bytes: u64,
    /// All the bytes which should to copy or move.
    pub total_bytes: u64,
}
*/