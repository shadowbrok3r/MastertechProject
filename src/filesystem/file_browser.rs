use anyhow::Context;
use futures::Future;
use log::{debug, info};
use sysinfo::Disks;
use tokio::{
    fs, io::{self, AsyncBufRead, BufReader, BufWriter}, sync::mpsc::{
        unbounded_channel, UnboundedReceiver, UnboundedSender
    }
};
use eframe::egui::{*, collapsing_header::CollapsingState, text::LayoutJob};
use std::{cell::RefCell, collections::{HashMap, HashSet}, path::{Path, PathBuf}, pin::Pin};
use num_format::{Locale, ToFormattedString};
use walkdir::WalkDir;
use std::env;
use pollster::block_on;
use crossbeam::channel;

/**
 * TODO:
 * 1. need to make metadata update once we do a data copy or hit the refresh button
 * 2. Need to move commands away from using channels, just make more fn's that impl self
 * 3. when copying data, have it pull the metadata from the already existing 
 */
use fs_extra::dir::get_size;
//use cached::proc_macro::{io_cached, cached};
use crate::filesystem::io::{
    copy_selected_items, 
    format_path_metadata, 
    MetaData,
    //TransferOptions,
    //Progress
};

use super::file_copy::CopyBuilder;

const KB_FROM_BYTES: u64 = 1024;
const MB_FROM_BYTES: u64 = 1024*1024;
const GB_FROM_BYTES: u64 = 1024*1024*1024;

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

pub struct FileBrowser {
    /// Current opened path.
    path: PathBuf, 
    /// Editable field with path.
    path_edit: String, 
    /// Selected file path
    selected_item: Option<PathBuf>, 
    /// Editable field with filename.
    filename_edit: String, 
    /// Show directories only
    read_dirs_only: bool, 
    /// Show hidden files
    show_hidden: bool, 
    /// rename folder/file
    rename: bool, 
    /// Create new folder
    new_folder: bool, 
    /// HashSet of selected files (hold CTRL key to select multiple)
    selected_items: RefCell<HashSet<PathBuf>>, 
    /// HashMap of subcontents of a given dir
    dir_contents: RefCell<HashMap<PathBuf, Vec<PathBuf>>>, 
    /// How many subfolders to retrieve contents from
    depth: usize, 
    /// Update directory contents once displayed 
    first_refresh_contents: bool, 
    /// Metadata of each file
    file_metadata: RefCell<HashMap<PathBuf, MetaData>>, 
    /// MetaData of each folder
    folder_metadata: RefCell<HashMap<PathBuf, MetaData>>, 
    /// Send size of file in bytes
    metadata_tx: channel::Sender<u64>, 
    /// Send size of folder in bytes
    metadata_rx: channel::Receiver<u64>, 
    
    /// Progress percentage
    progress: f64, 
    /// Send progress 
    progress_tx: channel::Sender<u64>, 
    /// Retrieve progress 
    progress_rx: channel::Receiver<u64>, 
    /// Animate the progress bar
    animated_progress: bool, 
    /// When CTRL+C is hit, get the selected files to be copied
    copied_items_src: Vec<PathBuf>, 
    /// When CTRL+V is hit, paste files in the current 'path_edit' directory
    copied_items_dest: PathBuf, 

    drive_letters: Vec<String>,

    source_dir_size: u64
}

