use egui_data_table::{viewer::{default_hotkeys, DecodeErrorBehavior, RowCodec, UiActionContext}, DataTable, Renderer, RowViewer, UiAction};
use eframe::egui::{Button, CentralPanel, ComboBox, Id, KeyboardShortcut, RichText, ScrollArea, Spinner, TextEdit, TopBottomPanel, Ui, Vec2, Widget, scroll_area};
use crossbeam::channel::{Receiver, Sender};
use database::schema::Process;
use egui_extras::Column;
use web_time::Instant;
use serde::Serialize;

/// Available refresh rate options in milliseconds
pub const REFRESH_RATE_OPTIONS: &[(u64, &str)] = &[
    (500, "500ms"),
    (1000, "1 second"),
    (2000, "2 seconds"),
    (5000, "5 seconds"),
    (10000, "10 seconds"),
];

/// Actions that can be performed on a process
#[derive(Debug, Clone)]
pub enum ProcessAction {
    /// Kill a process by PID
    Kill(u32),
    /// Open the process executable location in file explorer
    OpenInExplorer(String),
}

/// Every logic is defined in `Viewer`
#[derive(Serialize)]
pub struct ProcessRowViewer {
    filter: String,
    row_protection: bool,
    #[serde(skip)]
    hotkeys: Vec<(KeyboardShortcut, UiAction)>,
    pub selected: Option<Process>,
    open_hotkeys: bool,
    /// Channel to send process actions (Kill, Open in Explorer)
    #[serde(skip)]
    pub action_tx: Option<Sender<ProcessAction>>,
}

impl Default for ProcessRowViewer {
    fn default() -> Self {
        Self {
            filter: Default::default(),
            row_protection: Default::default(),
            hotkeys: Default::default(),
            selected: Default::default(),
            open_hotkeys: Default::default(),
            action_tx: None,
        }
    }
}
pub struct ProcessTableViewer {
    process_table: DataTable<Process>,
    pub process_viewer: ProcessRowViewer,
    loading: bool,
    /// Receiver for process actions from the context menu
    pub action_rx: Receiver<ProcessAction>,
    /// Sender stored for future use (e.g., if viewer is recreated)
    #[allow(dead_code)]
    action_tx: Sender<ProcessAction>,
    /// Refresh rate in milliseconds
    pub refresh_rate_ms: u64,
    /// Last time the process table was updated
    last_update: Instant,
    /// Index of the selected refresh rate option
    refresh_rate_idx: usize,
}

impl ProcessTableViewer {
    pub fn new() -> Self {
        let (action_tx, action_rx) = crossbeam::channel::unbounded();
        let mut process_viewer = ProcessRowViewer::default();
        process_viewer.action_tx = Some(action_tx.clone());
        
        Self {
            process_viewer,
            loading: false,
            process_table: DataTable::new(),
            action_rx,
            action_tx,
            refresh_rate_ms: 1000, // Default to 1 second
            last_update: Instant::now(),
            refresh_rate_idx: 1, // Index of "1 second" option
        }
    }

    /// Set process data, but only if enough time has passed since last update
    pub fn set_data(&mut self, data: Vec<Process>) {
        let elapsed = self.last_update.elapsed().as_millis() as u64;
        if elapsed >= self.refresh_rate_ms {
            self.process_table.replace(data);
            self.last_update = Instant::now();
        }
    }
    
    /// Force set data, ignoring the refresh rate
    pub fn force_set_data(&mut self, data: Vec<Process>) {
        self.process_table.replace(data);
        self.last_update = Instant::now();
    }
    
    /// Try to receive a process action from the context menu
    pub fn try_recv_action(&self) -> Option<ProcessAction> {
        self.action_rx.try_recv().ok()
    }

