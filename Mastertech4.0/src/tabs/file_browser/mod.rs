use self::{
    command::Command,
    io::{format_path_metadata, MetaData},
};
use crate::app_state::MastertechContext;
use crossbeam::channel::{self, Receiver, Sender};
use displays::ui_tools::toasts::{Toast, ToastKind, ToastOptions, Toasts};
use eframe::egui::Ui;
use eframe::egui::{collapsing_header::CollapsingState, text::LayoutJob, *};
use log::{debug, error, info};
use pollster::block_on;
use serde::Serialize;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    env,
    path::PathBuf,
    sync::{atomic::Ordering, Arc},
};
use sysinfo::Disks;
use walkdir::WalkDir;

pub mod command;
pub mod context_menu;
pub mod file_copy;
pub mod io;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FilesPanelMode {
    #[default]
    FileBrowser,
    MyTools,
}

impl MastertechContext {
    pub fn file_browse(&mut self, ui: &mut Ui) {
        eframe::egui::Panel::top("file_browser_mode_panel")
            .exact_size(28.0)
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    eframe::egui::ComboBox::from_id_salt("files_panel_mode")
                        .selected_text(match self.files_panel_mode {
                            FilesPanelMode::FileBrowser => "File Browser",
                            FilesPanelMode::MyTools => "My Tools",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.files_panel_mode,
                                FilesPanelMode::FileBrowser,
                                "File Browser",
                            );
                            ui.selectable_value(
                                &mut self.files_panel_mode,
                                FilesPanelMode::MyTools,
                                "My Tools",
                            );
                        });
                });
            });

        match self.files_panel_mode {
            FilesPanelMode::FileBrowser => {
                if !self.show_deferred_viewport.load(Ordering::Relaxed) {
                    let file_browser_clone = Arc::clone(&self.file_browser);
                    let mut file_browser = file_browser_clone.lock().unwrap();
                    file_browser.show(ui, self.copied_items_tx.clone());
                }
            }
            FilesPanelMode::MyTools => self.shared_ctx.filesystem.display(ui),
        }
    }
}

//use cached::proc_macro::{io_cached, cached};
const _KB_FROM_BYTES: u64 = 1024;
const _MB_FROM_BYTES: u64 = 1024 * 1024;
const _GB_FROM_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Serialize)]
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
    #[serde(skip)]
    file_metadata: RefCell<HashMap<PathBuf, MetaData>>,
    /// MetaData of each folder
    #[serde(skip)]
    folder_metadata: RefCell<HashMap<PathBuf, MetaData>>,
    /// Send size of file in bytes
    #[serde(skip)]
    metadata_tx: Sender<u64>,
    /// Send size of folder in bytes
    #[serde(skip)]
    metadata_rx: Receiver<u64>,

    /// Progress percentage
    progress: f64,
    /// Send progress
    #[serde(skip)]
    progress_tx: Sender<u64>,
    /// Retrieve progress
    #[serde(skip)]
    progress_rx: Receiver<u64>,
    #[serde(skip)]
    command_tx: Sender<Option<Command>>,
    #[serde(skip)]
    command_rx: Receiver<Option<Command>>,
    /// Animate the progress bar
    animated_progress: bool,
    /// When CTRL+C is hit, get the selected files to be copied
    copied_items_src: Vec<PathBuf>,
    /// When CTRL+V is hit, paste files in the current 'path_edit' directory
    copied_items_dest: PathBuf,

    drive_letters: Vec<String>,

    source_dir_size: u64,

    #[serde(skip)]
    toasts: Toasts,
}

