use std::error::Error;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::sync::mpsc::UnboundedReceiver;
use async_recursion::async_recursion; 

pub enum Command {
    Refresh,
    Copy(PathBuf, PathBuf),
    Move(PathBuf, PathBuf),
    CreateFile(PathBuf),
    CreateDirectory(PathBuf),
    Rename(PathBuf, PathBuf),
    ExpandDirectory(PathBuf),
}

#[derive(Debug)]
pub enum TreeNode {
    File(String),
    Directory(String, Vec<TreeNode>),
    UnexpandedDirectory(String, PathBuf),
}

pub struct FileBrowser {
    pub tree: Option<TreeNode>,
    pub current_path: PathBuf,
    pub read_dirs_only: bool,
    pub read_hidden_files: bool,
    pub expanded_dirs: std::collections::HashSet<String>,
    command_rx: UnboundedReceiver<Command>,
}

impl FileBrowser {
    pub fn new(current_path: PathBuf,command_rx: UnboundedReceiver<Command>) -> Self {
        Self {
            tree: None,
            current_path,
            read_dirs_only: false,
            read_hidden_files: false,
            expanded_dirs: std::collections::HashSet::new(),
            command_rx,
        }
    }

    pub async fn run(&mut self) -> Result<(), Box<dyn Error>> {
        while let Some(command) = self.command_rx.recv().await {
            match command {
                Command::Refresh => {
                    self.tree = Some(self.expand_directory(&self.current_path).await.unwrap());
                }
                Command::Copy(src, dst) => {
                    if src.is_file() {
                        fs::copy(src, dst).await?;
                    } else {
                        todo!(); // Implement directory copying
                    }
                }
                Command::Move(src, dst) => {
                    fs::rename(src, dst).await?;
                }
                Command::CreateFile(path) => {
                    fs::File::create(path).await?;
                }
                Command::CreateDirectory(path) => {
                    fs::create_dir(path).await?;
                }
                Command::Rename(old_path, new_path) => {
                    fs::rename(old_path, new_path).await?;
                }
                Command::ExpandDirectory(path) => {
                    self.tree = Some(self.expand_directory(&path).await.unwrap());
                }
            }
        }

        Ok(())
    }

    #[async_recursion]
    async fn expand_directory(&self, path: &Path) -> Result<TreeNode, std::io::Error> {
        let dir_name = path
            .file_name()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid directory name"))?
            .to_string_lossy()
            .to_string();
    
        if self.expanded_dirs.contains(&dir_name) {
            let mut entries = fs::read_dir(path).await?;
            let mut children = Vec::new();
            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                if path.is_dir() {
                    children.push(self.expand_directory(&path).await?);
                } else {
                    let file_name = path
                        .file_name()
                        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid file name"))?
                        .to_string_lossy()
                        .to_string();
    
                    children.push(TreeNode::File(file_name));
                }
            }
            Ok(TreeNode::Directory(dir_name, children))
        } else {
            Ok(TreeNode::UnexpandedDirectory(dir_name, path.to_owned()))
        }
    }
}


    /* 
    
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