use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use jwalk::*;
use egui_file::FileDialog;
use std::path::PathBuf;

pub struct FileBrowser{
    pub opened_file: Option<PathBuf>,
    pub open_file_dialog: Option<FileDialog>,
}

impl FileBrowser{
    pub fn file_dialog(&mut self, ctx: &egui::Context){
        let mut dialog = FileDialog::open_file(self.opened_file.clone());
        dialog.open();
        self.open_file_dialog = Some(dialog);

        if let Some(dialog) = &mut self.open_file_dialog {
            if dialog.show(ctx).selected() {
                if let Some(file) = dialog.path() {
                    self.opened_file = Some(file);
                }
            }
        }
    }

}

pub fn file_browser(){

    let mut total: u64 = 0;


    let path = "/home/shadowbroker/";

    for dir_entry_result in WalkDirGeneric::<((), Option<u64>)>::new(&path)
    .skip_hidden(false)
    .parallelism(Parallelism::RayonNewPool(4))
    .process_read_dir(|_, _, _, dir_entry_results| {
        dir_entry_results.iter_mut().for_each(|dir_entry_result| {
            if let Ok(dir_entry) = dir_entry_result {
                if !dir_entry.file_type.is_dir() {
                    dir_entry.client_state =
                        Some(dir_entry.metadata().map(|m| m.len()).unwrap_or_default());
                }
            }
        })
    })
    {
        match dir_entry_result {
            Ok(dir_entry) => {
                if let Some(len) = &dir_entry.client_state {
                    eprintln!("counting {:?}", dir_entry.path());
                    total += len;
                }
            }
            Err(error) => {
                println!("Read dir_entry error: {}", error);
            }
        }
    }

    println!("path: {} total bytes: {}", path, total);
}

// pub async fn copy_file(src_path: &str, dst_path: &str) -> Result<T, Box<dyn std::error::Error>> {
//     // Open source file.
//     let mut src_file = File::open(src_path).await?;
//     let mut dst_file = File::create(dst_path).await?;
    
//     // Create buffer to read into.
//     let mut buffer = Vec::new();

//     // Read file to string.
//     src_file.read_to_end(&mut buffer).await?;
    
//     // Write to destination file
//     dst_file.write_all(&buffer).await?;

//     Ok(())
// }

// Spawning the copy task
//let copy_task = tokio::spawn(copy_file("src.txt", "dst.txt"));