impl FileBrowser{ // sender: UnboundedSender<>
    pub fn new() -> Self{
        let mut path = env::current_dir().unwrap_or_default();
        let mut filename_edit = String::new();

        let path_edit = path.to_str().unwrap_or_default().to_string();
        println!("filebrowser::new() {}", &path_edit);
        if path.is_file() {
            filename_edit = get_file_name(&path).to_string();
            path.pop();
        }
        let (progress_tx, mut progress_rx) = channel::unbounded();
        let (metadata_tx, mut metadata_rx) = channel::unbounded();

        Self {
            path,
            path_edit,
            selected_item: None,
            selected_items: RefCell::new(HashSet::new()),
            dir_contents: RefCell::new(HashMap::new()),
            filename_edit,
            read_dirs_only: false,
            rename: true,
            new_folder: true,
            show_hidden: false,
            first_refresh_contents: true,
            depth: 1,
            file_metadata: RefCell::new(HashMap::new()),
            folder_metadata: RefCell::new(HashMap::new()),
            metadata_tx,
            metadata_rx,
            progress: 0.0,
            progress_tx,
            progress_rx,

            animated_progress: false,
            copied_items_src: Vec::new(),
            copied_items_dest: PathBuf::new(),
            drive_letters: Vec::new(),
            source_dir_size: 0
          }
    }
    
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

            Command::Copy(source, destination, progress_tx) => {
                // let dest_path = Path::new(&destination.to_str().unwrap());

                

                std::thread::spawn(move ||{
                    // Copy recursively, only including certain files:
                    for entry in source{
                        CopyBuilder::new(entry, destination.clone())
                            .overwrite_if_newer(true)
                            .overwrite_if_size_differs(true)
                            .with_exclude_filter(".sys")
                            .with_exclude_filter(".dat")
                            .run(progress_tx.clone())
                            .unwrap_or(());
                    }

                });
                    // copy_files(source, &destination, progress_tx).await.unwrap();
            },

            Command::Move(source, destination) => {
                println!("Command::Move");
                if let Err(err) = fs::rename(&source, &destination).await {
                    //let _ = response_sender.try_send(Response::Error(FileBrowserError::Io(err)));
                } else {
                    //let _ = response_sender.try_send(Response::Success(format!("Successfully moved from {:?} to {:?}", source, destination)));
                }
            
            },

