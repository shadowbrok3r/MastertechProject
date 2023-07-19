use std::io::{stdin, stdout, Write};
use std::path::PathBuf;
use tokio::{task, fs};
use walkdir::WalkDir;
use tokio::{sync::mpsc::{UnboundedSender, unbounded_channel, UnboundedReceiver}};
use std::{env, io::{Error, Result}};
use thiserror::Error;

use egui::{
    vec2, Align2, Context, Key, Layout, Pos2, RichText, ScrollArea, TextEdit, Ui, Vec2, Window, Color32, Stroke
};

/// Function that returns `true` if the path is accepted.
//pub type Filter = Box<dyn Fn(&PathBuf) -> bool + Send + Sync + 'static>;

#[derive(Debug)]
pub enum Response {
    DirectoryListing(Directory),
    Success(String),
    Error(FileBrowserError),
}

#[derive(Error, Debug)]
pub enum FileBrowserError {
    #[error("I/O error")]
    Io(#[from] Error),
    #[error("WalkDir error")]
    WalkDir(#[from] walkdir::Error),
    #[error("Other error: {0}")]
    Other(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Directory {
    pub path: PathBuf,
    pub children: Vec<Directory>,
    pub files: Vec<PathBuf>,
}

pub struct CommandControl {
    command_sender: UnboundedSender<Command>,
    response_receiver: UnboundedReceiver<Response>,
}

impl CommandControl {
    pub fn new() -> Self {
        let (command_sender, command_receiver) = unbounded_channel::<Command>();
        let (response_sender, response_receiver) = unbounded_channel::<Response>();

        tokio::spawn(async move {
            //file_browsing(command_receiver, response_sender).await;
        });

        Self {
            command_sender,
            response_receiver,
        }
    }

    pub fn get_sender(&self) -> UnboundedSender<Command> {
        self.command_sender.clone()
    }

    pub fn get_receiver(&mut self) -> &mut UnboundedReceiver<Response> {
        &mut self.response_receiver
    }
}

#[derive(Debug)]
pub enum Command {
    Copy(PathBuf, PathBuf),
    Move(PathBuf, PathBuf),
    Delete(PathBuf),
    Rename(PathBuf, PathBuf),
    //ListDir(PathBuf, usize),

    CreateDirectory,
    Folder,
    Refresh,
    Select(PathBuf),
    UpDirectory,
}

#[derive(Debug)]
pub struct FileBrowser {
    path: PathBuf, /// Current opened path.
    path_edit: String, /// Editable field with path.
    selected_file: Option<PathBuf>, /// Selected file path
    filename_edit: String, /// Editable field with filename.
    files: core::result::Result<Vec<PathBuf>, Error>, /// Files in directory.
    read_dirs_only: bool,
    read_hidden_files: bool,
    //filter: Option<Filter>,
    rename: bool,
    new_folder: bool,
  
    // Show hidden files on unix systems.
    //#[cfg(unix)]
    show_hidden: bool,
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

        Self {
            path,
            path_edit,
            selected_file: None,
            filename_edit,
            files: Ok(Vec::new()),
            read_dirs_only: false,
            read_hidden_files: false,

            //filter: None,
            rename: true,
            new_folder: true,
    
            //#[cfg(unix)]
            show_hidden: false,
          }
    }

    pub fn init(&mut self){
       
    }

    pub async fn show(&mut self, ctx: &Context) { //-> core::result::Result<(), Box<dyn std::error::Error>>{

        let mut command: Option<Command> = None;

        egui::TopBottomPanel::top("egui_file_top").show(&ctx, |ui| {
            ui.visuals_mut().override_text_color = Some(Color32::from_rgb(255, 204, 230));
            ui.style_mut().spacing.button_padding = (4.0, 5.0).into();
            ui.set_min_width(600.0);
            ui.set_max_height(600.0);
            ui.shrink_width_to_current();
            ui.shrink_height_to_current();
            ui.painter().rect_filled(ui.available_rect_before_wrap(),10.0,Color32::from_rgb(28,30,36));
            ui.painter().rect_stroke(ui.available_rect_before_wrap(),10.0, Stroke::new(1.0, Color32::from_rgb_additive(150, 62, 124)));

            ui.horizontal(|ui| {
                ui.add_enabled_ui(self.path.parent().is_some(), |ui| {
                    let response = ui.button("⬆").on_hover_text("Parent Folder"); //
                    if response.clicked() {
                        command = Some(Command::UpDirectory);
                    }
                });

                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                
                    let response = ui.button("⟲").on_hover_text("Refresh"); //
                    if response.clicked() {
                        command = Some(Command::Refresh);
                    }

                    let response = ui.add_sized(
                    ui.available_size(),
                    TextEdit::singleline(&mut self.path_edit).cursor_at_end(true),
                    );

                    if response.lost_focus() {
                        let path = PathBuf::from(&self.path_edit);
                        //command = Some(Command::Open(path));
                    };

                });
            });
            
            ui.horizontal_top(|ui| {
                ui.checkbox(&mut self.read_dirs_only, "Show Directories ONLY");
                ui.checkbox(&mut self.read_hidden_files, "Show hidden files");
            });
            ui.add_space(ui.spacing().item_spacing.y);
        });

        // Bottom file field.
        egui::TopBottomPanel::bottom("egui_file_bottom").show(&ctx, |ui| {
            ui.add_space(ui.spacing().item_spacing.y * 2.0);
            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.new_folder && ui.button("New Folder").clicked() {
                        command = Some(Command::CreateDirectory);
                    }

                    if self.rename {
                        ui.add_enabled_ui(self.can_rename(), |ui| {
                            if ui.button("Rename").clicked() {
                            if let Some(from) = self.selected_file.clone() {
                                let to = from.with_file_name(&self.filename_edit);
                                command = Some(Command::Rename(from, to));
                            }
                            }
                        });
                    }

                    let result = ui.add_sized(
                    ui.available_size(),
                    TextEdit::singleline(&mut self.filename_edit),
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

        egui::CentralPanel::default().show(&ctx, |ui| {
            ui.add_space(ui.spacing().item_spacing.y * 2.0);
            let scroll_area = ScrollArea::new([true, true])
                .id_source("file_browser_scroll")
                .auto_shrink([false, false]);
    
            scroll_area.show_rows(
            ui,
            ui.text_style_height(&egui::TextStyle::Body),
    self.files.as_ref().map_or(0, |files| files.len()),
    |ui, range| match self.files.as_ref() {
                    Ok(files) => {
                        ui.with_layout(ui.layout().with_cross_justify(true), |ui| {
                            for path in files[range].iter() {
                                let label = match path.is_dir() {
                                    true => "🗀 ",
                                    false => "🗋 ",
                                }.to_string() + get_file_name(path);
                
                                let is_selected = Some(path) == self.selected_file.as_ref();
                                let selectable_label = ui.selectable_label(is_selected, label);
                                if selectable_label.clicked() {
                                    command = Some(Command::Select(path.clone()));
                                }
                
                                if selectable_label.double_clicked() {
                                    // command = Some(match self.dialog_type == DialogType::SaveFile {
                                    // true => match path.is_dir() {
                                    //     true => Command::OpenSelected,
                                    //     false => Command::Save(path.clone()),
                                    // },
                                    // false => Command::Open(path.clone()),
                                    // });
                                }
                            }
                        }).response
                    }
                    Err(e) => ui.label(e.to_string()),
                },
            );
        });
        if let Some(command) = command {
            match command {
                Command::Select(file) => self.select(Some(file)),

                Command::Folder => {
                    //self.selected_file = Some(self.get_folder().to_owned());
                }
                
                Command::Refresh => self.refresh(),

                Command::UpDirectory => {
                    if self.path.pop() {
                        self.refresh();
                    }
                }

                Command::CreateDirectory => {
                    let mut path = self.path.clone();
                    let name = match self.filename_edit.is_empty() {
                        true => "New folder",
                        false => &self.filename_edit,
                    };
                    path.push(name);
                    match fs::create_dir(&path).await {
                        Ok(_) => {
                        self.refresh();
                        self.select(Some(path));
                        // TODO: scroll to selected?
                        }
                        Err(err) => println!("Error while creating directory: {err}"),
                    }
                }
                Command::Copy(source, destination) => {
                    if let Err(err) = fs::copy(&source, &destination).await {
                        //let _ = response_sender.send(Response::Error(FileBrowserError::Io(err)));
                    } else {
                        //let _ = response_sender.send(Response::Success(format!("Successfully copied from {:?} to {:?}", source, destination)));
                    }
                },
                Command::Move(source, destination) => {
                    if let Err(err) = fs::rename(&source, &destination).await {
                        //let _ = response_sender.send(Response::Error(FileBrowserError::Io(err)));
                    } else {
                        //let _ = response_sender.send(Response::Success(format!("Successfully moved from {:?} to {:?}", source, destination)));
                    }
                },
                Command::Delete(path) => {
                    if let Err(err) = fs::remove_dir_all(&path).await {
                        //let _ = response_sender.send(Response::Error(FileBrowserError::Io(err)));
                    } else {
                        //let _ = response_sender.send(Response::Success(format!("Successfully deleted {:?}", path)));
                    }
                },

                // Command::Rename(source, destination) => {
                //     if let Err(err) = fs::rename(&source, &destination).await {
                //         let _ = response_sender.send(Response::Error(FileBrowserError::Io(err)));
                //     } else {
                //         let _ = response_sender.send(Response::Success(format!("Successfully renamed from {:?} to {:?}", source, destination)));
                //     }
                // },

                Command::Rename(from, to) => match fs::rename(from, &to).await {
                    Ok(_) => {
                        self.refresh();
                        self.select(Some(to));
                    }
                    Err(err) => println!("Error while renaming: {err}"),
                },
            };
        }
        ctx.request_repaint();
        // Ok(())
    }

    pub fn default_filename(mut self, filename: impl Into<String>) -> Self {
        self.filename_edit = filename.into();
        self
    }

    /// Resulting file path.
    pub fn path(&self) -> Option<PathBuf> {
        self.selected_file.clone()
    }

    fn refresh(&mut self) {
        //read_directory_boxed
        self.path_edit = String::from(self.path.to_str().unwrap_or_default());
        self.select(None);
    }

    fn select(&mut self, file: Option<PathBuf>) {
        self.filename_edit = match &file {
            Some(path) => get_file_name(path).to_string(),
            None => String::new(),
        };
        self.selected_file = file;
    }

    fn can_save(&self) -> bool {
        self.selected_file.is_some() || !self.filename_edit.is_empty()
    }

    fn can_open(&self) -> bool {
        self.selected_file.is_some()
    }

    fn can_rename(&self) -> bool {
        if !self.filename_edit.is_empty() {
            if let Some(file) = &self.selected_file {
            return get_file_name(file) != self.filename_edit;
            }
        }

        false
    }
    
    
    // Set a function to filter shown files.
    // pub fn filter(mut self, filter: Filter) -> Self {
    //     self.filter = Some(filter);
    //     self
    // }
    
    // Returns true, if the file selection was confirmed.
    // pub fn selected(&self) -> bool {
        // self.state == State::Selected
    // }

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
