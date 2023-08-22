use egui_extras::{TableBuilder, Column};
use tokio::sync::mpsc::{UnboundedSender, UnboundedReceiver};
use eframe::egui::{*, collapsing_header::CollapsingState};
use std::{path::PathBuf, collections::{HashSet, HashMap}, cell::RefCell, ops::Range};
use tokio::fs;
use walkdir::WalkDir;
use std::env;
use pollster::block_on;
use crossbeam::channel;
use num_format::{Locale, ToFormattedString};
use fs_extra::dir::get_size;

/// Function that returns `true` if the path is accepted.
pub type Filter = Box<dyn Fn(&PathBuf) -> bool + Send + Sync + 'static>;

const KB_FROM_BYTES: u64 = 1024;
const MB_FROM_BYTES: u64 = 1024*1024;
const GB_FROM_BYTES: u64 = 1024*1024*1024;

#[derive(Debug)]
pub enum Command {
    Copy(PathBuf, PathBuf, channel::Sender<u64>),
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

}

pub struct MetaData{
    path_size: u64,
}

pub struct FileBrowser {
    path: PathBuf, /// Current opened path.
    path_edit: String, /// Editable field with path.
    selected_item: Option<PathBuf>, /// Selected file path
    filename_edit: String, /// Editable field with filename.
    read_dirs_only: bool,
    show_hidden: bool,
    rename: bool,
    new_folder: bool,
    selected_items: RefCell<HashSet<PathBuf>>,
    dir_contents: RefCell<HashMap<PathBuf, Vec<PathBuf>>>,
    filter: Option<Filter>,
    depth: usize,
    first_refresh_contents: bool,
    file_metadata: RefCell<HashMap<PathBuf, MetaData>>,
    metadata_tx: crossbeam::channel::Sender<u64>,
    metadata_rx: crossbeam::channel::Receiver<u64>,
}