            Command::Delete(path) => {
                println!("Command::Delete");
                if let Err(err) = fs::remove_dir_all(&path).await {
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
                puffin::profile_scope!("Command::ReadDirectory");
                let new_contents = read_folder(
                    &path,
                    self.depth,
                    self.show_hidden,
                );
                self.dir_contents.borrow_mut().insert(path, new_contents);
            }

            Command::ReadMetadata(path) => {
                puffin::profile_scope!("Command::ReadMetadata");
                let mut total_size: u64 = 0;
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
                } else {
                    // Handle the case where the path is neither a directory nor a file.
                    return;
                };
                // Use tokio::select! to wait for the metadata task to complete.
                tokio::select! {
                    result = read_metadata_task => {
                        match result {
                            Ok(path_size) => {
                                // Send the result through the channel.
                                if sender.try_send(path_size).is_err() {
                                    println!("Error sending metadata");
                                }
                                
                                // Insert the metadata into the appropriate HashMap.
                                if path.is_dir() {
                                    self.folder_metadata.borrow_mut().insert(clone_path1.clone(),
                                        MetaData { path_size });
                                    total_size += path_size;
                                } else {
                                    self.file_metadata.borrow_mut().insert(clone_path1.clone(),
                                        MetaData { path_size });
                                    total_size += path_size;
                                }
                                println!("Total size: {total_size}");
                                self.source_dir_size = total_size;
                            },
                            Err(e) => println!("Error reading metadata: {:?}", e),
                        }
                    }
                }
            },
            Command::GetDrives => self.get_drives()
        }
    }

    pub fn show(
        &mut self, 
        ui: &mut Ui,
        command_tx: channel::Sender<Option<Command>>,
        command_rx: channel::Receiver<Option<Command>> 
    ) {     
        let cut = &Event::Cut;
        let copy = &Event::Copy;
        let mut paste = false;
        let paste_event = &Event::Key { key: Key::V, physical_key: Some(Key::V), pressed: false, repeat: false, modifiers: Modifiers::COMMAND };

        let copy_shortcut =  ui.input_mut(|i| i.filtered_events(&EventFilter::default()).contains(copy));
        let cut_shortcut = ui.input_mut(|i| i.filtered_events(&EventFilter::default()).contains(cut));
        
        for mut event in ui.input_mut(|i| i.events.clone()){
            match event{
                Event::Paste(ref content) => {paste = true;},
                _ => {} // Handle other events normally
            }
        }
        
        if copy_shortcut { // && self.selected_items
            self.copied_items_src = self.selected_items.borrow_mut().drain().collect();
            println!("Copied Items: {:?}", self.copied_items_src);
            let command_tx = command_tx.clone();
            
            for path in &self.copied_items_src{
                match command_tx.clone().send(Some(Command::ReadMetadata(path.clone()))) {
                    Ok(_) => info!("Getting file size"),
                    Err(e) => println!("hovered sender error: {e:?}"),
                }
            }

        }
        
        if paste{
            self.animated_progress = true;
            if let Some(selected_path) = &self.selected_item{
                if selected_path.is_dir(){
                    self.copied_items_dest = PathBuf::from(selected_path);
                    match command_tx.send(Some(
                            Command::Copy(
                                self.copied_items_src.clone(), 
                                self.copied_items_dest.clone(), 
                                self.progress_tx.clone()
                            )
                        ))
                    {
                        Ok(_) => println!("Pasting contents"),
                        Err(e) => println!("{e}"),
                    }
                }else {
                    self.copied_items_dest = PathBuf::from(&self.path_edit);
                }
            }
            

            println!("Pasted {:?}\nin directory: {:?}", self.copied_items_src, self.copied_items_dest);

            ui.ctx().request_repaint();
        }


        while let Ok(progress) = self.progress_rx.try_recv() {
            // println!("progress_bar: {progress}");
            self.progress += progress as f64;
        }

        TopBottomPanel::top("file_browser_top").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add_enabled_ui(
                    self.path != env::current_dir().unwrap_or_default(),
                    |ui | {
                        let response = ui.button("🏠").on_hover_text("Home");
                        if response.clicked(){
                            match command_tx.send(Some(Command::Home)){
                                Ok(_) => println!("Home"),
                                Err(e) => println!("{e}"),
                            }
                        }
                    }
                );
                ui.add_enabled_ui(self.path.parent().is_some(), |ui| {
                    let response = ui.button("⬆").on_hover_text("Parent Folder"); //
                    if response.clicked() {
                        match command_tx.send(Some(Command::UpDirectory)){
                            Ok(_) => println!("UpDirectory"),
                            Err(e) => println!("{e}"),
                        }
                    }
                });

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                
                    let response = ui.button("⟲").on_hover_text("Refresh"); //
                    if response.clicked() {
                        match command_tx.send(Some(Command::Refresh)){
                            Ok(_) => println!("sent task successfully"),
                            Err(e) => println!("{e}")
                        }
                    }

                    ScrollArea::new([false, false])
                        .auto_shrink([false, false])
                        .show(ui, |ui| 
                    {
                        let response = ui.add_sized(
                            ui.available_size_before_wrap(),
                            TextEdit::singleline(&mut self.path_edit)
                                .id(Id::new("path_edit"))
                                .cursor_at_end(true),
                        ).on_hover_text(&self.path_edit);

                        if response.lost_focus() {
                            let path = PathBuf::from(&self.path_edit);
                            println!("Lost focus on self.path_edit");
                            match command_tx.send(Some(Command::OpenPath(path))){
                                Ok(_) => println!("sent task successfully"),
                                Err(e) => println!("{e}")
                            };

                        }

                    });
                    // if ui.rect_contains_pointer(top_bar){}
                });
            });
            
            ui.horizontal_top(|ui| {
                ui.checkbox(&mut self.read_dirs_only, "Show Directories ONLY");
                // ui.checkbox(&mut self.show_hidden, "Show Hidden");
                ui.with_layout(Layout::right_to_left(Align::TOP), |ui|{
                    ui.add_space(5.0);
                    self.drive_letters.sort_unstable_by(|b, a| a.partial_cmp(b).unwrap());
                    for drive in self.drive_letters.iter(){
                        let button = Button::new(RichText::new(format!("💾 {drive}")));
                        
                        if ui.add(
                            button
                        ).clicked(){
                            println!("Button clicked: {:?}", drive);
                            match command_tx.send(Some(Command::OpenPath(
                                    PathBuf::from(drive)
                                ))){
                                Ok(_) => println!("Opening drive path"),
                                Err(e) => println!("{e}"),
                            }
                        };
                    }
                    ui.label(RichText::new("Drive Letters -> ".to_string()));
                });

            });
            ui.add_space(ui.spacing().item_spacing.y);
        });

        TopBottomPanel::bottom("file_browser_bottom").show_inside(ui, |ui| {
            ui.add_space(ui.spacing().item_spacing.y * 2.0);
            
            ui.add
            ( // Update the progress bar
                ProgressBar::new(self.progress as f32 / self.source_dir_size as f32)
                    .show_percentage()
                    .fill(Color32::from_rgb(255, 77, 210))
                    .animate(self.animated_progress)
            );

            ui.add_space(ui.spacing().item_spacing.y * 2.0);

            ui.horizontal(|ui| {
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.label(RichText::new(format!("Try selecting some files to copy.. ")));
                    if copy_shortcut{
                        ui.colored_label(Color32::LIGHT_BLUE ,RichText::new(format!("Copied files to clipboard.")));
                    }else if paste{
                        ui.colored_label(Color32::RED , RichText::new(format!("File Copy In Progress")));
                    }
                    
                });

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if self.new_folder && ui.button("📁 New Folder").clicked() {
                        match command_tx.send(Some(Command::CreateDirectory)){
                            Ok(_) => println!("ok"),
                            Err(e) => print!("{e}")
                        }
                        
                    }

                    if self.rename {
                        ui.add_enabled_ui(self.can_rename(), |ui| {
                            if ui.button("Rename").clicked() {
                                if let Some(from) = self.selected_item.clone() {
                                    let to = from.with_file_name(&self.filename_edit);
                                    
                                    match command_tx.send(Some(Command::Rename(from, to))){
                                        Ok(_) => {
                                            println!("ok");
                                        },
                                        Err(e) => {
                                            print!("{e}");
                                        }
                                    }
                                    
                                }
                            }
                        });
                    }
                    
                    let result = ui.add(
                        TextEdit::singleline(&mut self.filename_edit).id(Id::new("file_name_edit")),
                    );

                    if result.lost_focus()
                    //&& result.ctx.input(|state| state.key_pressed(Key::Enter))
                    && !self.filename_edit.is_empty(){
                        let path = self.path.join(&self.filename_edit);

                    }
                });
            });
        });

        CentralPanel::default().show_inside(ui, |ui| 
        {
            ui.shrink_width_to_current();ui.shrink_height_to_current();
            ui.add_space(ui.spacing().item_spacing.y * 2.0);

            if self.first_refresh_contents{
                self.refresh_contents();
                self.get_drives();
                self.first_refresh_contents = false;
            }
            
            ScrollArea::new([true, true])
            .id_source("file_browser_scroll")
            .auto_shrink([false, false])
            .show_rows(ui,
            ui.text_style_height(&TextStyle::Body),
            self.dir_contents.borrow().get(&self.path).map_or(0, |files| files.len()),
            |ui, range| match self.dir_contents.borrow().get(&self.path) //borrow().get(&self.path) 
            {
                Some(files) => 
                {
                    ui.with_layout(ui.layout().with_main_justify(true), |ui| 
                    {
                        ui.vertical(|ui|{

                            for path in files[range].iter()
                            {
                                self.display_path
                                (
                                    ui, 
                                    path, 
                                    command_tx.clone(),
                                );
                            }
                        });
                    }).response
                }
                None => {
                    // There was an error fetching the directory contents
                    // Send a command to fetch them in the background
                    let command = Command::ReadDirectory(self.path.clone());
                    command_tx.send(Some(command)).unwrap();
                    ui.label("Loading...")
                },
            });
        });
        if let Ok(Some(cmd)) = command_rx.try_recv(){ block_on(async{self.run_command(cmd).await;});}
    }
    
    /** 
        Handles displaying of subcontents of given directory by calling list_subfolders
        and makes only directories collapsible so we can see its subcontents 
    */
    fn display_path(
        &self,
        ui: &mut Ui,
        path: &PathBuf,
        command_tx: channel::Sender<Option<Command>>,
    ){
        puffin::profile_scope!("display_path");
        // ui.separator();
        let command_sender = command_tx.clone();
        let command_sender2 = command_tx.clone();
        let command_sender3 = command_tx.clone();
        let command_sender4 = command_tx.clone();
        let command_sender5 = command_tx.clone();

        let label = match path.is_dir() {true => "🗀 ", false => "🗋 "}.to_string() + get_file_name(path);
        let mut formatted_size = "".to_string();            
        ui.horizontal_top(|ui| 
        {
            if path.is_dir() 
            {
                let id = ui.make_persistent_id(path.as_path().to_string_lossy());
                let modifiers = ui.input(|i| i.modifiers); // Get the current modifiers
        
                let contents = match self.dir_contents.borrow().get(path) 
                {
                    Some(contents) => contents.clone(),
                    None => {
                        let command = Command::ReadDirectory(path.clone()); // Contents are not cached, fetch in the background
                        match command_sender.send(Some(command)){
                            Ok(_) => drop(command_sender),
                            Err(e) => println!("error: {e:?}")
                        }
                        vec![] // Return an empty Vec for now
                    }
                };

                ui.vertical_centered_justified(|ui| {
                    CollapsingState::load_with_default_open(ui.ctx(), id, false)
                    .show_header(ui, |ui| 
                    {
                        let is_selected = self.selected_items.borrow().contains(path);
                        let selectable_label = ui.selectable_label(is_selected, &label);
                    
                        if selectable_label.secondary_clicked() && !self.folder_metadata.borrow().contains_key(path){
                            match command_sender5.send(Some(Command::ReadMetadata(path.clone()))) {
                                Ok(_) => drop(command_sender5),
                                Err(e) => println!("hovered sender error: {e:?}"),
                            }
                        } 
                        if let Some(metadata) = self.folder_metadata.borrow_mut().get(path)
                        {
                            let path_size = metadata.path_size;
                            formatted_size = format_path_metadata(path_size);
                            let mut job = LayoutJob::default();
                            let mut text_formatting = TextFormat::default();
                            text_formatting.color = Color32::DARK_GRAY;
                            text_formatting.italics = true;
                            job.halign = Align::RIGHT;
                            job.justify = true;
                            
                            let text = formatted_size.to_string();
                            job.append(&text, 30.0, text_formatting);
                            
                            let x = WidgetText::LayoutJob(job).small().background_color(Color32::RED);
                            ui.add_space(ui.available_size_before_wrap().x - 100.0);
                            ui.add(Label::new(x));
                        }

                        if selectable_label.clicked() 
                        { // If the item was already selected, deselect it
                            if self.selected_items.borrow().contains(path) { self.selected_items.borrow_mut().remove(path); } 
                            // If the control key is down and the item was not selected, select it
                            if modifiers.ctrl { self.selected_items.borrow_mut().insert(path.clone()); } 
                            else 
                            {// If the control key is not down, clear previous selection and select the current item
                                self.selected_items.borrow_mut().clear();
                                self.selected_items.borrow_mut().insert(path.clone());
                            }
                        }
            
                        if selectable_label.double_clicked() 
                        { //|| selectable_label.ctx.input(|state| state.key_pressed(Key::Enter))
                            match command_sender2.send(Some(Command::OpenPath(path.clone()))) {
                                Ok(_) => drop(command_sender2),
                                Err(e) => println!("error: {e:?}"),
                            }
                        }

                    }).body(|ui| 
                    {
                        for sub_path in &contents {
                            self.display_path(
                                ui,
                                &sub_path,
                                command_tx.clone()
                            );
                        }
                    });
                });

            } 
            else if !path.is_dir() && !self.read_dirs_only{
                let is_selected = self.selected_items.borrow().contains(path);
                let modifiers = ui.input(|i| i.modifiers); // Get the current modifiers
                
                let selectable_label = ui.selectable_label(is_selected, &label);
                if selectable_label.clicked() {
                    match command_sender3.send(Some(Command::Select(path.clone()))) {
                        Ok(_) => drop(command_sender3),
                        Err(e) => println!("error: {e:?}"),
                    }
                    // If the control key is down and the item was not selected, select it 
                    if modifiers.ctrl { self.selected_items.borrow_mut().insert(path.clone());} 
                    if self.selected_items.borrow().contains(path) {
                        // If the item was already selected, deselect it
                        self.selected_items.borrow_mut().remove(path);
                    } 
                    else { // If the control key is not down, clear previous selection and select the current item
                        self.selected_items.borrow_mut().clear();
                        self.selected_items.borrow_mut().insert(path.clone());
                    }
                }else if selectable_label.secondary_clicked() {

                }
                
                if let Some(metadata) = self.file_metadata.borrow_mut().get(path)
                {
                    let path_size = metadata.path_size;
                    formatted_size = format_path_metadata(path_size);
                    let mut job = LayoutJob::default();
                    let mut text_formatting = TextFormat::default();
                    text_formatting.color = Color32::DARK_GRAY;
                    text_formatting.italics = true;
                    job.halign = Align::RIGHT;
                    job.justify = true;
                    
                    let text = format!("{}", formatted_size.as_str());
                    job.append(&text, 30.0, text_formatting);
                    
                    let x = WidgetText::LayoutJob(job).small().background_color(Color32::RED);
                    ui.add_space(ui.available_size_before_wrap().x - 100.0);
                    ui.add(Label::new(x));
                }
            }
        });

    }

    fn default_filename(mut self, filename: impl Into<String>) -> Self {
        self.filename_edit = filename.into();
        self
    }

    /**  Resulting file path. */
    fn path(&self) -> Option<PathBuf> {
        self.selected_item.clone()
    }

    /** Set the dialog's current opened path */
    fn set_path(&mut self, path: impl Into<PathBuf>) {
        self.path = path.into();
        self.refresh_contents();
    }

    /**
        Refreshes current directory upon 
        changing directory, or double clicking
        a folder
    */ 
    fn refresh_contents(&mut self) {
        puffin::profile_scope!("refresh_contents");
        let new_contents = read_folder(
            &self.path,
            self.depth,
            self.read_dirs_only,
        );
        self.dir_contents.borrow_mut().insert(self.path.clone(), new_contents);
        self.path_edit = self.path.to_string_lossy().to_string();
        //self.select(None);
    }

    /**  
        Updates the textedit which displays the 
        currently selected file or folder
    */
    fn select(&mut self, file: PathBuf) {//fn select(&mut self, file: Option<PathBuf>) {
        
        self.filename_edit = match &file {
            path => get_file_name(path).to_string(),
            //None => String::new(),
        };
        self.selected_item = Some(file.as_path().to_path_buf());
        self.selected_items.borrow_mut().insert(file);
    }

    fn deselect(&mut self, file: PathBuf) {
        self.selected_items.borrow_mut().remove(&file);
    }
    
    /**
        Makes the double clicked directory 
        the new current directory via set_path
    */
    fn open_path(&mut self) {
        if let Some(path) = &self.selected_item {
          if path.is_dir() {
            self.set_path(path.clone())
          } else if path.is_file() {
            //self.confirm();
          }
        }
      }

    /**
        Checks whether or not we can rename 
        the directory by making sure the 
        filename_edit (bottom textedit) is not
        empty 
    */
    fn can_rename(&self) -> bool {
        if !self.filename_edit.is_empty() {
            if let Some(file) = &self.selected_item {
            return get_file_name(file) != self.filename_edit;
            }
        }
        false
    }
    
    /** Returns the path of the folder or file 
    */
    fn get_folder(&self) -> &std::path::Path {
        if let Some(file) = &self.selected_item {
            if file.is_dir() {
                return file.as_path();
            }
        }
        // No selected file or it's not a folder, 
        // so use the current path.
        &self.path 
    }

    fn get_drives(&mut self) {
        let mut disks = Disks::new_with_refreshed_list();
        

        for disk in &mut disks{
            self.drive_letters.push(disk.mount_point().to_str().unwrap_or("").to_string());  
            
        }
    }
}

