use std::collections::HashSet;
use std::ffi::OsStr;
use std::io::{self, Write};
use std::path::PathBuf;
use std::path::Path;
use std::fs;
use futures_util::future::{join_all, BoxFuture};
use walkdir::WalkDir;
use tokio::task;
use egui::Context;
use tokio::sync::mpsc::{self, UnboundedSender, UnboundedReceiver};

#[derive(Clone, Debug)]
pub enum TreeNode {
    File(String),
    Directory(String, Vec<TreeNode>),
    UnexpandedDirectory(String, PathBuf), // New variant
}

// Define a command that can be sent through the channel
pub enum Command {
    ExpandDirectory(PathBuf),
    // More commands can be added here as needed
}


pub struct FileBrowser {
    pub current_path: PathBuf,
    pub read_dirs_only: bool,
    pub expanded_dirs: HashSet<String>,
    pub tree: Option<TreeNode>,
    pub needs_refresh: bool,
    pub directories: Vec<PathBuf>,
    sender: UnboundedSender<PathBuf>,
    receiver: UnboundedReceiver<PathBuf>,
}

impl FileBrowser {
    pub fn new(command_tx: Sender<Command>, command_rx: Receiver<Command>) -> FileBrowser {
        let (sender, receiver) = mpsc::unbounded_channel();
        FileBrowser {
            current_path: PathBuf::from("."),
            read_dirs_only: false,
            expanded_dirs: HashSet::new(),
            tree: None,
            needs_refresh: true,
            directories: Vec::new(),
            command_tx: command_tx.clone(),
        }
    }

    pub async fn run(&mut self) -> Result<(), Box<dyn std::error::Error + Send>> {
        if self.needs_refresh {
            let current_path_clone = self.current_path.clone();
            self.tree = Some(self.build_tree(current_path_clone).await.await?);
            self.needs_refresh = false;
        }
        Ok(())
    }
    
    

    pub fn clone_for_thread(&self) -> FileBrowser {
        FileBrowser {
            current_path: self.current_path.clone(),
            read_dirs_only: self.read_dirs_only,
            expanded_dirs: self.expanded_dirs.clone(),
            tree: self.tree.clone(),
            needs_refresh: self.needs_refresh,
            directories: self.directories.clone()
        }
    }
    
    
    pub async fn build_tree(&self, path: PathBuf) -> BoxFuture<'static, Result<TreeNode, Box<dyn std::error::Error + Send>>> {
        let self_clone = self.clone_for_thread();
        Box::pin(async move {
            let mut children = Vec::new();
            let mut read_dir = tokio::fs::read_dir(path.clone()).await.map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;

            let mut dir_entries = Vec::new();
            while let Some(res) = read_dir.next_entry().await.map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>).ok().flatten() {
                dir_entries.push(res);
            }
            
    
            let mut child_futures: Vec<BoxFuture<'static, Result<TreeNode, Box<dyn std::error::Error + Send>>>> = Vec::new();



            for entry in dir_entries {
                let path = entry.path();
                if path.file_name().is_some() {
                    if path.is_dir() {
                        let name = path.file_name().unwrap().to_string_lossy().to_string();
                        children.push(TreeNode::UnexpandedDirectory(name, path)); // Add an UnexpandedDirectory node
                    } else {
                        children.push(TreeNode::File(path.file_name().unwrap().to_string_lossy().to_string()));
                    }
                }
            }
            
            
    
            let child_results = join_all(child_futures).await;
            for child in child_results {
                children.push(child?);
            }
            //let file_name = path.file_name();
            let file_stem = path.file_stem();
            let name = match file_stem {
                Some(stem) => stem.to_string_lossy().to_string(),
                None => {
                    if path == Path::new(".") {
                        ".".to_string() // for current directory
                    } else if path == Path::new("..") {
                        "..".to_string() // for parent directory
                    } else if path.has_root() {
                        "/".to_string() // for root directory
                    } else {
                        return Err(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "The path does not have a file name")) as Box<dyn std::error::Error + Send>);
                    }
                },
            };
            
            
            
            
            

            Ok(TreeNode::Directory(name, children))
        })
    }
    
    
    pub fn change_directory(&mut self, directory: &str) {
        self.current_path = PathBuf::from(directory);
        self.needs_refresh = true;
    }

    pub async fn expand_directory(&self, path: PathBuf) -> Result<TreeNode, Box<dyn std::error::Error + Send>> {
        self.build_tree(path).await.await
    }

    pub fn copy(&self, src: &Path, dst: &Path) -> std::io::Result<()> {
        if src.is_dir() {
            for entry in WalkDir::new(src) {
                let entry = entry?;
                let path = entry.path();
                let relative_path = path.strip_prefix(src).unwrap();
    
                let dst_path = dst.join(relative_path);
    
                if path.is_dir() {
                    fs::create_dir_all(dst_path)?;
                } else {
                    fs::copy(path, dst_path)?;
                }
            }
        } else {
            if let Some(parent) = dst.parent() {
                if !parent.exists() {
                    fs::create_dir_all(parent)?;
                }
            }
            fs::copy(src, dst)?;
        }
    
        Ok(())
    }
    
    pub fn move_or_rename_file(&self) {
        println!("Enter old file path:");
        let mut old_path = String::new();
        io::stdin().read_line(&mut old_path).unwrap();
        println!("Enter new file path:");
        let mut new_path = String::new();
        io::stdin().read_line(&mut new_path).unwrap();
        fs::rename(old_path.trim(), new_path.trim()).unwrap();
    }
    
    pub fn create_file(&self) {
        println!("Enter file path:");
        let mut file_path = String::new();
        io::stdin().read_line(&mut file_path).unwrap();
        let mut file = fs::File::create(file_path.trim()).unwrap();
        file.write_all(b"").unwrap();
    }

    pub fn send_command(&self, command: Command) {
        self.command_tx.send(command).unwrap();
    }
}
    /*
    pub async fn run(&mut self) {
        loop {
            let absolute_path = fs::canonicalize(&self.current_path).unwrap();
            let display_path = absolute_path.to_string_lossy().to_string();
            let display_path = display_path.trim_start_matches("\\\\?\\");
            // println!("Current Directory: {:?}", display_path);

            // Build the file system tree and store it in self.tree
            self.tree = Some(self.build_tree(&self.current_path).unwrap());
            //let tree = task::block_in_place(|| self.build_tree(&self.current_path).unwrap());

            if let Some(tree) = &self.tree {
                self.print_tree(tree, 0);  // print the tree for debugging
            }
            
            let contents = task::block_in_place(|| self.list_directory_contents());

            println!("contents: {:?}", contents);
            //update_gui(contents);  // update GUI with new directory contents
            // pass context here and repaint // or will i need to since this is all constantly being ran by the gui..
            
            let mut option = String::new();
            task::block_in_place(|| io::stdin().read_line(&mut option)).unwrap();
            let option = option.trim();

            match option {
                "copy" => task::block_in_place(|| self.copy_file()),
                "move" | "rename" => task::block_in_place(|| self.move_or_rename_file()),
                "create" => task::block_in_place(|| self.create_file()),
                _ => {
                    self.current_path = PathBuf::from(option);
                }
            };
            // Yield control back to the caller
            tokio::task::yield_now().await;
        }
    }
     */