// TODO: Catch errors when scanning and display them to the user, then continue
// TODO: Display scanning animation when refreshing too
// TODO: Allow specifying a command to print the size of a file instead of using disk usage
// TODO: Add an argument parser to handle invalid input better
use std::{collections::BTreeMap,ffi::OsString,path::PathBuf,sync::{Arc, Mutex},thread};
use path_info::{get_starting_dir, get_wrapped_contents, PathInfo};
use crate::terminal_mode::context::TerminalContext;
use ratatui::widgets::ListState;

pub mod render;
pub mod path_info;

pub struct NcduTab {
    ctx: Arc<Mutex<TerminalContext>>,
    starting_dir: Arc<Mutex<PathBuf>>,
    state: Arc<Mutex<ListState>>,
    contents: Arc<Mutex<PathInfo>>,
    current_dir: Arc<Mutex<Vec<OsString>>>,
}

impl NcduTab {
    pub fn new(ctx: Arc<Mutex<TerminalContext>>) -> Self {
        let starting_dir: Arc<Mutex<PathBuf>> = match get_starting_dir() {
            Ok(dir) => Arc::new(Mutex::new(dir)),
            Err(e) => panic!("{}", e),
        };
    
        let state: Arc<Mutex<ListState>> = Arc::new(Mutex::new(ListState::default()));
        state.lock().unwrap().select(Some(0));
    
        // let (tx, rx) = std::sync::mpsc::channel();
    
        let contents: Arc<Mutex<PathInfo>> = Arc::new(Mutex::new(PathInfo::Folder(0, BTreeMap::new(), 0)));
        let contents_clone: Arc<Mutex<PathInfo>> = Arc::clone(&contents);
        let dir: Vec<OsString> = vec![];
        let current_dir: Arc<Mutex<Vec<OsString>>> = Arc::new(Mutex::new(dir));
        let starting_dir_clone: Arc<Mutex<PathBuf>> = Arc::clone(&starting_dir);
        // thread::spawn(move || {
        //     *contents_clone.lock().unwrap() = get_wrapped_contents(&starting_dir_clone.lock().unwrap());
        //     tx.send(0).unwrap();
        // });
    
        // let mut dot_pos = 0;
        // let mut dot_fwd = true;


        Self { 
            ctx,
            state,
            starting_dir,
            contents,
            current_dir,
        }
    }

    pub fn receive(&mut self) {
        // match self.rx.try_recv() {
        //     Ok(_) => {},
        //     Err(_) => {}
        // }
    }
}