#[cfg(windows)]
fn is_drive_root(path: &PathBuf) -> bool {
  path
    .to_str()
    .filter(|path| &path[1..] == ":\\")
    .and_then(|path| path.chars().next())
    .map_or(false, |ch| ch.is_ascii_uppercase())
}

fn get_file_name(path: &PathBuf) -> &str {
    #[cfg(windows)]
    if path.is_dir() && is_drive_root(path) {
      return path.to_str().unwrap_or_default();
    }
    path
      .file_name()
      .and_then(|name| name.to_str())
      .unwrap_or_default()
}
   
#[cfg(windows)]
extern "C" {
    pub fn GetLogicalDrives() -> u32;
}

/** Returns a Vec<PathBuf> of current directory contents and files. */
fn read_folder(path: &PathBuf, depth: usize, read_dirs_only: bool) -> Vec<PathBuf> {
    let result: Vec<_> = WalkDir::new(path).min_depth(depth).max_depth(depth)
        .into_iter()
        .filter_map(|e| e.ok()) // Only retreive the resulted items
        .filter(|entry| !read_dirs_only || entry.path().is_dir()) // Include only directories if read_dirs_only is true
        .map(|entry| entry.path().to_path_buf())// iterate through each direntry
        .collect();
    let mut result = result;

    result.sort_by(|a, b| {
        let da = a.is_dir();
        let db = b.is_dir();
        match da == db {
          true => a.file_name().cmp(&b.file_name()),
          false => db.cmp(&da),
        }
    });

    let result = result
    .into_iter()
    .filter(|path| {
        if !path.is_dir() {
            // Do not show system files.
            if !path.is_file() {
                return false;
            }
        }
        #[cfg(unix)]
        if !show_hidden && get_file_name(path).starts_with('.') {
            return false;
        }
        true
    })
    .collect();

    result
}

