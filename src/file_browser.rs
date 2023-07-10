use tokio::fs::*;
use tokio::io::*;
use std::path::Path;
use jwalk::*;
use egui_file::FileDialog;
use std::path::PathBuf;
use std::env;

// Error messages
const TEMPDIR_ERR_MSG: &str = "Can't write to temporary directory";
const ENV_READ_ERR_MSG: &str = "Can't access current directory";
const ENV_WRITE_ERR_MSG: &str = "Can't change environment";

pub struct FileBrowser{
    pub opened_file: Option<PathBuf>,
    pub open_file_dialog: Option<FileDialog>,
}

impl FileBrowser{
    pub async fn file_browsing_test(ctx: &egui::Context) -> tokio::io::Result<()>{
        let pwd = env::current_dir().map_err(|_| ENV_READ_ERR_MSG);

        let temp_dir = env::var("temp").unwrap_or_else(|_| env::current_dir()
        .unwrap()
        .into_os_string()
        .into_string()
        .unwrap());

        let target_dir = &get_target_dir();

        if Path::new(target_dir).exists() {
            tokio::fs::remove_dir_all(target_dir).await.map_err(|_| TEMPDIR_ERR_MSG);
        }
        tokio::fs::create_dir_all(target_dir).await.map_err(|_| TEMPDIR_ERR_MSG);

        env::set_current_dir(target_dir).map_err(|_| ENV_WRITE_ERR_MSG);

        // restore to the actual current directory
        env::set_current_dir(pwd.unwrap()).map_err(|_| ENV_WRITE_ERR_MSG);

        // remove temporary files
        // fs::remove_dir_all(target_dir).map_err(|_| "Can't write to temporary directory")?;

        let path = Path::new("/home/shadowbroker/Desktop");
        let mut read_dir = tokio::fs::read_dir(path).await?;

        while let Some(entry) = read_dir.next_entry().await? {
            let path = entry.path();
            if path.is_dir() {
                println!("{}", path.display());
            }
        }
        Ok(())




        // DirBuilder::new()
        // .recursive(true)
        // .create("/tmp/foo/bar/baz")
        // .await?;


        

        // let mut dialog = FileDialog::open_file(self.opened_file.clone());
        // dialog.open();
        // self.open_file_dialog = Some(dialog);

        // if let Some(dialog) = &mut self.open_file_dialog {
        //     if dialog.show(ctx).selected() {
        //         if let Some(file) = dialog.path() {
        //             self.opened_file = Some(file);
        //         }
        //     }
        // }
    }

}

pub fn get_target_dir() -> String {
    #[cfg(unix)]
    return "/tmp/Mastertech".to_string();
    #[cfg(windows)]
    return format!(
        "{p}/Mastertech",
        p = env::var("temp").unwrap_or_else(|_| env::current_dir()
            .unwrap()
            .into_os_string()
            .into_string()
            .unwrap())
    );
    #[cfg(not(any(unix, windows)))]
    return format!(
        "{p}/.Mastertech",
        p = env::current_dir()
            .unwrap()
            .into_os_string()
            .into_string()
            .unwrap()
    );
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
