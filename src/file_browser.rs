use tokio::sync::mpsc::{channel, Sender, Receiver, UnboundedReceiver, UnboundedSender, unbounded_channel};
use eframe::egui::{*, collapsing_header::CollapsingState};
use std::{path::PathBuf, sync::{Arc, Mutex}, collections::{HashSet, HashMap}, cell::RefCell};
use tokio::{task, fs};
use walkdir::WalkDir;
use std::{env, io::Error};
use pollster::block_on;
use crossbeam;

/// Function that returns `true` if the path is accepted.
pub type Filter = Box<dyn Fn(&PathBuf) -> bool + Send + Sync + 'static>;

#[derive(Debug)]
pub enum Command {
    Copy(PathBuf, PathBuf),
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
}

pub struct FileBrowser {
    path: PathBuf, /// Current opened path.
    path_edit: String, /// Editable field with path.
    selected_item: Option<PathBuf>, /// Selected file path
    filename_edit: String, /// Editable field with filename.
    files: core::result::Result<Vec<PathBuf>, Error>, /// Files in directory.
    read_dirs_only: bool,
    show_hidden: bool,
    rename: bool,
    new_folder: bool,

    selected_items: RefCell<HashSet<PathBuf>>,
    dir_contents: RefCell<HashMap<PathBuf, Vec<PathBuf>>>,
    
    filter: Option<Filter>,
    depth: usize,
    double_clicked_directory: Option<PathBuf>,

    first_refresh_contents: bool,

    command_rx: Option<Receiver<Option<Command>>>,
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

        let (_, command_rx) = channel(4);

        Self {
            path,
            path_edit,
            selected_item: None,

            selected_items: RefCell::new(HashSet::new()),
            dir_contents: RefCell::new(HashMap::new()),

            filename_edit,
            files: Ok(Vec::new()),
            read_dirs_only: false,
            rename: true,
            new_folder: true,
            show_hidden: false,
            first_refresh_contents: true,
            depth: 1,
            double_clicked_directory: None,
            filter: None,
            command_rx: Some(command_rx),
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

            Command::Copy(source, destination) => {
                println!("Command::copy");
                if let Err(err) = fs::copy(&source, &destination).await {
                    //let _ = response_sender.send(Response::Error(FileBrowserError::Io(err)));
                } else {
                    //let _ = response_sender.send(Response::Success(format!("Successfully copied from {:?} to {:?}", source, destination)));
                }
            
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

        }
    }

