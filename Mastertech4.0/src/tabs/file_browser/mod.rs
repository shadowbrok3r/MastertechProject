use self::{
    command::Command,
    io::{format_path_metadata, MetaData},
};
use crate::app_state::MastertechContext;
use crossbeam::channel::{self, Receiver, Sender};
use directories::UserDirs;
use displays::ui_tools::controls::{
    self, BarKind, ButtonKind, IconBox, IconButton, Metrics, TextButton, TextField,
    Tokens,
};
use displays::ui_tools::icons::{self, svg::{self, Icon}};
use displays::ui_tools::toasts::{Toast, ToastKind, ToastOptions, Toasts};
use displays::ui_tools::tokens;
use eframe::egui::Ui;
use eframe::egui::*;
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

/// Key a directory listing is ordered by. Directories stay ahead of files in
/// every mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize)]
pub enum FileSort {
    #[default]
    Name,
    Type,
    Size,
    Modified,
}

impl FileSort {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Type => "Type",
            Self::Size => "Size",
            Self::Modified => "Modified",
        }
    }
}

/// Width of the rename field on the mode bar.
const RENAME_W: f32 = 200.0;
/// Width of the sort-key dropdown.
const SORT_W: f32 = 100.0;
/// Opacity of the error hue in the delete-confirmation fill.
const DELETE_WASH: f32 = 0.18;
/// Tallest the multi-selection list grows before it scrolls.
const SELECTION_LIST_H: f32 = 180.0;
/// Tallest the clipboard list grows before it scrolls.
const CLIPBOARD_LIST_H: f32 = 140.0;

/// The SVG icon for `path`: a folder, or the file type's icon.
fn path_icon(path: &PathBuf) -> Icon {
    if path.is_dir() {
        svg::FOLDER
    } else {
        Icon::from_glyph(icons::file_icon(get_file_name(path), false)).unwrap_or(svg::FILE)
    }
}

/// Tint for a path's icon: folders in the tertiary role, files in the icon colour.
fn path_tint(t: &Tokens, path: &std::path::Path) -> Color32 {
    if path.is_dir() {
        t.tertiary
    } else {
        t.icon
    }
}

/// A faint sidebar heading led by `icon`.
/// A path's icon and name on one row, with `suffix` after the name.
fn path_label(ui: &mut Ui, path: &PathBuf, suffix: &str) {
    let t = Tokens::get(ui.ctx());
    let size = Metrics::get(ui.ctx()).icon_sm;
    ui.horizontal(|ui| {
        ui.add(path_icon(path).image(size).tint(path_tint(&t, path)));
        ui.label(format!("{}{suffix}", get_file_name(path)));
    });
}

