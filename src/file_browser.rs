use std::ffi::OsStr;
use std::io::{self, Write};
use std::path::PathBuf;
use std::fs;
use walkdir::WalkDir;
use tokio::task;
use std::path::Path;
use egui::Context;

enum TreeNode {
    File(String),
    Directory(String, Vec<TreeNode>),
}

pub struct FileBrowser {
    current_path: PathBuf,
    ctx: Context,
    pub read_dirs_only: bool,
}

impl FileBrowser {
    pub fn new(ctx: Context) -> FileBrowser {
        FileBrowser {
            current_path: PathBuf::from("."),
            ctx,
            read_dirs_only: false,
        }
    }

    pub async fn run(&mut self) {
        loop {
            let absolute_path = fs::canonicalize(&self.current_path).unwrap();
            let display_path = absolute_path.to_string_lossy().to_string();
            let display_path = display_path.trim_start_matches("\\\\?\\");
            // println!("Current Directory: {:?}", display_path);

            let tree = task::block_in_place(|| self.build_tree(&self.current_path).unwrap());
            self.print_tree(&tree, 0);  // print the tree for debugging

            let contents = task::block_in_place(|| self.list_directory_contents());

            println!("contents: {:?}", contents);
            //update_gui(contents);  // update GUI with new directory contents
            // pass context here and repaint 

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
        }
    }
    
    fn update_egui_ctx(&mut self, ctx: Context) {
        self.ctx = ctx;
    }

    fn print_tree(&self, node: &TreeNode, indent: usize) {
        let indentation = " ".repeat(indent * 2);
        match node {
            TreeNode::File(name) => println!("{}File: {}", indentation, name),
            TreeNode::Directory(name, children) => {
                println!("{}Directory: {}", indentation, name);
                for child in children {
                    self.print_tree(child, indent + 1);
                }
            }
        }
    }
    
    fn list_directory_contents(&self) -> Vec<String> {
        let mut contents = Vec::new();
        if self.read_dirs_only {
            // Show directories only
            for entry in WalkDir::new(&self.current_path) {
                let entry = entry.unwrap();
                if entry.file_type().is_dir() {
                    let name = format!("{}", entry.file_name().to_string_lossy());
                    contents.push(name);
                }
            }
        } else {
            // Show all files and directories
            let entries = fs::read_dir(&self.current_path).unwrap();
            for entry in entries {
                let entry = entry.unwrap();
                let metadata = entry.metadata().unwrap();
                let name = if metadata.is_file() {
                    format!("{}", entry.file_name().to_string_lossy())
                } else if metadata.is_dir() {
                    format!("DIR: {}", entry.file_name().to_string_lossy())
                } else {
                    continue;
                };
                contents.push(name);
            }
        }
        contents
    }
    
    
    fn build_tree(&self, path: &Path) -> io::Result<TreeNode> {
        if path.is_file() {
            return Ok(TreeNode::File(path.file_name().unwrap_or(OsStr::new("unknown")).to_string_lossy().to_string()));
        }
    
        let mut children = Vec::new();
        for entry_result in fs::read_dir(path)? {
            let entry = entry_result?;
            children.push(self.build_tree(&entry.path())?);
        }
    
        Ok(TreeNode::Directory(path.file_name().unwrap_or(OsStr::new("unknown")).to_string_lossy().to_string(), children))
    }
    
    
    fn copy_file(&self) {
        println!("Enter source file:");
        let mut source = String::new();
        io::stdin().read_line(&mut source).unwrap();
        println!("Enter destination file:");
        let mut destination = String::new();
        io::stdin().read_line(&mut destination).unwrap();
        fs::copy(source.trim(), destination.trim()).unwrap();
    }
    
    fn move_or_rename_file(&self) {
        println!("Enter old file path:");
        let mut old_path = String::new();
        io::stdin().read_line(&mut old_path).unwrap();
        println!("Enter new file path:");
        let mut new_path = String::new();
        io::stdin().read_line(&mut new_path).unwrap();
        fs::rename(old_path.trim(), new_path.trim()).unwrap();
    }
    
    fn create_file(&self) {
        println!("Enter file path:");
        let mut file_path = String::new();
        io::stdin().read_line(&mut file_path).unwrap();
        let mut file = fs::File::create(file_path.trim()).unwrap();
        file.write_all(b"").unwrap();
    }
    
}