impl FileBrowser{ // sender: UnboundedSender<>
    pub fn new() -> Self{
        let mut path = env::current_dir().unwrap_or_default();
        let mut filename_edit = String::new();

        let path_edit = path.to_str().unwrap_or_default().to_string();

        if path.is_file() {
            filename_edit = get_file_name(&path).to_string();
            path.pop();
        }
        
        let (metadata_tx, mut metadata_rx) = crossbeam::channel::unbounded();

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
            filter: None,
            metadata_tx,
            metadata_rx,
          }
    }
    
    pub async fn run_command(&mut self, command: Command) {
        match command{
            Command::Select(file) => self.select(file),

            Command::Folder => {
                println!("Command::Folder");
                self.selected_item = Some(self.get_folder().to_owned());
            },
            
            Command::Refresh => self.refresh_contents(),

            Command::UpDirectory => {
                println!("Command::UpDirectory");
                if self.path.pop() {
                    self.refresh_contents();
                }
            },

            Command::CreateDirectory => {
                println!("Command::CreateDirectory");
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
                        // TODO: scroll to selected?
                    }
                    Err(err) => println!("Error while creating directory: {err}"),
                }


            },

            Command::Copy(source, destination, progress_tx) => {
                println!("Command::copy");
                
                tokio::spawn(async move{
                    match fs::copy(&source, &destination).await {
                        Ok(bytes_copied) => progress_tx.send(bytes_copied).unwrap(),
                        Err(e) => println!("{e:?}")
                    }
                });

            
            },

            Command::Move(source, destination) => {
                println!("Command::Move");
                if let Err(err) = fs::rename(&source, &destination).await {
                    //let _ = response_sender.send(Response::Error(FileBrowserError::Io(err)));
                } else {
                    //let _ = response_sender.send(Response::Success(format!("Successfully moved from {:?} to {:?}", source, destination)));
                }
            
            },

            Command::Delete(path) => {
                println!("Command::Delete");
                if let Err(err) = fs::remove_dir_all(&path).await {
                    //let _ = response_sender.send(Response::Error(FileBrowserError::Io(err)));
                } else {
                    //let _ = response_sender.send(Response::Success(format!("Successfully deleted {:?}", path)));
                }
            },

            Command::Rename(from, to) => {
                println!("Command::Rename");
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
                    self.filter.as_ref(),
                    self.show_hidden,
                );
                self.dir_contents.borrow_mut().insert(path, new_contents);
            }

            Command::ReadMetadata(path) => {
                let sender = self.metadata_tx.clone();
                let cloned_path = path.clone();
                //let metadata = fs::metadata(path).expect("Failed to retrieve metadata");
                tokio::spawn(async move
                {
                    match sender.try_send(tokio::fs::metadata(&path).await.unwrap().len())
                    {
                        Ok(_) => drop(sender),
                        Err(e) => println!("{e}")
                    }
                });

                if let Ok(path_size) = self.metadata_rx.try_recv()
                {
                    self.file_metadata.borrow_mut().insert(cloned_path.clone(), MetaData{path_size});
                    //self.path_size = path_size;
                }
            }
        }
    }

    pub fn show(
        &mut self, 
        ui: &mut Ui,
        ctx:&Context,
        command_tx: UnboundedSender<Option<Command>>,
        mut command_rx: UnboundedReceiver<Option<Command>>
    ) {     
        TopBottomPanel::top("file_browser_top").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add_enabled_ui(self.path.parent().is_some(), |ui| {
                    let response = ui.button("⬆").on_hover_text("Parent Folder"); //
                    if response.clicked() {
                        match command_tx.send(Some(Command::UpDirectory)){
                            Ok(_) => {
                                println!("sent task successfully");
                            },
                            Err(e) => {
                                print!("{e}");
                            }
                        }
                    }
                });

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                
                    let response = ui.button("⟲").on_hover_text("Refresh"); //
                    if response.clicked() {
                        match command_tx.send(Some(Command::Refresh)){
                            Ok(_) => {
                                println!("sent task successfully");
                            },
                            Err(e) => {
                                print!("{e}");
                            }
                        }
                    }
                    ScrollArea::new([false, false]).auto_shrink([false, false]).show(ui, |ui| {
                        let response = ui.add_sized(
                            ui.available_size_before_wrap(),
                            TextEdit::singleline(&mut self.path_edit)
                                .id(Id::new("path_edit"))
                                .cursor_at_end(true),
                        ).on_hover_text(&self.path_edit);
                       
                        if response.lost_focus() {
                            let path = PathBuf::from(&self.path_edit);

                            match command_tx.send(Some(Command::OpenPath(path))){
                                Ok(_) => {
                                    println!("sent task successfully");
                                },
                                Err(e) => {
                                    print!("{e}");
                                }
                            };

                        }
                        /* 
                        else if response.lost_focus() && response.ctx.input(|state| state.key_pressed(Key::Enter)){
                            let path = PathBuf::from(&self.path_edit);

                            match command_tx.send(Some(Command::OpenPath(path))){
                                Ok(_) => {
                                    println!("sent task successfully");
                                },
                                Err(e) => {
                                    print!("{e}");
                                }
                            };
                        } 
                        */
                    });
                });
            });
            
            ui.horizontal_top(|ui| {
                ui.checkbox(&mut self.read_dirs_only, "Show Directories ONLY");
                if ui.checkbox(&mut self.show_hidden, "Show Hidden").changed() {
                    self.refresh_contents();
                }
            });
            ui.add_space(ui.spacing().item_spacing.y);
        });

        TopBottomPanel::bottom("file_browser_bottom").show_inside(ui, |ui| {
            ui.add_space(ui.spacing().item_spacing.y * 2.0);

            // let (progress_tx, progress_rx) = channel::unbounded::<u64>();
            // let src = PathBuf::new();
            // let source = src.join("D:\\Users\\Owner\\Desktop\\B.S.-10.5-Sized_B.S.-10.5.ctb");
            // let dest = PathBuf::new();
            // let destination = dest.join("D:\\Users\\Owner\\Desktop\\filestuff");
            // match command_tx.send(Some(
            //     Command::Copy(
            //         source, 
            //         destination, 
            //         progress_tx.clone()
            //     )
            // )){
            //     Ok(_) => println!("ok"),
            //     Err(e) => print!("{e}")
            // }
            // match progress_rx.try_recv() {
            //     Ok(bytes_copied) => {
            //         // Update the progress bar
            //         ui.add(
            //             ProgressBar::new(bytes_copied as f32)
            //             .show_percentage()
            //             .animate(true)
            //             .fill(Color32::from_rgb(255, 255, 255))
            //         );
            //         println!("Copied {bytes_copied} bytes", );
            //     }
            //     Err(channel::TryRecvError::Empty) => {
            //         // No progress update yet, do nothing
            //     }
            //     Err(channel::TryRecvError::Disconnected) => {
            //         // The file copying thread has finished
            //         println!("File copying finished");
            //     }
            // }


            ui.add_space(ui.spacing().item_spacing.y * 2.0);

            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if self.new_folder && ui.button("New Folder").clicked() {
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
                    // ui.available_size_before_wrap(),
                    TextEdit::singleline(&mut self.filename_edit)
                    .id(Id::new("file_name_edit")),
                    );

                    if result.lost_focus()
                    //&& result.ctx.input(|state| state.key_pressed(Key::Enter))
                    && !self.filename_edit.is_empty(){
                        let path = self.path.join(&self.filename_edit);

                    }
                });
            });
        });

        CentralPanel::default().show_inside(ui, |ui| {
            ui.visuals_mut().override_text_color = Some(Color32::from_rgb(255, 204, 230));
            //ui.style_mut().spacing.button_padding = (4.0, 5.0).into();
            ui.shrink_width_to_current();ui.shrink_height_to_current();
            ui.painter().rect_filled(ui.available_rect_before_wrap(),10.0,Color32::from_rgb(28,30,36));
            //ui.painter().rect_stroke(ui.available_rect_before_wrap(),10.0, Stroke::new(1.0, Color32::from_rgb_additive(150, 62, 124)));

            ui.add_space(ui.spacing().item_spacing.y * 2.0);

            if self.first_refresh_contents == true{
                self.refresh_contents();
            }
            
            match self.dir_contents.borrow().get(&self.path) 
            {
                Some(files) => 
                {
                    ui.with_layout(ui.layout().with_main_justify(true), |ui| 
                    {
                        display_path
                        (
                            ui, 
                            files, 
                            &self.selected_items, 
                            self.depth, 
                            command_tx.clone(), 
                            &self.dir_contents, 
                            self.read_dirs_only,
                            &self.file_metadata,
                        );
                        
                    }).response
                }
                None => {
                    // There was an error fetching the directory contents
                    // Send a command to fetch them in the background
                    let command = Command::ReadDirectory(self.path.clone());
                    command_tx.send(Some(command)).unwrap();
                    ui.label("Loading...")
                },
            };
        });
        if let Ok(Some(cmd)) = command_rx.try_recv(){
            // tokio::spawn(async move{
            //     self.run_command(cmd).await;
            // });
            block_on(async{self.run_command(cmd).await;});
        }
    }
    
    pub fn default_filename(mut self, filename: impl Into<String>) -> Self {
        self.filename_edit = filename.into();
        self
    }

    /**  Resulting file path. */
    pub fn path(&self) -> Option<PathBuf> {
        self.selected_item.clone()
    }

    /** Set the dialog's current opened path */
    pub fn set_path(&mut self, path: impl Into<PathBuf>) {
        self.path = path.into();
        self.refresh_contents();
    }

    /**
        Refreshes current directory upon 
        changing directory, or double clicking
        a folder
    */ 
    fn refresh_contents(&mut self) {
        let new_contents = read_folder(
            &self.path,
            self.depth,
            self.filter.as_ref(),
            self.read_dirs_only,
        );
        self.dir_contents.borrow_mut().insert(self.path.clone(), new_contents);
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
    
    /** Returns the path of the folder or file */
    fn get_folder(&self) -> &std::path::Path {
        if let Some(file) = &self.selected_item {
            if file.is_dir() {
                return file.as_path();
            }
        }
    &self.path // No selected file or it's not a folder, so use the current path.
    }
    
    /// Set a function to filter shown files.
    pub fn filter(mut self, filter: Filter) -> Self {
        self.filter = Some(filter);
        self
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
fn read_folder(path: &PathBuf, depth: usize, filter: Option<&Filter>, read_dirs_only: bool) -> Vec<PathBuf> {
    //#[cfg(windows)]
    // let drives = {
    //   let mut drives = unsafe { GetLogicalDrives() };
    //   let mut letter = b'A';
    //   let mut drive_names = Vec::new();
    //   while drives > 0 {
    //     if drives & 1 != 0 {
    //       drive_names.push(format!("{}:\\", letter as char).into());
    //     }
    //     drives >>= 1;
    //     letter += 1;
    //   }
    //   drive_names
    // };


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

    #[cfg(windows)]
    // let result = {
    //     let mut items = drives;
    //     items.reserve(result.len());
    //     items.append(&mut result);
    //     items
    // };

    let result = result
    .into_iter()
    .filter(|path| {
        if !path.is_dir() {
            // Do not show system files.
            if !path.is_file() {
                return false;
            }
            // Filter.
            if let Some(filter) = filter.as_ref() {
                if !filter(path) {
                    return false;
                }
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

/** 
    Handles displaying of subcontents of given directory by calling list_subfolders
    and makes only directories collapsible so we can see its subcontents 
 */
fn display_path(
    ui: &mut Ui,
    files: &Vec<PathBuf>,
    selected_items: &RefCell<HashSet<PathBuf>>,
    depth: usize,
    command_tx: UnboundedSender<Option<Command>>,
    dir_contents: &RefCell<HashMap<PathBuf, Vec<PathBuf>>>,
    show_dirs_only: bool,
    file_metadata: &RefCell<HashMap<PathBuf, MetaData>>,
) {
    ui.vertical_centered_justified(|ui: &mut Ui|
        {
            ui.vertical(|ui|
            {
                ScrollArea::new([false, true])
                .stick_to_bottom(true)
                .auto_shrink([true, true])
                .show(ui, |ui| {
                    Grid::new("filebrowser_columns")
                    .max_col_width(500.0)
                    .num_columns(2)
                    .striped(true)
                    .show(ui, |ui| 
                    {
                        for path in files.iter(){
                            let command_sender = command_tx.clone();
                            let command_sender2 = command_tx.clone();
                            let command_sender3 = command_tx.clone();
                            let command_sender4 = command_tx.clone();
                    
                            let label = match path.is_dir() {true => "🗀 ", false => "🗋 "}.to_string() + get_file_name(path);
                
                            if path.is_dir() 
                            {
                                let id = ui.make_persistent_id(path.as_path().to_string_lossy());
                                let modifiers = ui.input(|i| i.modifiers); // Get the current modifiers
                        
                                let contents = match dir_contents.borrow().get(path) 
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
                                
                                ui.vertical(|ui|
                                {
                                    CollapsingState::load_with_default_open(ui.ctx(), id.into(), false)
                                    .show_header(ui, |ui| 
                                    {
                                        let is_selected = selected_items.borrow().contains(path);
                                        let selectable_label = ui.selectable_label(is_selected, &label);
                                        
                                        if selectable_label.clicked() 
                                        { // If the item was already selected, deselect it
                                            if selected_items.borrow().contains(path) { selected_items.borrow_mut().remove(path); } 
                                            // If the control key is down and the item was not selected, select it
                                            if modifiers.ctrl { selected_items.borrow_mut().insert(path.clone()); } 
                                            else 
                                            {// If the control key is not down, clear previous selection and select the current item
                                                selected_items.borrow_mut().clear();
                                                selected_items.borrow_mut().insert(path.clone());
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
                                        display_path(
                                            ui,
                                            &contents,
                                            selected_items,
                                            depth + 1,
                                            command_tx.clone(),
                                            dir_contents,
                                            show_dirs_only,
                                            &file_metadata
                                        );
                                        //
                                    });
                                });
                            } 
                            else if !path.is_dir() && show_dirs_only == false{
                                let is_selected = selected_items.borrow().contains(path);
                                let modifiers = ui.input(|i| i.modifiers); // Get the current modifiers
                        
                                let selectable_label = ui.selectable_label(is_selected, &label);
                                if selectable_label.clicked() {
                                    match command_sender3.send(Some(Command::Select(path.clone()))) {
                                        Ok(_) => drop(command_sender3),
                                        Err(e) => println!("error: {e:?}"),
                                    }
                                    // If the control key is down and the item was not selected, select it 
                                    if modifiers.ctrl { selected_items.borrow_mut().insert(path.clone());} 
                                    if selected_items.borrow().contains(path) {
                                        // If the item was already selected, deselect it
                                        selected_items.borrow_mut().remove(path);
                                    } 
                                    else { // If the control key is not down, clear previous selection and select the current item
                                        selected_items.borrow_mut().clear();
                                        selected_items.borrow_mut().insert(path.clone());
                                    }
                                }
                                if !file_metadata.borrow().contains_key(path){
                                    match command_sender4.send(Some(Command::ReadMetadata(path.clone()))) {
                                        Ok(_) => drop(command_sender4),
                                        Err(e) => println!("hovered sender error: {e:?}"),
                                    }
                                }
                                if let Some(metadata) = file_metadata.borrow_mut().get(path)
                                {
                                    let path_size = metadata.path_size;
                                    let formatted_size = format_path_metadata(path_size);
                                    ui.label(&formatted_size);
                                }
                            }
                            ui.end_row();
                        }
                        //ui.end_row();
                    });
                });

            });
        });

}

fn format_path_metadata(mut path_size: u64) -> String{
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
        format!("{formatted_size}")
    }
    else {
        format!("Nothing returned")
    }

}


// if selectable_label.hovered(){     
    // match command_sender3.send(Some(Command::ReadMetadata(path.clone()))) {
    //     Ok(_) => drop(command_sender3),
    //     Err(e) => println!("hovered sender error: {e:?}"),
    // }
                    // The below works, but it will get extremely resource heavy when hovering.. i may 
                    // need to spawn another thread and only read the directories sizes once,
                    // and display them all in the same line
// let mut folder_size: u64 = get_size(path).unwrap();
// let folder_name = path.file_name();
// println!("File: {folder_name:?}\n ---- 🗀 {folder_size}\n");
// if folder_size > 0{
//     if folder_size > GB_FROM_BYTES
//     {
//         let mut x = folder_size as f32;
//         x  = &x / GB_FROM_BYTES as f32;
//         let two_decimal_places = (x*100.0).round() / 100.0;
//         let x_as_string = two_decimal_places.to_string();
//         let y: Vec<&str> = x_as_string.split(".").collect();
//         let decimal = y[1].as_str();
//         let new_folder_size = x.clone() as u64;
//         formatted_size = format!("🗀 {}.{decimal} Gb", new_folder_size.to_formatted_string(&Locale::en));
//     }
//     else if folder_size > MB_FROM_BYTES
//     {
//         folder_size = folder_size / MB_FROM_BYTES;
//         formatted_size = format!("🗀 {} Mb", folder_size.to_formatted_string(&Locale::en));
//     } 
//     else if folder_size > KB_FROM_BYTES
//     {
//         folder_size = folder_size / KB_FROM_BYTES;
//         formatted_size = format!("🗀 {} Kb", folder_size.to_formatted_string(&Locale::en));
//         println!("kb from bytes: {folder_size}");
//     }
//     else{
//         formatted_size = format!("🗀 {} bytes", folder_size.to_formatted_string(&Locale::en));
//     }
// }
// selectable_label.on_hover_text(formatted_size);
// }