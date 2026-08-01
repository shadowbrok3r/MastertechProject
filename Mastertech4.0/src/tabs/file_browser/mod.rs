use self::{
    command::Command,
    io::{format_path_metadata, MetaData},
};
use crate::app_state::MastertechContext;
use crossbeam::channel::{self, Receiver, Sender};
use directories::UserDirs;
use displays::ui_tools::icons;
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
            .exact_size(30.0)
            .show(ui, |ui| {
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
    icon: &'static str,
    path: PathBuf,
}

/// Full-width, left-aligned selectable row for the sidebar panels.
fn sidebar_row(
    ui: &mut Ui,
    width: f32,
    height: f32,
    selected: bool,
    label: impl Into<String>,
) -> Response {
    let label = label.into();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    let visuals = ui.global_style().interact_selectable(&response, selected);
    if selected || response.hovered() {
        ui.painter()
            .rect_filled(rect, visuals.corner_radius, visuals.weak_bg_fill);
    }
    let text_pos = rect.left_center() + Vec2::new(8.0, 0.0);
    ui.painter().text(
        text_pos,
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(13.0),
        visuals.text_color(),
    );
    response
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
            toasts: Toasts::new().anchor(Align2::RIGHT_TOP, (5.0, 45.0)),
            shortcuts: Vec::new(),
            rename_prefill_for: None,
            pending_delete: None,
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
                    ui.label("Loading...");
                },
            },
                );
        }); // .response.context_menu(|ui| FileBrowser::filebrowser_ctx_menu(ui));
    }

    pub fn top_panel(&mut self, ui: &mut Ui) {
        eframe::egui::Panel::top("file_browser_top").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add_enabled_ui(self.path != env::current_dir().unwrap_or_default(), |ui| {
                    if ui.button(icons::HOME).on_hover_text("Home").clicked() {
                        let _ = self.command_tx.send(Some(Command::Home));
                    }
                });

                ui.add_enabled_ui(self.path.parent().is_some(), |ui| {
                    if ui.button(icons::UP).on_hover_text("Parent Folder").clicked() {
                        let _ = self.command_tx.send(Some(Command::UpDirectory));
                    }
                });

                if ui.button(icons::REFRESH).on_hover_text("Refresh").clicked() {
                    let _ = self.command_tx.send(Some(Command::Refresh));
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .button(icons::FOLDER_PLUS)
                        .on_hover_text("New Folder")
                        .clicked()
                    {
                        let _ = self.command_tx.send(Some(Command::CreateDirectory));
                    }

                    let response = ui
                        .add_sized(
                            Vec2::new(ui.available_width(), 20.0),
                            TextEdit::singleline(&mut self.path_edit)
                                .id(Id::new("path_edit"))
                                .cursor_at_end(true),
                        )
                        .on_hover_text(&self.path_edit);

                    if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        let path = PathBuf::from(&self.path_edit);
                        if path != self.path {
                            let _ = self.command_tx.send(Some(Command::OpenPath(path)));
                        }
                    }
                });
            });
            ui.add_space(ui.spacing().item_spacing.y);
        });
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
                ProgressBar::new(self.progress as f32 / self.source_dir_size as f32)
                    .show_percentage()
                    .desired_width(ui.available_size_before_wrap().x / 1.6)
                    .fill(Color32::from_rgb(255, 77, 210))
                    .animate(self.animated_progress)
                    .ui(ui);
            }
        });
    }

    /// Rename field and button shown on the mode combobox row.
    pub fn rename_row(&mut self, ui: &mut Ui) {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let can_rename = self.can_rename();
            ui.add_enabled_ui(can_rename, |ui| {
                if ui.button(format!("{} Rename", icons::EDIT)).clicked() {
                    if let Some(from) = self.single_selection() {
                        let to = from.with_file_name(&self.filename_edit);
                        let _ = self.command_tx.send(Some(Command::Rename(from, to)));
                    }
                }
            });
            ui.add(
                TextEdit::singleline(&mut self.filename_edit)
                    .id(Id::new("rename_edit_top"))
                    .desired_width(200.0)
                    .hint_text("rename…"),
            );
        });
    }

    /// Quick-access and drive navigation sidebar.
    fn left_sidebar(&mut self, ui: &mut Ui) {
        let sidebar_frame = Frame::default()
            .fill(Color32::from_rgb(20, 20, 24))
            .inner_margin(Margin::same(8))
            .corner_radius(CornerRadius::same(6))
            .stroke(Stroke::new(1.0, Color32::from_rgb(60, 60, 70)));

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
                        const ENTRY_H: f32 = 24.0;
                        let entry_w = ui.available_width();

                        ui.label(
                            RichText::new(format!("{} Quick Access", icons::STAR))
                                .strong()
                                .color(Color32::LIGHT_GRAY),
                        );
                        ui.add_space(4.0);
                        for shortcut in &self.shortcuts {
                            let selected = self.path == shortcut.path;
                            let label = format!("{} {}", shortcut.icon, shortcut.name);
                            if sidebar_row(ui, entry_w, ENTRY_H, selected, label).clicked() {
                                navigate_to = Some(shortcut.path.clone());
                            }
                        }

                        if !self.drive_letters.is_empty() {
                            ui.add_space(12.0);
                            ui.label(
                                RichText::new("Drives")
                                    .strong()
                                    .color(Color32::LIGHT_GRAY),
                            );
                            ui.add_space(4.0);
                            for drive in &self.drive_letters {
                                let selected = self.path == PathBuf::from(drive);
                                let label = format!("{} {drive}", icons::HARD_DRIVE);
                                if sidebar_row(ui, entry_w, ENTRY_H, selected, label).clicked() {
                                    navigate_to = Some(PathBuf::from(drive));
                                }
                            }
                        }

                        ui.add_space(12.0);
                        ui.separator();
                        ui.checkbox(&mut self.read_dirs_only, "Directories Only");
                    });
            });

        if let Some(path) = navigate_to {
            let _ = self.command_tx.send(Some(Command::OpenPath(path)));
        }
    }

    /// Selection and clipboard panel shown while items are selected or copied.
    fn right_sidebar(&mut self, ui: &mut Ui, is_open: bool, copied_items_tx: Sender<String>) {
        let sidebar_frame = Frame::default()
            .fill(Color32::from_rgb(24, 20, 28))
            .inner_margin(Margin::same(8))
            .corner_radius(CornerRadius::same(6))
            .stroke(Stroke::new(1.0, Color32::from_rgb(70, 60, 80)));

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
                            let icon = if is_dir {
                                icons::FOLDER
                            } else {
                                icons::file_icon(name, false)
                            };
                            ui.label(
                                RichText::new(format!("{icon} {name}"))
                                    .strong()
                                    .color(Color32::WHITE),
                            );
                            ui.label(
                                RichText::new(if is_dir { "Folder" } else { "File" })
                                    .italics()
                                    .color(Color32::LIGHT_GRAY),
                            );

                            let size = if is_dir {
                                self.folder_metadata.borrow().get(&path).map(|m| m.path_size)
                            } else {
                                self.file_metadata.borrow().get(&path).map(|m| m.path_size)
                            };
                            match size {
                                Some(sz) => {
                                    ui.label(format!("Size: {}", format_path_metadata(sz)));
                                }
                                None => {
                                    ui.label(RichText::new("Size: calculating…").weak());
                                    request_meta.push(path.clone());
                                }
                            }

                            if let Ok(meta) = std::fs::metadata(&path) {
                                if let Ok(modified) = meta.modified() {
                                    let dt: chrono::DateTime<chrono::Local> = modified.into();
                                    ui.label(format!("Modified: {}", dt.format("%Y-%m-%d %H:%M")));
                                }
                            }
                            ui.label(RichText::new(path.to_string_lossy()).small().weak());

                            ui.add_space(6.0);
                            ui.separator();

                            let can_rename = self.can_rename();
                            ui.horizontal(|ui| {
                                let field_w = (ui.available_width() - 40.0).max(80.0);
                                ui.add(
                                    TextEdit::singleline(&mut self.filename_edit)
                                        .id(Id::new("rename_edit_panel"))
                                        .desired_width(field_w)
                                        .hint_text("rename…"),
                                );
                                ui.add_enabled_ui(can_rename, |ui| {
                                    if ui
                                        .button(icons::EDIT)
                                        .on_hover_text("Rename")
                                        .clicked()
                                    {
                                        do_rename = true;
                                    }
                                });
                            });

                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                if ui.button(format!("{} Copy", icons::COPY)).clicked() {
                                    do_copy = true;
                                }
                                if ui.button(format!("{} Delete", icons::TRASH)).clicked() {
                                    self.pending_delete = Some(selection.clone());
                                }
                            });
                        } else if selection.len() > 1 {
                            ui.label(
                                RichText::new(format!("{} items selected", selection.len()))
                                    .strong()
                                    .color(Color32::WHITE),
                            );
                            ui.add_space(4.0);
                            ScrollArea::vertical()
                                .id_salt("multi_selection_list")
                                .max_height(180.0)
                                .show(ui, |ui| {
                                    for p in &selection {
                                        let icon = if p.is_dir() {
                                            icons::FOLDER
                                        } else {
                                            icons::file_icon(get_file_name(p), false)
                                        };
                                        ui.label(format!("{} {}", icon, get_file_name(p)));
                                    }
                                });
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                if ui.button(format!("{} Copy", icons::COPY)).clicked() {
                                    do_copy = true;
                                }
                                if ui.button(format!("{} Delete", icons::TRASH)).clicked() {
                                    self.pending_delete = Some(selection.clone());
                                }
                            });
                        }

                        if let Some(pending) = self.pending_delete.clone() {
                            ui.add_space(6.0);
                            Frame::default()
                                .fill(Color32::from_rgb(60, 20, 20))
                                .inner_margin(Margin::same(6))
                                .corner_radius(CornerRadius::same(4))
                                .show(ui, |ui| {
                                    ui.label(
                                        RichText::new(format!(
                                            "Delete {} item(s)? This cannot be undone.",
                                            pending.len()
                                        ))
                                        .color(Color32::from_rgb(255, 180, 180)),
                                    );
                                    ui.horizontal(|ui| {
                                        if ui
                                            .button(RichText::new("Confirm Delete").color(Color32::WHITE))
                                            .clicked()
                                        {
                                            let _ = self
                                                .command_tx
                                                .send(Some(Command::DeleteMany(pending.clone())));
                                            self.pending_delete = None;
                                        }
                                        if ui.button("Cancel").clicked() {
                                            self.pending_delete = None;
                                        }
                                    });
                                });
                        }

                        if !self.copied_items_src.is_empty() {
                            ui.add_space(8.0);
                            ui.separator();
                            ui.label(
                                RichText::new(format!(
                                    "{} Clipboard — {} item(s) to copy",
                                    icons::CLIPBOARD,
                                    self.copied_items_src.len()
                                ))
                                .strong()
                                .color(Color32::from_rgb(200, 180, 255)),
                            );
                            ui.add_space(2.0);
                            ScrollArea::vertical()
                                .id_salt("clipboard_list")
                                .max_height(140.0)
                                .show(ui, |ui| {
                                    for p in &self.copied_items_src {
                                        let is_dir = p.is_dir();
                                        let icon = if is_dir {
                                            icons::FOLDER
                                        } else {
                                            icons::file_icon(get_file_name(p), false)
                                        };
                                        let size = if is_dir {
                                            self.folder_metadata.borrow().get(p).map(|m| m.path_size)
                                        } else {
                                            self.file_metadata.borrow().get(p).map(|m| m.path_size)
                                        };
                                        let size_str = size
                                            .map(|s| format!(" ({})", format_path_metadata(s)))
                                            .unwrap_or_default();
                                        ui.label(format!("{} {}{}", icon, get_file_name(p), size_str));
                                    }
                                });
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(format!("Destination: {}", self.path.to_string_lossy()))
                                    .small()
                                    .weak(),
                            );

                            let same_dir = self
                                .copied_items_src
                                .iter()
                                .any(|p| p.parent() == Some(self.path.as_path()));
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                ui.add_enabled_ui(!same_dir, |ui| {
                                    if ui.button(format!("{} Paste here", icons::CLIPBOARD)).clicked() {
                                        do_paste = true;
                                    }
                                });
                                if ui.button("Clear").clicked() {
                                    clear_clipboard = true;
                                }
                            });
                            if same_dir {
                                ui.label(
                                    RichText::new("Can't paste into the source folder.")
                                        .small()
                                        .color(Color32::from_rgb(255, 170, 120)),
                                );
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
            ("Home", icons::HOME, Some(dirs.home_dir())),
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
                        icon,
                        path: p.to_path_buf(),
                    });
                }
            }
        }
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

        let icon = if path.is_dir() {
            icons::FOLDER
        } else {
            icons::file_icon(get_file_name(path), false)
        };
        let label = format!("{icon}   {}", get_file_name(path));
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
                                ui.add_space((ui.available_size_before_wrap().x - 100.0).max(0.0));
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
                    ui.add_space((ui.available_size_before_wrap().x - 100.0).max(0.0));
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