    pub fn show(
        &mut self, 
        ui: &mut egui::Ui,
        ctx:&egui::Context,
        command_tx: Sender<Option<Command>>,
        mut command_rx: Receiver<Option<Command>>
    ) {     
        egui::TopBottomPanel::top("egui_file_top").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add_enabled_ui(self.path.parent().is_some(), |ui| {
                    let response = ui.button("⬆").on_hover_text("Parent Folder"); //
                    if response.clicked() {
                        match command_tx.try_send(Some(Command::UpDirectory)){
                            Ok(_) => {
                                println!("sent task successfully");
                            },
                            Err(e) => {
                                print!("{e}");
                            }
                        }
                    }
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                
                    let response = ui.button("⟲").on_hover_text("Refresh"); //
                    if response.clicked() {
                        match command_tx.try_send(Some(Command::Refresh)){
                            Ok(_) => {
                                println!("sent task successfully");
                            },
                            Err(e) => {
                                print!("{e}");
                            }
                        }
                    }
                    egui::ScrollArea::new([false, false]).auto_shrink([false, false]).show(ui, |ui| {
                        let response = ui.add_sized(
                            ui.available_size_before_wrap(),
                            egui::TextEdit::singleline(&mut self.path_edit)
                                .id(egui::Id::new("path_edit"))
                                .cursor_at_end(true),
                        );
                        if response.lost_focus() && response.ctx.input(|state| state.key_pressed(egui::Key::Enter)) {
                            let path = PathBuf::from(&self.path_edit);

                            match command_tx.try_send(Some(Command::OpenPath(path))){
                                Ok(_) => {
                                    println!("sent task successfully");
                                },
                                Err(e) => {
                                    print!("{e}");
                                }
                            };

                        }
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

        egui::TopBottomPanel::bottom("egui_file_bottom").show_inside(ui, |ui| {
            ui.add_space(ui.spacing().item_spacing.y * 2.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.new_folder && ui.button("New Folder").clicked() {
                        match command_tx.try_send(Some(Command::CreateDirectory)){
                            Ok(_) => {
                                println!("ok");
                            },
                            Err(e) => {
                                print!("{e}");
                            }
                        }
                        
                    }

                    if self.rename {

                        ui.add_enabled_ui(self.can_rename(), |ui| {
                            if ui.button("Rename").clicked() {
                            if let Some(from) = self.selected_item.clone() {
                                let to = from.with_file_name(&self.filename_edit);
                                
                                match command_tx.try_send(Some(Command::Rename(from, to))){
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
                    egui::TextEdit::singleline(&mut self.filename_edit)
                    .id(egui::Id::new("file_name_edit")),
                    );

                    if result.lost_focus()
                    && result
                        .ctx
                        .input(|state| state.key_pressed(egui::Key::Enter))
                    && !self.filename_edit.is_empty(){
                        let path = self.path.join(&self.filename_edit);

                    }
                });
            });
        });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.visuals_mut().override_text_color = Some(egui::Color32::from_rgb(255, 204, 230));
            //ui.style_mut().spacing.button_padding = (4.0, 5.0).into();
            ui.shrink_width_to_current();ui.shrink_height_to_current();
            ui.painter().rect_filled(ui.available_rect_before_wrap(),10.0,egui::Color32::from_rgb(28,30,36));
            ui.painter().rect_stroke(ui.available_rect_before_wrap(),10.0, egui::Stroke::new(1.0, egui::Color32::from_rgb_additive(150, 62, 124)));

            ui.add_space(ui.spacing().item_spacing.y * 2.0);

            if self.first_refresh_contents == true{
                self.refresh_contents();
            }

            egui::ScrollArea::new([true, true])
            .id_source("file_browser_scroll")
            .auto_shrink([false, false])
            .show_rows(ui,
            ui.text_style_height(&egui::TextStyle::Body),
            self.dir_contents.borrow().get(&self.path).map_or(0, |files| files.len()),
            |ui, range| match self.dir_contents.borrow().get(&self.path) {
                Some(files) => {
                    ui.with_layout(ui.layout().with_main_justify(true), |ui| {
                        ui.vertical(|ui|{
                            for path in files[range].iter() {
                                display_path(ui, path, &self.selected_items, self.depth, command_tx.clone(), &self.dir_contents);
                            }
                        });
                    }).response
                }
                None => {
                    // There was an error fetching the directory contents
                    // Send a command to fetch them in the background
                    let command = Command::ReadDirectory(self.path.clone());
                    command_tx.try_send(Some(command)).unwrap();
                    ui.label("Loading...")
                },
            },
        );
        
        
        });

        if let Ok(Some(cmd)) = command_rx.try_recv(){
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
        // self.files = Ok(read_folder(
        //     &self.path,
        //     self.depth,
        //     self.filter.as_ref(),
        //     self.show_hidden,
        //   ));
        // self.path_edit = String::from(self.path.to_str().unwrap_or_default());
        let new_contents = read_folder(
            &self.path,
            self.depth,
            self.filter.as_ref(),
            self.show_hidden,
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
fn read_folder(path: &PathBuf, depth: usize, filter: Option<&Filter>, show_hidden: bool) -> Vec<PathBuf> {
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

/** Returns a Receiver containing a Vec<PathBuf> of subcontents of a given directory. */

/** 
    Handles displaying of subcontents of given directory by calling list_subfolders
    and makes only directories collapsible so we can see its subcontents 
 */
fn display_path(
    ui: &mut egui::Ui,
    path: &PathBuf,
    selected_items: &RefCell<HashSet<PathBuf>>,
    depth: usize,
    command_tx: Sender<Option<Command>>,
    dir_contents: &RefCell<HashMap<PathBuf, Vec<PathBuf>>>,
) {
    let label = match path.is_dir() {
        true => "🗀 ",
        false => "🗋 ",
    }
    .to_string()
    + get_file_name(path);

    if path.is_dir() {
        let id = ui.make_persistent_id(path.as_path().to_string_lossy());

        
        let contents = match dir_contents.borrow().get(path) {
            Some(contents) => contents.clone(),
            None => {
                // Contents are not cached, fetch in the background
                let command = Command::ReadDirectory(path.clone());
                match command_tx.try_send(Some(command)){
                    Ok(_) => println!("sent successfully"),
                    Err(e) => println!("error: {e:?}")
                }
                vec![] // Return an empty Vec for now
            }
        };

        CollapsingState::load_with_default_open(ui.ctx(), id.into(), false)
            .show_header(ui, |ui| {
                let is_selected = selected_items.borrow().contains(path);
                let selectable_label = ui.selectable_label(is_selected, &label);

                if selectable_label.clicked() {
                    if selected_items.borrow().contains(path) {
                        // If the item was already selected, deselect it
                        selected_items.borrow_mut().remove(path);
                    } else {
                        // If the item was not selected, select it
                        selected_items.borrow_mut().insert(path.clone());
                    }
                }

                if selectable_label.double_clicked()
                    || selectable_label.ctx.input(|state| state.key_pressed(egui::Key::Enter))
                {
                    match command_tx.try_send(Some(Command::OpenPath(path.clone()))) {
                        Ok(_) => println!("Success"),
                        Err(e) => println!("error: {e:?}"),
                    }
                }
            })
            .body(|ui| {
                for sub_path in &contents {
                    display_path(
                        ui,
                        sub_path,
                        selected_items,
                        depth + 1,
                        command_tx.clone(),
                        dir_contents,
                    );
                }
            });
    } else {
        let is_selected = selected_items.borrow().contains(path);
        let selectable_label = ui.selectable_label(is_selected, &label);
        if selectable_label.clicked() {
            match command_tx.try_send(Some(Command::Select(path.clone()))) {
                Ok(_) => println!("Success"),
                Err(e) => println!("error: {e:?}"),
            }
        }
    }
}



// TODO
/* NOW i will need to find a way to keep track of the list of items being displayed
 * so they only display one time, so threads are not spawning all the time
 * Gamplan: this function has a lot of recursiveness, because i run
 * display_path inside of the for loop for every subdir, which could be a lot.
 ****
 * Watch the one guy who talked about the select! macro, and look at how
 * he cloned that broadcast receiver
 ****
 * I need to utilize multi threading to compute the directories, the hashset
 * to store the items (Caching) ((This should be what i return from this fn,
 * or send through a channel)), the Arc<Mutex<T>> if needed for passing 
 * info into spawned threads, 
 ****
 * READ::CROSSBEAM ---v
 * like channels for communication between threads, scoped threads, and 
 * various LOCK-FREE data structures. It's great for tasks where you need 
 * fine control over threads and concurrent computations.
 ****/

// if selectable_label.clicked() {
//     if self.selected_items.contains(path) {
//         self.selected_items.remove(path);
//     } else {
//         self.selected_items.insert(path.clone());
//     }
// }


/* fn list_subfolders(path: &PathBuf, depth: usize) -> UnboundedReceiver<Vec<PathBuf>>{
    // may need to create a channel of its own here to send and receive
    // children_items
    let (tx, rx) = unbounded_channel::<Vec<PathBuf>>();
    let path = path.clone();

    tokio::spawn(async move{
        let mut children_items = Vec::new();
        let children = WalkDir::new(path)
        .min_depth(depth)
        .max_depth(depth)
        .into_iter()
        .filter_map(|e| e.ok()); 
    
        for items in children {
            let sub_items = items.path().to_path_buf();
            children_items.push(sub_items);
        }
        match tx.send(children_items){
            Ok(x) => println!("ok: {x:?}"),
            Err(_) => println!("error")
        }
        
    });
    return rx;
}
 */