impl FileBrowser {
    pub fn new() -> Self {
        let mut path = env::current_dir().unwrap_or_default();
        let mut filename_edit = String::new();

        let path_edit = path.to_str().unwrap_or_default().to_string();
        info!("filebrowser::new() {}", &path_edit);
        if path.is_file() {
            filename_edit = get_file_name(&path).to_string();
            path.pop();
        }
        let (progress_tx, progress_rx) = channel::unbounded();
        let (metadata_tx, metadata_rx) = channel::unbounded();
        let (command_tx, command_rx) = channel::unbounded();

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
            progress: 0.0,
            metadata_tx,
            metadata_rx,
            progress_tx,
            progress_rx,
            command_tx,
            command_rx,

            animated_progress: false,
            copied_items_src: Vec::new(),
            copied_items_dest: PathBuf::new(),
            drive_letters: Vec::new(),
            source_dir_size: 0,
            toasts: Toasts::new().anchor(Align2::RIGHT_TOP, (5.0, 5.0)),
        }
    }

    pub fn show(&mut self, ui: &mut Ui, copied_items_tx: Sender<String>) {
        let mut total_size = 0;

        ui.style_mut().visuals.selection.stroke.color = Color32::BLACK;
        ui.style_mut().visuals.selection.bg_fill = Color32::from_rgb(120, 10, 120);
        ui.style_mut().visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, Color32::WHITE);
        ui.style_mut().visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(20, 20, 25);
        ui.style_mut().visuals.widgets.inactive.bg_stroke =
            Stroke::new(1.0, Color32::from_rgb(80, 80, 80));
        ui.style_mut().visuals.widgets.open.bg_fill = Color32::from_black_alpha(50);
        ui.style_mut().visuals.widgets.open.weak_bg_fill = Color32::from_black_alpha(50);
        ui.style_mut().visuals.widgets.active.weak_bg_fill = Color32::from_rgb(30, 30, 30);
        ui.style_mut().visuals.widgets.hovered.weak_bg_fill = Color32::TRANSPARENT;
        ui.style_mut().visuals.widgets.hovered.bg_fill = Color32::from_rgb(12, 12, 12);
        ui.style_mut().visuals.widgets.hovered.bg_stroke =
            Stroke::new(1.0, Color32::from_rgb(200, 20, 200));

        self.handle_keyboard_events(ui, copied_items_tx.clone());
        self.top_panel(ui);
        self.bottom_panel(ui);
        self.central_panel(ui);

        while let Ok(progress) = self.progress_rx.try_recv() {
            self.progress += progress as f64;
        }

        while let Ok(meta) = self.metadata_rx.try_recv() {
            total_size += meta;
            self.source_dir_size = total_size;
        }

        self.toasts.show(ui.ctx());
        if let Ok(Some(cmd)) = self.command_rx.try_recv() {
            block_on(async {
                self.run_command(cmd).await;
            });
        }
    }

    pub fn central_panel(&mut self, ui: &mut Ui) {
        CentralPanel::default().show_inside(ui, |ui| {
            ui.shrink_width_to_current();
            ui.shrink_height_to_current();
            ui.add_space(ui.spacing().item_spacing.y * 1.5);

            if self.first_refresh_contents {
                self.refresh_contents();
                self.get_drives();
                self.first_refresh_contents = false;
            }

            ScrollArea::new([false, true])
                .id_salt("file_browser_scroll")
                .max_width(f32::INFINITY)
                .auto_shrink([false, false])
                .show_rows(
                    ui,
                    ui.text_style_height(&TextStyle::Body),
                    self.dir_contents
                        .borrow()
                        .get(&self.path)
                        .map_or(0, |files| files.len()),
                    |ui, range| match self.dir_contents.borrow().get(&self.path) //borrow().get(&self.path) 
            {
                Some(files) => {
                    ui.with_layout(ui.layout().with_main_justify(true), |ui| {
                        ui.vertical(|ui| {
                            for path in files[range].iter(){
                                self.display_path(ui, path);
                            }
                        });
                    });
                }
                None => {
                    // There was an error fetching the directory contents
                    // Send a command to fetch them in the background
                    let command = Command::ReadDirectory(self.path.clone());
                    self.command_tx.send(Some(command)).unwrap();
                    ui.label("Loading...");
                },
            },
                );
        }); // .response.context_menu(|ui| FileBrowser::filebrowser_ctx_menu(ui));
    }

    pub fn top_panel(&mut self, ui: &mut Ui) {
        eframe::egui::Panel::top("file_browser_top").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                let response = ui
                    .add_sized(
                        ui.available_size_before_wrap(),
                        TextEdit::singleline(&mut self.path_edit)
                            .id(Id::new("path_edit"))
                            .cursor_at_end(true),
                    )
                    .on_hover_text(&self.path_edit);

                if response.lost_focus() {
                    let path = PathBuf::from(&self.path_edit);
                    info!("Lost focus on self.path_edit");

                    match self.command_tx.send(Some(Command::OpenPath(path))) {
                        Ok(_) => info!("sent task successfully"),
                        Err(e) => log::error!("{e}"),
                    };
                }
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.add_enabled_ui(self.path != env::current_dir().unwrap_or_default(), |ui| {
                    let response = ui.button("🏠").on_hover_text("Home");
                    if response.clicked() {
                        match self.command_tx.send(Some(Command::Home)) {
                            Ok(_) => info!("Home"),
                            Err(e) => log::error!("{e}"),
                        }
                    }
                });

                ui.add_enabled_ui(self.path.parent().is_some(), |ui| {
                    let response = ui.button("⬆").on_hover_text("Parent Folder"); //
                    if response.clicked() {
                        match self.command_tx.send(Some(Command::UpDirectory)) {
                            Ok(_) => info!("UpDirectory"),
                            Err(e) => log::error!("{e}"),
                        }
                    }
                });

                let response = ui.button("⟲").on_hover_text("Refresh");
                if response.clicked() {
                    match self.command_tx.send(Some(Command::Refresh)) {
                        Ok(_) => info!("sent task successfully"),
                        Err(e) => log::error!("{e}"),
                    }
                }

                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.checkbox(&mut self.read_dirs_only, RichText::new("Directories Only"));
                    // ui.checkbox(&mut self.show_hidden, "Show Hidden");
                });
            });
            ui.add_space(ui.spacing().item_spacing.y);
        });
    }

    pub fn bottom_panel(&mut self, ui: &mut Ui) {
        eframe::egui::Panel::bottom("file_browser_bottom").show_inside(ui, |ui| {
            if self.progress as u64 == self.source_dir_size && self.animated_progress {
                self.progress = 0.0;
                self.animated_progress = false;
                ui.ctx().request_repaint();
                match self.command_tx.try_send(Some(Command::Refresh)) {
                    Ok(_) => debug!("Refreshing.."),
                    Err(e) => debug!("{e}"),
                };
            }

            // let mut display_text = format!("Try selecting some files to copy.. ");
            // let mut color = Color32::LIGHT_BLUE;
            // if copy_shortcut{
            //     color = Color32::LIGHT_GREEN;
            //     display_text = "Copied files to clipboard.".to_string();
            // }else if paste{
            //     color = ui.style().visuals.error_fg_color;
            //     display_text = format!("File Copy In Progress: {:?} / {:?}", self.progress as u64 / MB_FROM_BYTES, self.source_dir_size / MB_FROM_BYTES as u64);
            // }
            // ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            //     ui.colored_label(color ,RichText::new(display_text));
            // });

            ui.horizontal(|ui| {
                ui.with_layout(Layout::left_to_right(Align::BOTTOM), |ui| {
                    ui.add_space(5.0);
                    self.drive_letters
                        .sort_unstable_by(|b, a| b.partial_cmp(a).unwrap());
                    for drive in self.drive_letters.iter() {
                        let button =
                            Button::new(RichText::new(format!("💾 {drive}")).small()).small();

                        if ui.add(button).clicked() {
                            let path = Some(Command::OpenPath(PathBuf::from(drive)));
                            info!("Drive: {:?} -- Path: {:?}", drive, &path);

                            match self.command_tx.send(path) {
                                Ok(_) => info!("Opening drive path"),
                                Err(e) => log::error!("{e}"),
                            }
                        };
                    }
                });
            });

            ui.horizontal(|ui| {
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    let result = ui.add(
                        TextEdit::singleline(&mut self.filename_edit)
                            .id(Id::new("file_name_edit"))
                            .desired_width(ui.available_width() / 3.0),
                    );

                    if result.lost_focus() && !self.filename_edit.is_empty() {
                        let _ = self.path.join(&self.filename_edit);
                    }

                    if self.rename {
                        ui.add_enabled_ui(self.can_rename(), |ui| {
                            if ui.button(RichText::new("Rename").small()).clicked() {
                                if let Some(from) = self.selected_item.clone() {
                                    let to = from.with_file_name(&self.filename_edit);
                                    match self.command_tx.send(Some(Command::Rename(from, to))) {
                                        Ok(_) => {
                                            info!("ok");
                                        }
                                        Err(e) => {
                                            print!("{e}");
                                        }
                                    }
                                }
                            }
                        });
                    }

                    if self.new_folder
                        && ui.button(RichText::new("📁 New Folder").small()).clicked()
                    {
                        match self.command_tx.send(Some(Command::CreateDirectory)) {
                            Ok(_) => info!("ok"),
                            Err(e) => print!("{e}"),
                        }
                    }
                });
            });

            ProgressBar::new(self.progress as f32 / self.source_dir_size as f32)
                .show_percentage()
                .desired_width(ui.available_size_before_wrap().x / 1.6)
                .fill(Color32::from_rgb(255, 77, 210))
                .animate(self.animated_progress)
                .ui(ui);
        });
    }
    /**
        Handles displaying of subcontents of given directory by calling list_subfolders
        and makes only directories collapsible so we can see its subcontents
    */
    fn display_path(&self, ui: &mut Ui, path: &PathBuf) {
        let command_sender = self.command_tx.clone();
        let command_sender2 = self.command_tx.clone();
        let command_sender3 = self.command_tx.clone();
        // let command_sender4 = self.command_tx.clone();
        let command_sender5 = self.command_tx.clone();

        let label = match path.is_dir() {
            true => "🗀   ",
            false => "🗋   ",
        }
        .to_string()
            + get_file_name(path);
        let mut formatted_size = "".to_string();
        ui.horizontal_top(|ui| {
            if path.is_dir() {
                let id = ui.make_persistent_id(path.as_path().to_string_lossy());
                let modifiers = ui.input(|i| i.modifiers); // Get the current modifiers

                let contents = match self.dir_contents.borrow().get(path) {
                    Some(contents) => contents.clone(),
                    None => {
                        let command = Command::ReadDirectory(path.clone()); // Contents are not cached, fetch in the background
                        match command_sender.send(Some(command)) {
                            Ok(_) => drop(command_sender),
                            Err(e) => error!("Error: {e:?}"),
                        }
                        vec![] // Return an empty Vec for now
                    }
                };

                ui.vertical_centered_justified(|ui| {
                    CollapsingState::load_with_default_open(ui.ctx(), id, false)
                        .show_header(ui, |ui| {
                            let is_selected = self.selected_items.borrow().contains(path);
                            let selectable_label =
                                ui.selectable_label(is_selected, RichText::new(&label));
                            if selectable_label.clicked() {
                                info!("label:path {:?} // {:?}", &label, &path);
                            }
                            if selectable_label.secondary_clicked()
                                && !self.folder_metadata.borrow().contains_key(path)
                            {
                                match command_sender5
                                    .send(Some(Command::ReadMetadata(path.clone())))
                                {
                                    Ok(_) => drop(command_sender5),
                                    Err(e) => log::error!("hovered sender error: {e:?}"),
                                }
                            }
                            if let Some(metadata) = self.folder_metadata.borrow_mut().get(path) {
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
                                
                                let l = Arc::new(job);
                                let x = WidgetText::LayoutJob(l)
                                    .small()
                                    .background_color(ui.style().visuals.error_fg_color);
                                ui.add_space(ui.available_size_before_wrap().x - 100.0);
                                ui.add(Label::new(x));
                            }

                            if selectable_label.clicked() {
                                // If the item was already selected, deselect it
                                if self.selected_items.borrow().contains(path) {
                                    self.selected_items.borrow_mut().remove(path);
                                }
                                // If the control key is down and the item was not selected, select it
                                if modifiers.ctrl {
                                    self.selected_items.borrow_mut().insert(path.clone());
                                } else {
                                    // If the control key is not down, clear previous selection and select the current item
                                    self.selected_items.borrow_mut().clear();
                                    self.selected_items.borrow_mut().insert(path.clone());
                                }
                            }

                            if selectable_label.double_clicked() {
                                //|| selectable_label.ctx.input(|state| state.key_pressed(Key::Enter))
                                match command_sender2.send(Some(Command::OpenPath(path.clone()))) {
                                    Ok(_) => drop(command_sender2),
                                    Err(e) => error!("Error: {e:?}"),
                                }
                            }
                        })
                        .body(|ui| {
                            for sub_path in &contents {
                                self.display_path(ui, &sub_path);
                            }
                        });
                });
            } else if !path.is_dir() && !self.read_dirs_only {
                let is_selected = self.selected_items.borrow().contains(path);
                let modifiers = ui.input(|i| i.modifiers); // Get the current modifiers

                let selectable_label = ui.selectable_label(is_selected, RichText::new(&label));
                if selectable_label.clicked() {
                    match command_sender3.send(Some(Command::Select(path.clone()))) {
                        Ok(_) => drop(command_sender3),
                        Err(e) => error!("Error: {e:?}"),
                    }
                    // If the control key is down and the item was not selected, select it
                    if modifiers.ctrl {
                        self.selected_items.borrow_mut().insert(path.clone());
                    }
                    if self.selected_items.borrow().contains(path) {
                        // If the item was already selected, deselect it
                        self.selected_items.borrow_mut().remove(path);
                    } else {
                        // If the control key is not down, clear previous selection and select the current item
                        self.selected_items.borrow_mut().clear();
                        self.selected_items.borrow_mut().insert(path.clone());
                    }
                } else if selectable_label.secondary_clicked() {
                }

                if let Some(metadata) = self.file_metadata.borrow_mut().get(path) {
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

                    let x = WidgetText::LayoutJob(Arc::new(job))
                        .small()
                        .background_color(ui.style().visuals.error_fg_color);
                    ui.add_space(ui.available_size_before_wrap().x - 100.0);
                    ui.add(Label::new(x));
                }
            }
        });
    }

    fn _default_filename(mut self, filename: impl Into<String>) -> Self {
        self.filename_edit = filename.into();
        self
    }

    /**  Resulting file path. */
    fn _path(&self) -> Option<PathBuf> {
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
        let new_contents = read_folder(&self.path, self.depth, self.read_dirs_only);
        self.dir_contents
            .borrow_mut()
            .insert(self.path.clone(), new_contents);
        self.path_edit = self.path.to_string_lossy().to_string();
        self.get_drives();
        //self.select(None);
    }

    /**
        Updates the textedit which displays the
        currently selected file or folder
    */
    fn select(&mut self, file: PathBuf) {
        //fn select(&mut self, file: Option<PathBuf>) {

        self.filename_edit = match &file {
            path => get_file_name(path).to_string(),
            //None => String::new(),
        };
        self.selected_item = Some(file.as_path().to_path_buf());
        self.selected_items.borrow_mut().insert(file);
    }

    fn _deselect(&mut self, file: PathBuf) {
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

        for disk in &mut disks {
            self.drive_letters
                .push(disk.mount_point().to_str().unwrap_or("").to_string());
        }
    }

    fn handle_keyboard_events(&mut self, ui: &Ui, copied_items_tx: Sender<String>) {
        let cut = ui.input(|i| i.key_pressed(Key::Cut));
        let copy = ui.input_mut(|i| i.key_pressed(Key::C));
        let paste = ui.input_mut(|i| i.key_pressed(Key::V));
        let shift = ui.input_mut(|i| i.modifiers.shift);
        // let copy = ui.input(|i| i.events.iter().any(|ev| matches!(ev, Event::Copy)));
        // let paste = ui.input(|i| i.events.iter().any(|ev| matches!(ev, Event::Paste(_))));

        let selected_item_len = self.selected_items.borrow().len();

        if copy && shift && selected_item_len > 0 {
            self.copied_items_src = self.selected_items.borrow_mut().drain().collect();
            info!("Copied Items: {:?}", self.copied_items_src);

            let name = if self.copied_items_src.len() == 1 {
                if let Some(path) = self.copied_items_src[0].file_name() {
                    path.to_string_lossy().to_string()
                } else {
                    "No Source".to_string()
                }
            } else {
                format!("{} items", self.copied_items_src.len())
            };

            let copy_toast = Toast {
                kind: ToastKind::Info,
                text: format!("Copied {}", name).into(),
                options: ToastOptions::default()
                    .show_progress(true)
                    .duration_in_seconds(6.0),
                ..Default::default()
            };
            self.toasts.add(copy_toast);

            for path in &self.copied_items_src {
                match self
                    .command_tx
                    .clone()
                    .send(Some(Command::ReadMetadata(path.clone())))
                {
                    Ok(_) => info!("Getting file size"),
                    Err(e) => log::error!("hovered sender error: {e:?}"),
                }
            }
        } else if cut && shift && selected_item_len > 0 {
            self.copied_items_src = self.selected_items.borrow_mut().drain().collect();
            info!("Cut Items: {:?}", self.copied_items_src);

            for path in &self.copied_items_src {
                match self
                    .command_tx
                    .clone()
                    .send(Some(Command::ReadMetadata(path.clone())))
                {
                    Ok(_) => info!("Getting file size"),
                    Err(e) => log::error!("hovered sender error: {e:?}"),
                }
            }
        } else if paste && shift {
            self.animated_progress = true;
            if let Some(selected_path) = &self.selected_item {
                if selected_path.is_dir() {
                    self.copied_items_dest = PathBuf::from(selected_path);
                    match self.command_tx.try_send(Some(Command::Copy(
                        self.copied_items_src.clone(),
                        self.copied_items_dest.clone(),
                        self.progress_tx.clone(),
                        copied_items_tx.clone(),
                    ))) {
                        Ok(_) => info!(
                            "Source: {:?}\nDest: {:?}\n",
                            self.copied_items_src, self.copied_items_dest
                        ),
                        Err(e) => log::error!("{e}"),
                    }
                } else {
                    self.copied_items_dest = PathBuf::from(&self.path_edit);
                }

                let name = self.copied_items_dest.to_string_lossy().to_string();

                let paste_toast = Toast {
                    kind: ToastKind::Info,
                    text: format!("Pasting items to: {name}").into(),
                    options: ToastOptions::default()
                        .show_progress(true)
                        .duration_in_seconds(6.0),
                    ..Default::default()
                };
                self.toasts.add(paste_toast);
            }

            info!(
                "Pasted {:?}\nin directory: {:?}",
                self.copied_items_src, self.copied_items_dest
            );

            ui.ctx().request_repaint();
        }
    }
}

// #[cfg(windows)]
pub fn is_drive_root(path: &PathBuf) -> bool {
    path.to_str()
        .filter(|path| &path[1..] == ":\\")
        .and_then(|path| path.chars().next())
        .map_or(false, |ch| ch.is_ascii_uppercase())
}

pub fn get_file_name(path: &PathBuf) -> &str {
    // #[cfg(windows)]
    if path.is_dir() && is_drive_root(path) {
        return path.to_str().unwrap_or_default();
    }
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
}

/** Returns a Vec<PathBuf> of current directory contents and files. */
pub fn read_folder(path: &PathBuf, depth: usize, read_dirs_only: bool) -> Vec<PathBuf> {
    let result: Vec<_> = WalkDir::new(path)
        .min_depth(depth)
        .max_depth(depth)
        .into_iter()
        .filter_map(|e| e.ok()) // Only retreive the resulted items
        .filter(|entry| !read_dirs_only || entry.path().is_dir()) // Include only directories if read_dirs_only is true
        .map(|entry| entry.path().to_path_buf()) // iterate through each direntry
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
            // #[cfg(unix)]
            // if !show_hidden && get_file_name(path).starts_with('.') {
            //     return false;
            // }
            true
        })
        .collect();

    result
}