    pub fn show(&mut self, ui: &mut Ui) {
        TopBottomPanel::top("Process Viewer Top Panel")
            .exact_height(30.)
            .show_inside(ui, |ui| 
        {
            ui.horizontal_top(|ui| {
                TextEdit::singleline(&mut self.process_viewer.filter)
                    .hint_text(" Search for Process ")
                    .ui(ui);

                ui.add_space(10.);
                
                // Refresh rate dropdown
                ui.label("Refresh:");
                let current_label = REFRESH_RATE_OPTIONS.get(self.refresh_rate_idx)
                    .map(|(_, label)| *label)
                    .unwrap_or("1 second");
                
                ComboBox::from_id_salt("process_refresh_rate")
                    .selected_text(current_label)
                    .show_ui(ui, |ui| {
                        for (idx, (ms, label)) in REFRESH_RATE_OPTIONS.iter().enumerate() {
                            if ui.selectable_value(&mut self.refresh_rate_idx, idx, *label).clicked() {
                                self.refresh_rate_ms = *ms;
                            }
                        }
                    });
                
                ui.add_space(10.);
                
                let label = if self.process_viewer.open_hotkeys {
                    " Hide Hotkeys "
                } else {
                    " Show Hotkeys "
                };
                if Button::new(label).ui(ui).clicked() {
                    self.process_viewer.open_hotkeys = !self.process_viewer.open_hotkeys;
                }
            });
        });

        TopBottomPanel::bottom(Id::new("Task Audit Hot Keys"))
            .max_height(240.)
            .show_animated_inside(ui, self.process_viewer.open_hotkeys, |ui| 
        {
            ui.vertical_centered(|ui| ui.heading("Hotkeys"));
            ui.vertical_centered_justified(|ui| ui.separator());

            ui.horizontal_wrapped(|ui| {
                ui.style_mut().spacing.item_spacing.y = 5.0;
                ui.add_space(2.);
                let mut count = 0;
                for (k, a) in &self.process_viewer.hotkeys {
                    Button::new(format!("{a:?}"))
                        .min_size(Vec2::new(280., 25.))
                        .shortcut_text(
                            RichText::new(ui.ctx().format_shortcut(k))
                            .code()
                            .color(ui.style().visuals.warn_fg_color)
                        )
                        .ui(ui);
                    
                    count += 1;
                    if count % 4 == 0 {
                        ui.end_row();
                    }
                }
            });
        });

        CentralPanel::default()
            .show_inside(ui, |ui| 
        {
            ui.horizontal(|ui| {
                ui.add_space(10.);
            
                if self.loading {
                    ui.ctx().request_repaint();
                    ui.add_space(10.);
                    Spinner::new().color(ui.style().visuals.error_fg_color).ui(ui);
                }

            });
            ui.add_space(5.);
            ScrollArea::horizontal()
                .auto_shrink(false)
                .show(ui, |ui| 
                    Renderer::new(&mut self.process_table, &mut self.process_viewer)                    
                    .with_style_modify(|s| {
                        s.scroll_bar_visibility = scroll_area::ScrollBarVisibility::AlwaysVisible;
                        s.single_click_edit_mode = true;
                        s.auto_shrink = [false, false].into();
                    })
                    .ui(ui)
                );
            
        });  
    }
}


// Don't need to implement any trait on row data itself.
// #[derive(Default, Serialize, Clone, Deserialize, PartialEq, Debug)]
// pub struct ProcessTableData(pub String, pub String, pub String, pub String, pub String, pub String, pub String);

// pub struct Process 

impl RowViewer<Process> for ProcessRowViewer {
    fn try_create_codec(&mut self, _: bool) -> Option<impl RowCodec<Process>> {
        Some(Codec)
    }

    fn num_columns(&mut self) -> usize {
        6
    }