fn circle_icon(ui: &mut Ui, openness: f32, response: &Response) {
    let stroke = ui.style().interact(&response).fg_stroke;
    let radius = lerp(2.0..=3.0, openness);
    ui.painter().circle_filled(response.rect.center(), radius, stroke.color);
}


// async fn copy_files(source: Vec<PathBuf>, destination: &PathBuf, progress_tx: channel::Sender<u64>) -> io::Result<()>{
//     if !destination.exists(){
//         fs::create_dir_all(destination).await?;
//     }
//     for entry in source{
//         println!("Path: {entry:?}");
//         let x = fs::read_dir(entry).await?;
//         while let Some(entry) = x.next_entry().await? {
//             let mut src = BufReader::with_capacity(8192, entry.path());
//             let src_path = entry.path();
//             let dst_path = destination.join(entry.file_name());

//             match io::copy(&mut src, &mut dest).await {
//                 Ok(bytes_copied) => progress_tx.try_send(bytes_copied).unwrap(),
//                 Err(e) => debug!("{e:?}")
//             }

//             if src_path.is_dir() {
//                 // copy_recursively_in_chunks(src_path, dst_path, 8196).await
//             } else {
//                 // copy_file_in_chunks(src_path, dst_path, 8196).await
//             }
//         }

        
//         let mut dest = BufWriter::with_capacity(8192, fs::File::open(destination).await?);


//     }
//     Ok(())
// }

