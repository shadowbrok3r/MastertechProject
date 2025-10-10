use egui_data_table::{viewer::{default_hotkeys, DecodeErrorBehavior, RowCodec, UiActionContext}, DataTable, Renderer, RowViewer, UiAction};
use eframe::egui::{Button, CentralPanel, Id, KeyboardShortcut, RichText, ScrollArea, Spinner, TextEdit, TopBottomPanel, Ui, Vec2, Widget, scroll_area};
use database::schema::Process;
use egui_extras::Column;
use serde::Serialize;


// impl SharedContext {
//     pub fn process_table_viewer(&mut self, ui: &mut Ui) {
//         self.process_table_viewer.show(ui);
//     }
// }

/// Every logic is defined in `Viewer`
#[derive(Serialize)]
pub struct ProcessRowViewer {
    filter: String,
    row_protection: bool,
    #[serde(skip)]
    hotkeys: Vec<(KeyboardShortcut, UiAction)>,
    pub selected: Option<Process>,
    open_hotkeys: bool,
}

impl Default for ProcessRowViewer {
    fn default() -> Self {
        Self {
            filter: Default::default(),
            row_protection: Default::default(),
            hotkeys: Default::default(),
            selected: Default::default(),
            open_hotkeys: Default::default(),
        }
    }
}
pub struct ProcessTableViewer {
    // audit_selection: ,
    process_table: DataTable<Process>,
    pub process_viewer: ProcessRowViewer,
    loading: bool,
}

impl ProcessTableViewer {
    pub fn new() -> Self {
        Self {
            process_viewer: ProcessRowViewer::default(),
            loading: false,
            process_table: DataTable::new(),
        }
    }

    pub fn set_data(&mut self, data: Vec<Process>) {

        self.process_table.replace(data);
    }

    pub fn show(&mut self, ui: &mut Ui) {
        // SidePanel::right(Id::new("Process Viewer Side Panel"))
        //     .default_width(280.)
        //     .max_width(900.)
        //     .resizable(true)
        //     .show_separator_line(true)
        //     .show_inside(ui, |ui| 
        // {});

        TopBottomPanel::top("Process Viewer Top Panel")
            .exact_height(30.)
            .show_inside(ui, |ui| 
        {
            ui.horizontal_top(|ui| {
                TextEdit::singleline(&mut self.process_viewer.filter)
                    .hint_text(" Search for Process ")
                    .ui(ui);

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
        _row: &Process,
        column: usize,
        resp: &eframe::egui::Response,
    ) -> Option<Box<Process>> {
        match column {
            0 | 1 => {
                if resp.clicked() {

                }
            },
            _ => {}
        }
    
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