    fn column_name(&mut self, column: usize) -> std::borrow::Cow<'static, str> {
        ["PID", "Name", "CPU Usage", "Memory Usage", "Disk R/W", "CMD"][column].into()
    }

    fn is_sortable_column(&mut self, column: usize) -> bool {
        [true, true, true, true, true, true][column]
    }

    fn row_filter_hash(&mut self) -> &impl std::hash::Hash {
        &self.filter
    }

    fn filter_row(&mut self, row: &Process) -> bool {
        row.name.contains(&self.filter) 
        || row.id.to_string().contains(&self.filter)
        || row.user_id.clone().unwrap_or_default().contains(&self.filter)
    }

    fn hotkeys(&mut self, context: &UiActionContext) -> Vec<(KeyboardShortcut, UiAction)> {
        let hotkeys = default_hotkeys(context);
        self.hotkeys.clone_from(&hotkeys);
        hotkeys
    }

    // fn custom_context_menu_items(
    //     &mut self,
    //     _context: &UiActionContext,
    //     _selection: &egui_data_table::SelectionSnapshot<'_, Process>,
    // ) -> Vec<egui_data_table::CustomMenuItem> {
    //     vec![
    //         egui_data_table::CustomMenuItem::new("Kill Process", UiAction::KillProcess),
    //         egui_data_table::CustomMenuItem::new("Open Process in File Explorer", UiAction::OpenProcessInFileExplorer),
    //     ]
    // }

    // fn on_custom_action_ex(
    //     &mut self,
    //     action_id: &'static str,
    //     ctx: &egui_data_table::viewer::CustomActionContext<'_, Process>,
    //     editor: &mut egui_data_table::viewer::CustomActionEditor<Process>,
    // ) {
    //     match action_id {
    //         "Kill Process" => {
    //             ctx.row.kill_process();
    //         },
    //         "Open Process in File Explorer" => {
    //             ctx.row.open_process_in_file_explorer();
    //         },
    //     }
    //     editor.close();
    // }

    fn show_cell_view(&mut self, ui: &mut eframe::egui::Ui, row: &Process, column: usize) {
        let _ = match column {
            0 => ui.horizontal_centered(|ui| ui.colored_label(ui.style().visuals.warn_fg_color, format!(" {}", row.id.clone()))).inner,
            1 => ui.horizontal_centered(|ui| ui.label(format!(" {}", row.name.clone()))).inner,
            2 => ui.horizontal_centered(|ui| ui.label(format!(" {:.2}%", row.cpu_usage.clone()))).inner,
            3 => ui.horizontal_centered(|ui| ui.label(format!(" {}Mb", row.memory.clone()))).inner,
            4 => ui.horizontal_centered(|ui| ui.label(format!(" {}Mb / {}Mb", row.process_disk_usage.total_read_bytes.clone(), row.process_disk_usage.total_written_bytes.clone()))).inner,
            5 => ui.horizontal_centered(|ui| ui.label(format!(" {}", row.cmd.clone()))).inner,
            _ => unreachable!(),
        };
    }

    fn column_render_config(
        &mut self,
        column: usize,
        _is_last_visible_column: bool,
    ) -> Column {
        let col_config = Column::auto();
        match column {
            0 => col_config.resizable(true).at_least(60.).at_most(60.),
            1 => col_config.resizable(true).at_least(180.).at_most(225.),
            2 => col_config.resizable(true).at_least(90.).at_most(90.),
            3 => col_config.resizable(true).at_least(100.).at_most(100.),
            4 => col_config.resizable(true).at_least(150.).at_most(200.),
            5 => col_config.resizable(true).clip(false),
            _ => col_config,
        }
    }
    
    fn show_cell_editor(
        &mut self,
        _ui: &mut eframe::egui::Ui,
        _row: &mut Process,
        column: usize,
    ) -> Option<eframe::egui::Response> {
        match column {
            // 0 => Some(
            //     Hyperlink::from_label_and_url(
            //         format!(" {}", row.0.clone()), 
            //         format!("{BASE_URL}{}", row.0.clone())
            //     )
            //     .open_in_new_tab(true)
            //     .ui(ui)
            // ),
            _ => None,
        }
    }

    fn on_cell_view_response(
        &mut self,
        row: &Process,
        _column: usize,
        resp: &eframe::egui::Response,
    ) -> Option<Box<Process>> {
        // Store selected process when clicked
        if resp.clicked() {
            self.selected = Some(row.clone());
        }
        
        // Show context menu on right-click
        resp.context_menu(|ui| {
            ui.set_min_width(180.0);
            
            ui.label(RichText::new(format!("Process: {}", row.name)).strong());
            ui.label(RichText::new(format!("PID: {}", row.id)).small());
            ui.separator();
            
            if ui.button("🔫 Kill Process").clicked() {
                if let Some(tx) = &self.action_tx {
                    let _ = tx.try_send(ProcessAction::Kill(row.id));
                }
                ui.close();
            }
            
            if ui.button("📂 Open in Explorer").clicked() {
                // Use the exe_path if available, otherwise use cmd
                let path = row.exe_path.clone().unwrap_or_else(|| row.cmd.clone());
                if let Some(tx) = &self.action_tx {
                    let _ = tx.try_send(ProcessAction::OpenInExplorer(path));
                }
                ui.close();
            }
        });
    
        resp
            .clone()
            .on_hover_and_drag_cursor(eframe::egui::CursorIcon::Crosshair)
            .dnd_release_payload::<String>()
            .map(|_| Box::new(Process::default()))
    }

    fn set_cell_value(
        &mut self,
        src: &Process,
        dst: &mut Process,
        column: usize,
    ) {
        match column {
            0 => dst.id = src.id.clone(),
            1 => dst.name = src.name.clone(),
            2 => dst.cpu_usage = src.cpu_usage.clone(),
            3 => dst.memory = src.memory.clone(),
            4 => dst.process_disk_usage = src.process_disk_usage.clone(),
            5 => dst.cmd = src.cmd.clone(),
            _ => unreachable!(),
        }
    }

    fn compare_cell(
        &self,
        row_l: &Process,
        row_r: &Process,
        column: usize,
    ) -> std::cmp::Ordering {
        match column {
            0 => row_l.id.cmp(&row_r.id),
            1 => row_l.name.cmp(&row_r.name),
            2 => row_l.cpu_usage.to_string().cmp(&row_r.cpu_usage.to_string()),
            3 => row_l.memory.to_string().cmp(&row_r.memory.to_string()),
            4 => row_l.process_disk_usage.total_read_bytes.to_string().cmp(&row_r.process_disk_usage.total_read_bytes.to_string()),
            5 => row_l.cmd.cmp(&row_r.cmd),
            _ => row_l.id.cmp(&row_r.id)
        }
    }

    fn new_empty_row(&mut self) -> Process {
        // Instead of requiring `Default` trait for row data types, the viewer is
        // responsible of providing default creation method.
        Process::default()
    }
}