impl MastertechContext {
    pub fn file_browse(&mut self, ui: &mut Ui) {
        let bar_h = Metrics::get(ui.ctx()).bar_h;
        eframe::egui::Panel::top("file_browser_mode_panel")
            .exact_size(bar_h)
            .frame(Frame::NONE)
            .show(ui, |ui| {
                controls::bar(ui, BarKind::Tool, |ui| {
                    let modes = [
                        (FilesPanelMode::FileBrowser, svg::FOLDER_OPEN, "File Browser"),
                        (FilesPanelMode::MyTools, svg::TOOLBOX, "My Tools"),
                    ];
                    for (mode, icon, label) in modes {
                        if controls::pill(ui, icon, label, self.files_panel_mode == mode).clicked() {
                            self.files_panel_mode = mode;
                        }
                    }

                    if self.files_panel_mode == FilesPanelMode::FileBrowser
                        && !self.show_deferred_viewport.load(Ordering::Relaxed)
                    {
                        if let Ok(mut file_browser) = self.file_browser.lock() {
                            file_browser.rename_row(ui);
                        }
                    }
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

/// Quick-access sidebar entry resolved to an absolute path.
struct FolderShortcut {
    name: String,
    icon: Icon,
    path: PathBuf,
}

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
    /// Key the listing is ordered by.
    sort: FileSort,
    /// Reverse the active sort key.
    sort_desc: bool,
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
    /// Send computed (path, size) from a background sizing thread.
    #[serde(skip)]
    metadata_tx: Sender<(PathBuf, u64)>,
    /// Receive computed (path, size) from a background sizing thread.
    #[serde(skip)]
    metadata_rx: Receiver<(PathBuf, u64)>,

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

    /// Quick-access sidebar shortcuts resolved to absolute paths.
    #[serde(skip)]
    shortcuts: Vec<FolderShortcut>,
    /// Single selection the rename field was last prefilled for.
    #[serde(skip)]
    rename_prefill_for: Option<PathBuf>,
    /// Items awaiting delete confirmation in the selection panel.
    #[serde(skip)]
    pending_delete: Option<Vec<PathBuf>>,
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
            show_hidden: false,
            sort: FileSort::default(),
            sort_desc: false,
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
            toasts: Toasts::with_id(Id::unique("file_browser_toasts")).anchor(Align2::RIGHT_TOP, (5.0, 45.0)),
            shortcuts: Vec::new(),
            rename_prefill_for: None,
            pending_delete: None,
        }
    }

    pub fn show(&mut self, ui: &mut Ui, copied_items_tx: Sender<String>) {
        let mut total_size = 0;

        self.handle_keyboard_events(ui, copied_items_tx.clone());

        if self.first_refresh_contents {
            self.refresh_contents();
            self.build_shortcuts();
            self.first_refresh_contents = false;
        }

        let current_single = self.single_selection();
        if current_single != self.rename_prefill_for {
            if let Some(p) = &current_single {
                self.filename_edit = get_file_name(p).to_string();
            }
            self.rename_prefill_for = current_single;
        }

        if let Some(pending) = &self.pending_delete {
            let current: HashSet<PathBuf> = self.selected_items.borrow().iter().cloned().collect();
            let armed: HashSet<PathBuf> = pending.iter().cloned().collect();
            if current != armed {
                self.pending_delete = None;
            }
        }

        self.top_panel(ui);
        self.left_sidebar(ui);
        let show_right =
            !self.selected_items.borrow().is_empty() || !self.copied_items_src.is_empty();
        self.right_sidebar(ui, show_right, copied_items_tx.clone());
        if self.animated_progress {
            self.bottom_panel(ui);
        }
        self.central_panel(ui);

        while let Ok(progress) = self.progress_rx.try_recv() {
            self.progress += progress as f64;
        }

        while let Ok((path, size)) = self.metadata_rx.try_recv() {
            if path.is_dir() {
                self.folder_metadata
                    .borrow_mut()
                    .insert(path.clone(), MetaData { path_size: size });
            } else {
                self.file_metadata
                    .borrow_mut()
                    .insert(path.clone(), MetaData { path_size: size });
            }
            total_size += size;
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
        CentralPanel::default().show(ui, |ui| {
            ui.shrink_width_to_current();
            ui.shrink_height_to_current();
            ui.add_space(ui.spacing().item_spacing.y * 1.5);

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
                    controls::loading_row(ui, "Loading...");
                },
            },
                );
        }); // .response.context_menu(|ui| FileBrowser::filebrowser_ctx_menu(ui));
    }

    pub fn top_panel(&mut self, ui: &mut Ui) {
        let bar_h = Metrics::get(ui.ctx()).bar_h;
        let panel = eframe::egui::Panel::top("file_browser_top")
            .exact_size(bar_h)
            .frame(Frame::NONE);
        panel.show(ui, |ui| {
            controls::bar(ui, BarKind::Tool, |ui| {
                let away = self.path != env::current_dir().unwrap_or_default();
                if IconButton::new(svg::HOUSE, IconBox::Md)
                    .tip("Home")
                    .enabled(away)
                    .show(ui)
                    .clicked()
                {
                    let _ = self.command_tx.send(Some(Command::Home));
                }

                if IconButton::new(svg::ARROW_UP, IconBox::Md)
                    .tip("Parent Folder")
                    .enabled(self.path.parent().is_some())
                    .show(ui)
                    .clicked()
                {
                    let _ = self.command_tx.send(Some(Command::UpDirectory));
                }

                if controls::icon_button(ui, svg::ARROW_CLOCKWISE, IconBox::Md, false, "Refresh")
                    .clicked()
                {
                    let _ = self.command_tx.send(Some(Command::Refresh));
                }

                self.sort_controls(ui);

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if controls::icon_button(ui, svg::FOLDER_PLUS, IconBox::Md, false, "New Folder")
                        .clicked()
                    {
                        let _ = self.command_tx.send(Some(Command::CreateDirectory));
                    }

                    let width = ui.available_width();
                    let response = TextField::new(&mut self.path_edit)
                        .id_salt("path_edit")
                        .icon(svg::FOLDER_OPEN)
                        .mono(true)
                        .width(width)
                        .show(ui)
                        .on_hover_text(&self.path_edit);

                    if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        let path = PathBuf::from(&self.path_edit);
                        if path != self.path {
                            let _ = self.command_tx.send(Some(Command::OpenPath(path)));
                        }
                    }
                });
            });
        });
    }

    /// Sort key picker and direction toggle for the current listing.
    fn sort_controls(&mut self, ui: &mut Ui) {
        let options: Vec<(FileSort, &str)> =
            [FileSort::Name, FileSort::Type, FileSort::Size, FileSort::Modified]
                .into_iter()
                .map(|sort| (sort, sort.as_str()))
                .collect();
        let mut changed =
            controls::dropdown(ui, "file_browser_sort", &mut self.sort, &options, SORT_W).changed();

        let (arrow, tip) = if self.sort_desc {
            (svg::SORT_DESCENDING, "Descending")
        } else {
            (svg::SORT_ASCENDING, "Ascending")
        };
        if controls::icon_button(ui, arrow, IconBox::Md, false, tip).clicked() {
            self.sort_desc = !self.sort_desc;
            changed = true;
        }

        if changed {
            self.resort_contents();
        }
    }

    /// Re-orders the cached listings without re-reading the disk.
    fn resort_contents(&mut self) {
        for paths in self.dir_contents.borrow_mut().values_mut() {
            sort_paths(paths, self.sort, self.sort_desc);
        }
    }

    pub fn bottom_panel(&mut self, ui: &mut Ui) {
        eframe::egui::Panel::bottom("file_browser_bottom").show(ui, |ui| {
            if self.progress as u64 == self.source_dir_size && self.animated_progress {
                self.progress = 0.0;
                self.animated_progress = false;
                ui.ctx().request_repaint();
                match self.command_tx.try_send(Some(Command::Refresh)) {
                    Ok(_) => debug!("Refreshing.."),
                    Err(e) => debug!("{e}"),
                };
            }

            if self.animated_progress {
                controls::ProgressBar::new(self.progress as f32 / self.source_dir_size as f32)
                    .percentage(true)
                    .width(ui.available_size_before_wrap().x / 1.6)
                    .show(ui);
            }
        });
    }

    /// Rename field and button shown on the mode combobox row.
    pub fn rename_row(&mut self, ui: &mut Ui) {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let can_rename = self.can_rename();
            if TextButton::new("Rename")
                .icon(svg::PENCIL_SIMPLE)
                .enabled(can_rename)
                .show(ui)
                .clicked()
                && let Some(from) = self.single_selection()
            {
                let to = from.with_file_name(&self.filename_edit);
                let _ = self.command_tx.send(Some(Command::Rename(from, to)));
            }
            TextField::new(&mut self.filename_edit)
                .id_salt("rename_edit_top")
                .hint("rename…")
                .icon(svg::TEXT_AA)
                .width(RENAME_W)
                .show(ui);
        });
    }

    /// Quick-access and drive navigation sidebar.
    fn left_sidebar(&mut self, ui: &mut Ui) {
        let t = Tokens::get(ui.ctx());
        let m = Metrics::get(ui.ctx());
        let sidebar_frame = Frame::default()
            .fill(t.sunken)
            .inner_margin(Margin::same(m.gap.x as i8))
            .corner_radius(CornerRadius::same(t.radius_md))
            .stroke(Stroke::new(1.0, t.card_border));

        let mut navigate_to: Option<PathBuf> = None;

        eframe::egui::Panel::left("file_browser_sidebar")
            .frame(sidebar_frame)
            .resizable(true)
            .default_size(170.0)
            .min_size(140.0)
            .max_size(300.0)
            .show(ui, |ui| {
                ScrollArea::vertical()
                    .id_salt("file_browser_sidebar_scroll")
                    .show(ui, |ui| {
                        let entry_w = ui.available_width();

                        controls::section_label_icon(ui, svg::STAR, "Quick Access");
                        ui.add_space(m.gap.y / 2.0);
                        for shortcut in &self.shortcuts {
                            let selected = self.path == shortcut.path;
                            if controls::ListItem::new(&shortcut.name).icon(shortcut.icon).selected(selected).width(entry_w).show(ui)
                                .clicked()
                            {
                                navigate_to = Some(shortcut.path.clone());
                            }
                        }

                        if !self.drive_letters.is_empty() {
                            ui.add_space(m.gap.y * 2.0);
                            controls::section_label_icon(ui, svg::HARD_DRIVES, "Drives");
                            ui.add_space(m.gap.y / 2.0);
                            for drive in &self.drive_letters {
                                let selected = self.path == PathBuf::from(drive);
                                if controls::ListItem::new(drive).icon(svg::HARD_DRIVE).selected(selected).width(entry_w).show(ui)
                                    .clicked()
                                {
                                    navigate_to = Some(PathBuf::from(drive));
                                }
                            }
                        }

                        ui.add_space(m.gap.y * 2.0);
                        controls::hairline(ui);
                        controls::toggle(ui, &mut self.read_dirs_only, "Directories Only");
                    });
            });

        if let Some(path) = navigate_to {
            let _ = self.command_tx.send(Some(Command::OpenPath(path)));
        }
    }

    /// Selection and clipboard panel shown while items are selected or copied.
    fn right_sidebar(&mut self, ui: &mut Ui, is_open: bool, copied_items_tx: Sender<String>) {
        let t = Tokens::get(ui.ctx());
        let m = Metrics::get(ui.ctx());
        let sidebar_frame = Frame::default()
            .fill(t.sunken)
            .inner_margin(Margin::same(m.gap.x as i8))
            .corner_radius(CornerRadius::same(t.radius_md))
            .stroke(Stroke::new(1.0, t.card_border));

        let mut selection: Vec<PathBuf> = self.selected_items.borrow().iter().cloned().collect();
        selection.sort();

        let mut do_copy = false;
        let mut do_paste = false;
        let mut do_rename = false;
        let mut clear_clipboard = false;
        let mut request_meta: Vec<PathBuf> = Vec::new();
        let mut is_expanded = is_open;

        eframe::egui::Panel::right("file_browser_selection")
            .frame(sidebar_frame)
            .resizable(true)
            .default_size(260.0)
            .min_size(210.0)
            .max_size(440.0)
            .show_collapsible(ui, &mut is_expanded, |ui| {
                ScrollArea::vertical()
                    .id_salt("file_browser_selection_scroll")
                    .show(ui, |ui| {
                        if selection.len() == 1 {
                            let path = selection[0].clone();
                            let is_dir = path.is_dir();
                            let name = get_file_name(&path);
                            ui.horizontal(|ui| {
                                ui.add(path_icon(&path).image(m.icon_lg).tint(path_tint(&t, &path)));
                                ui.label(RichText::new(name).strong().color(t.text));
                            });
                            ui.label(
                                RichText::new(if is_dir { "Folder" } else { "File" })
                                    .italics()
                                    .color(t.text_dim),
                            );

                            let size = if is_dir {
                                self.folder_metadata.borrow().get(&path).map(|m| m.path_size)
                            } else {
                                self.file_metadata.borrow().get(&path).map(|m| m.path_size)
                            };
                            match size {
                                Some(sz) => {
                                    controls::icon_label(
                                        ui,
                                        svg::DATABASE,
                                        &format!("Size: {}", format_path_metadata(sz)),
                                    );
                                }
                                None => {
                                    controls::loading_row(ui, "Size: calculating…");
                                    request_meta.push(path.clone());
                                }
                            }

                            if let Ok(meta) = std::fs::metadata(&path) {
                                if let Ok(modified) = meta.modified() {
                                    let dt: chrono::DateTime<chrono::Local> = modified.into();
                                    controls::icon_label(
                                        ui,
                                        svg::CLOCK,
                                        &format!("Modified: {}", dt.format("%Y-%m-%d %H:%M")),
                                    );
                                }
                            }
                            ui.label(RichText::new(path.to_string_lossy()).small().weak());

                            ui.add_space(m.gap.y);
                            controls::hairline(ui);

                            let can_rename = self.can_rename();
                            ui.horizontal(|ui| {
                                let field_w = (ui.available_width()
                                    - IconBox::Md.side(&m)
                                    - ui.spacing().item_spacing.x)
                                    .max(80.0);
                                TextField::new(&mut self.filename_edit)
                                    .id_salt("rename_edit_panel")
                                    .hint("rename…")
                                    .width(field_w)
                                    .show(ui);
                                if IconButton::new(svg::PENCIL_SIMPLE, IconBox::Md)
                                    .tip("Rename")
                                    .enabled(can_rename)
                                    .show(ui)
                                    .clicked()
                                {
                                    do_rename = true;
                                }
                            });

                            ui.add_space(m.gap.y / 2.0);
                            ui.horizontal(|ui| {
                                if controls::icon_text_button(ui, svg::COPY, "Copy").clicked() {
                                    do_copy = true;
                                }
                                if TextButton::new("Delete")
                                    .icon(svg::TRASH)
                                    .kind(ButtonKind::Danger)
                                    .show(ui)
                                    .clicked()
                                {
                                    self.pending_delete = Some(selection.clone());
                                }
                            });
                        } else if selection.len() > 1 {
                            ui.horizontal(|ui| {
                                ui.add(svg::CHECKS.image(m.icon_md).tint(t.primary));
                                ui.label(
                                    RichText::new(format!("{} items selected", selection.len()))
                                        .strong()
                                        .color(t.text),
                                );
                            });
                            ui.add_space(m.gap.y / 2.0);
                            ScrollArea::vertical()
                                .id_salt("multi_selection_list")
                                .max_height(SELECTION_LIST_H)
                                .show(ui, |ui| {
                                    for p in &selection {
                                        path_label(ui, p, "");
                                    }
                                });
                            ui.add_space(m.gap.y / 2.0);
                            ui.horizontal(|ui| {
                                if controls::icon_text_button(ui, svg::COPY, "Copy").clicked() {
                                    do_copy = true;
                                }
                                if TextButton::new("Delete")
                                    .icon(svg::TRASH)
                                    .kind(ButtonKind::Danger)
                                    .show(ui)
                                    .clicked()
                                {
                                    self.pending_delete = Some(selection.clone());
                                }
                            });
                        }

                        if let Some(pending) = self.pending_delete.clone() {
                            ui.add_space(m.gap.y);
                            Frame::default()
                                .fill(tokens::composite(t.error.gamma_multiply(DELETE_WASH), t.sunken))
                                .stroke(Stroke::new(1.0, t.error))
                                .inner_margin(Margin::same(m.gap.y as i8))
                                .corner_radius(CornerRadius::same(t.radius_sm))
                                .show(ui, |ui| {
                                    ui.horizontal_top(|ui| {
                                        ui.add(svg::WARNING.image(m.icon_md).tint(t.error));
                                        ui.label(
                                            RichText::new(format!(
                                                "Delete {} item(s)? This cannot be undone.",
                                                pending.len()
                                            ))
                                            .color(t.error),
                                        );
                                    });
                                    ui.horizontal(|ui| {
                                        if TextButton::new("Confirm Delete")
                                            .icon(svg::TRASH)
                                            .kind(ButtonKind::Danger)
                                            .show(ui)
                                            .clicked()
                                        {
                                            let _ = self
                                                .command_tx
                                                .send(Some(Command::DeleteMany(pending.clone())));
                                            self.pending_delete = None;
                                        }
                                        if controls::icon_text_button(ui, svg::X, "Cancel").clicked() {
                                            self.pending_delete = None;
                                        }
                                    });
                                });
                        }

                        if !self.copied_items_src.is_empty() {
                            ui.add_space(m.gap.y);
                            controls::hairline(ui);
                            ui.horizontal(|ui| {
                                ui.add(svg::CLIPBOARD.image(m.icon_md).tint(t.secondary));
                                ui.label(
                                    RichText::new(format!(
                                        "Clipboard — {} item(s) to copy",
                                        self.copied_items_src.len()
                                    ))
                                    .strong()
                                    .color(t.secondary),
                                );
                            });
                            ui.add_space(m.gap.y / 2.0);
                            ScrollArea::vertical()
                                .id_salt("clipboard_list")
                                .max_height(CLIPBOARD_LIST_H)
                                .show(ui, |ui| {
                                    for p in &self.copied_items_src {
                                        let is_dir = p.is_dir();
                                        let size = if is_dir {
                                            self.folder_metadata.borrow().get(p).map(|m| m.path_size)
                                        } else {
                                            self.file_metadata.borrow().get(p).map(|m| m.path_size)
                                        };
                                        let size_str = size
                                            .map(|s| format!(" ({})", format_path_metadata(s)))
                                            .unwrap_or_default();
                                        path_label(ui, p, &size_str);
                                    }
                                });
                            ui.add_space(m.gap.y / 2.0);
                            ui.label(
                                RichText::new(format!("Destination: {}", self.path.to_string_lossy()))
                                    .small()
                                    .weak(),
                            );

                            let same_dir = self
                                .copied_items_src
                                .iter()
                                .any(|p| p.parent() == Some(self.path.as_path()));
                            ui.add_space(m.gap.y / 2.0);
                            ui.horizontal(|ui| {
                                if TextButton::new("Paste here")
                                    .icon(svg::CLIPBOARD_TEXT)
                                    .kind(ButtonKind::Primary)
                                    .enabled(!same_dir)
                                    .show(ui)
                                    .clicked()
                                {
                                    do_paste = true;
                                }
                                if TextButton::new("Clear")
                                    .icon(svg::X)
                                    .kind(ButtonKind::Ghost)
                                    .show(ui)
                                    .clicked()
                                {
                                    clear_clipboard = true;
                                }
                            });
                            if same_dir {
                                ui.horizontal(|ui| {
                                    ui.add(svg::WARNING.image(m.icon_sm).tint(t.warn));
                                    ui.label(
                                        RichText::new("Can't paste into the source folder.")
                                            .small()
                                            .color(t.warn),
                                    );
                                });
                            }
                        }
                    });
            });

        if do_rename {
            if let Some(from) = self.single_selection() {
                let to = from.with_file_name(&self.filename_edit);
                let _ = self.command_tx.send(Some(Command::Rename(from, to)));
            }
        }

        for p in request_meta {
            let _ = self.command_tx.send(Some(Command::ReadMetadata(p)));
        }

        if do_copy {
            self.copied_items_src = selection.clone();
            for p in &self.copied_items_src {
                let _ = self.command_tx.send(Some(Command::ReadMetadata(p.clone())));
            }
            let name = if self.copied_items_src.len() == 1 {
                get_file_name(&self.copied_items_src[0]).to_string()
            } else {
                format!("{} items", self.copied_items_src.len())
            };
            self.toasts.add(Toast {
                kind: ToastKind::Info,
                text: format!("Copied {name}").into(),
                options: ToastOptions::default()
                    .show_progress(true)
                    .duration_in_seconds(4.0),
                ..Default::default()
            });
        }

        if do_paste {
            self.animated_progress = true;
            self.copied_items_dest = self.path.clone();
            match self.command_tx.try_send(Some(Command::Copy(
                self.copied_items_src.clone(),
                self.copied_items_dest.clone(),
                self.progress_tx.clone(),
                copied_items_tx.clone(),
            ))) {
                Ok(_) => info!(
                    "Pasting {:?} to {:?}",
                    self.copied_items_src, self.copied_items_dest
                ),
                Err(e) => log::error!("{e}"),
            }
            self.toasts.add(Toast {
                kind: ToastKind::Info,
                text: format!("Pasting to {}", self.copied_items_dest.to_string_lossy()).into(),
                options: ToastOptions::default()
                    .show_progress(true)
                    .duration_in_seconds(4.0),
                ..Default::default()
            });
        }

        if clear_clipboard {
            self.copied_items_src.clear();
        }
    }

    /// The single selected path, or `None` when zero or multiple are selected.
    fn single_selection(&self) -> Option<PathBuf> {
        let selected = self.selected_items.borrow();
        if selected.len() == 1 {
            selected.iter().next().cloned()
        } else {
            None
        }
    }

    /// Populates quick-access shortcuts from the current user's known folders.
    fn build_shortcuts(&mut self) {
        self.shortcuts.clear();
        let Some(dirs) = UserDirs::new() else {
            return;
        };
        let entries: [(&str, &'static str, Option<&std::path::Path>); 7] = [
            ("Home", svg::HOUSE.glyph(), Some(dirs.home_dir())),
            ("Desktop", icons::folder_shortcut_icon("Desktop"), dirs.desktop_dir()),
            ("Documents", icons::folder_shortcut_icon("Documents"), dirs.document_dir()),
            ("Downloads", icons::folder_shortcut_icon("Downloads"), dirs.download_dir()),
            ("Pictures", icons::folder_shortcut_icon("Pictures"), dirs.picture_dir()),
            ("Music", icons::folder_shortcut_icon("Music"), dirs.audio_dir()),
            ("Videos", icons::folder_shortcut_icon("Videos"), dirs.video_dir()),
        ];
        for (name, icon, path) in entries {
            if let Some(p) = path {
                if p.exists() {
                    self.shortcuts.push(FolderShortcut {
                        name: name.to_string(),
                        icon: Icon::from_glyph(icon).unwrap_or(svg::FOLDER),
                        path: p.to_path_buf(),
                    });
                }
            }
        }
    }

    /// One tree row for `path`; a directory row lists its contents while open.
    fn display_path(&self, ui: &mut Ui, path: &PathBuf) {
        let t = Tokens::get(ui.ctx());
        let label = get_file_name(path).to_owned();
        let modifiers = ui.input(|i| i.modifiers);
        let is_selected = self.selected_items.borrow().contains(path);
        let row = controls::ListItem::new(&label)
            .icon(path_icon(path))
            .icon_tone(path_tint(&t, path))
            .selected(is_selected);
        if path.is_dir() {
            let contents = match self.dir_contents.borrow().get(path) {
                Some(contents) => contents.clone(),
                None => {
                    let command = Command::ReadDirectory(path.clone());
                    if let Err(e) = self.command_tx.send(Some(command)) {
                        error!("Error: {e:?}");
                    }
                    vec![]
                }
            };
            let size = self
                .folder_metadata
                .borrow()
                .get(path)
                .map(|metadata| format_path_metadata(metadata.path_size))
                .unwrap_or_default();
            let id_salt = path.as_path().to_string_lossy().into_owned();
            let (response, _) = row.detail(&size).show_node(ui, id_salt, false, |ui| {
                for sub_path in &contents {
                    self.display_path(ui, sub_path);
                }
            });
            if response.secondary_clicked()
                && !self.folder_metadata.borrow().contains_key(path)
                && let Err(e) = self.command_tx.send(Some(Command::ReadMetadata(path.clone())))
            {
                log::error!("hovered sender error: {e:?}");
            }
            if response.clicked() {
                info!("label:path {:?} // {:?}", label, path);
                if self.selected_items.borrow().contains(path) {
                    self.selected_items.borrow_mut().remove(path);
                }
                if modifiers.ctrl {
                    self.selected_items.borrow_mut().insert(path.clone());
                } else {
                    self.selected_items.borrow_mut().clear();
                    self.selected_items.borrow_mut().insert(path.clone());
                }
            }
            if response.double_clicked()
                && let Err(e) = self.command_tx.send(Some(Command::OpenPath(path.clone())))
            {
                error!("Error: {e:?}");
            }
        } else if !self.read_dirs_only {
            let size = self
                .file_metadata
                .borrow()
                .get(path)
                .map(|metadata| format_path_metadata(metadata.path_size))
                .unwrap_or_default();
            let response = row.detail(&size).reserve_caret(true).show(ui);
            if response.clicked() {
                if let Err(e) = self.command_tx.send(Some(Command::Select(path.clone()))) {
                    error!("Error: {e:?}");
                }
                if modifiers.ctrl {
                    self.selected_items.borrow_mut().insert(path.clone());
                }
                if self.selected_items.borrow().contains(path) {
                    self.selected_items.borrow_mut().remove(path);
                } else {
                    self.selected_items.borrow_mut().clear();
                    self.selected_items.borrow_mut().insert(path.clone());
                }
            }
        }
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
        let mut new_contents = read_folder(&self.path, self.depth, self.read_dirs_only);
        sort_paths(&mut new_contents, self.sort, self.sort_desc);
        self.dir_contents
            .borrow_mut()
            .insert(self.path.clone(), new_contents);
        self.path_edit = self.path.to_string_lossy().to_string();
        self.selected_items.borrow_mut().clear();
        self.selected_item = None;
        self.get_drives();
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
        if self.filename_edit.is_empty() {
            return false;
        }
        if let Some(file) = self.single_selection() {
            return get_file_name(&file) != self.filename_edit;
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
        self.drive_letters.clear();
        let mut disks = Disks::new_with_refreshed_list();

        for disk in &mut disks {
            let mount = disk.mount_point().to_str().unwrap_or("").to_string();
            if !mount.is_empty() && !self.drive_letters.contains(&mount) {
                self.drive_letters.push(mount);
            }
        }
        self.drive_letters.sort();
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

/// Orders a directory listing in place, directories ahead of files.
pub fn sort_paths(paths: &mut Vec<PathBuf>, sort: FileSort, descending: bool) {
    // Decorate-sort-undecorate: the Size and Modified keys stat each entry, so
    // read them once instead of inside the comparator.
    let mut keyed: Vec<(SortKey, PathBuf)> = paths
        .drain(..)
        .map(|path| (SortKey::of(&path, sort), path))
        .collect();

    keyed.sort_by(|(a, _), (b, _)| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| {
                let by_key = a.cmp_value(b, sort);
                if descending { by_key.reverse() } else { by_key }
            })
            .then_with(|| a.name.cmp(&b.name))
    });

    paths.extend(keyed.into_iter().map(|(_, path)| path));
}

fn natural_name(path: &PathBuf) -> String {
    get_file_name(path).to_lowercase()
}

/// Precomputed ordering key for one entry.
struct SortKey {
    is_dir: bool,
    name: String,
    extension: String,
    size: u64,
    modified: std::time::SystemTime,
}

impl SortKey {
    fn of(path: &PathBuf, sort: FileSort) -> Self {
        let is_dir = path.is_dir();
        let (size, modified) = match sort {
            FileSort::Size | FileSort::Modified => std::fs::metadata(path)
                .map(|m| (m.len(), m.modified().unwrap_or(std::time::UNIX_EPOCH)))
                .unwrap_or((0, std::time::UNIX_EPOCH)),
            _ => (0, std::time::UNIX_EPOCH),
        };
        let name = natural_name(path);
        // Directories and extensionless files share the empty extension, which
        // groups them together ahead of every typed file.
        let extension = if is_dir {
            String::new()
        } else {
            path.extension()
                .and_then(|e| e.to_str())
                .unwrap_or_default()
                .to_lowercase()
        };
        Self { is_dir, name, extension, size, modified }
    }

    fn cmp_value(&self, other: &Self, sort: FileSort) -> std::cmp::Ordering {
        match sort {
            FileSort::Name => self.name.cmp(&other.name),
            FileSort::Type => self.extension.cmp(&other.extension),
            FileSort::Size => self.size.cmp(&other.size),
            FileSort::Modified => self.modified.cmp(&other.modified),
        }
    }
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

#[cfg(test)]
mod sort_tests {
    use super::{sort_paths, FileSort};
    use std::path::PathBuf;

    fn names(paths: &[PathBuf]) -> Vec<String> {
        paths
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect()
    }

    fn listing() -> Vec<PathBuf> {
        ["notes.txt", "b.rs", "archive.zip", "a.rs", "readme"]
            .iter()
            .map(PathBuf::from)
            .collect()
    }

    #[test]
    fn sorting_by_type_groups_extensions_then_names() {
        let mut paths = listing();
        sort_paths(&mut paths, FileSort::Type, false);
        assert_eq!(names(&paths), ["readme", "a.rs", "b.rs", "notes.txt", "archive.zip"]);
    }

    #[test]
    fn sorting_by_name_ignores_the_extension() {
        let mut paths = listing();
        sort_paths(&mut paths, FileSort::Name, false);
        assert_eq!(names(&paths), ["a.rs", "archive.zip", "b.rs", "notes.txt", "readme"]);
    }

    #[test]
    fn descending_reverses_the_key_but_keeps_the_name_tiebreak() {
        let mut paths = listing();
        sort_paths(&mut paths, FileSort::Type, true);
        assert_eq!(names(&paths), ["archive.zip", "notes.txt", "a.rs", "b.rs", "readme"]);
    }
}
