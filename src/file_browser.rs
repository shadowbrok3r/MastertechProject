use std::{error::Error, path::{Path, PathBuf}};
use tokio::fs;
use tokio::sync::mpsc::UnboundedReceiver;
use async_recursion::async_recursion; 

#[derive(Clone)]
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
    pub read_hidden_files: bool,
    pub expanded_dirs: std::collections::HashSet<String>,
    command_rx: UnboundedReceiver<Command>,
}

impl FileBrowser {
    pub fn new(current_path: PathBuf, command_rx: UnboundedReceiver<Command>) -> Self {
        Self {
            tree: None,
            current_path,
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