/* -------------------------------------------- Codec ------------------------------------------- */

struct Codec;

impl RowCodec<Process> for Codec {
    type DeserializeError = &'static str;

    fn encode_column(&mut self, src_row: &Process, column: usize, dst: &mut String) {
        match column {
            0 => dst.push_str(&src_row.id.to_string()),
            1 => dst.push_str(&src_row.name),
            2 => dst.push_str(&src_row.cpu_usage.to_string()),
            3 => dst.push_str(&src_row.memory.to_string()),
            4 => dst.push_str(&format!("{}/{}", src_row.process_disk_usage.read_bytes, src_row.process_disk_usage.written_bytes)),
            5 => dst.push_str(&src_row.cmd),
            _ => unreachable!(),
        }
    }

    fn decode_column(
        &mut self,
        src_data: &str,
        column: usize,
        dst_row: &mut Process,
    ) -> Result<(), DecodeErrorBehavior> {
        match column {
            0 => dst_row.id = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            1 => dst_row.name = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            2 => dst_row.cpu_usage = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            3 => dst_row.memory = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            // 4 => dst_row.process_disk_usage = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            5 => dst_row.cmd = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            _ => unreachable!(),
        }

        Ok(())
    }

    fn create_empty_decoded_row(&mut self) -> Process {
        Process::default()
    }
}