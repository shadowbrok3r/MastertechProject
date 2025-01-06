use eframe::egui::{Button, CentralPanel, Color32, Id, KeyboardShortcut, RichText, Spinner, TextEdit, TopBottomPanel, Ui, Vec2, Widget};
use chrono::{DateTime, NaiveDateTime, Utc};
use displays::egui_data_table::{viewer::{default_hotkeys, DecodeErrorBehavior, RowCodec, UiActionContext}, DataTable, Renderer, RowViewer, UiAction};
use serde::{Deserialize, Serialize};
use egui_extras::Column;
// impl SharedContext {
//     pub fn process_table_viewer(&mut self, ui: &mut Ui) {
//         self.process_table.show(ui);
//     }
// }

/// Every logic is defined in `Viewer`
#[derive(Serialize)]
pub struct ProcessRowViewer {
    filter: String,
    row_protection: bool,
    hotkeys: Vec<(KeyboardShortcut, UiAction)>,
    pub selected: Option<ProcessTableData>,
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
    process_table: DataTable<ProcessTableData>,
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

    fn show(&mut self, ui: &mut Ui) {
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
            
            Renderer::new(&mut self.process_table, &mut self.process_viewer).ui(ui);
        });  
    }
}


// Don't need to implement any trait on row data itself.
#[derive(Default, Serialize, Clone, Deserialize, PartialEq, Debug)]
pub struct ProcessTableData(pub String, pub String, pub String, pub String, pub String, pub String, pub String);

impl RowViewer<ProcessTableData> for ProcessRowViewer {
    fn try_create_codec(&mut self, _: bool) -> Option<impl RowCodec<ProcessTableData>> {
        Some(Codec)
    }

    fn num_columns(&mut self) -> usize {
        7
    }

    fn column_name(&mut self, column: usize) -> std::borrow::Cow<'static, str> {
        ["Order #", "Customer Name", "Date", "Status", "Sales Rep", "Split Rep", "Needs Call"][column].into()
    }

    fn is_sortable_column(&mut self, column: usize) -> bool {
        [true, true, true, true, true, true, true][column]
    }

    fn row_filter_hash(&mut self) -> &impl std::hash::Hash {
        &self.filter
    }

    fn filter_row(&mut self, row: &ProcessTableData) -> bool {
        row.0.contains(&self.filter) 
        || row.1.to_lowercase().contains(&self.filter)
        || row.4.to_lowercase().contains(&self.filter)
        || row.5.to_lowercase().contains(&self.filter)
    }

    fn hotkeys(&mut self, context: &UiActionContext) -> Vec<(KeyboardShortcut, UiAction)> {
        let hotkeys = default_hotkeys(context);
        self.hotkeys.clone_from(&hotkeys);
        hotkeys
    }

    fn show_cell_view(&mut self, ui: &mut eframe::egui::Ui, row: &ProcessTableData, column: usize) {
        let _ = match column {
            0 => ui.horizontal_centered(|ui| ui.colored_label(ui.style().visuals.warn_fg_color, format!(" {}", row.0.clone()))).inner,
            1 => ui.horizontal_centered(|ui| ui.label(format!(" {}", row.1.clone()))).inner,
            2 => ui.horizontal_centered(|ui| {
                // Parse the input into a NaiveDateTime
                let naive_datetime = NaiveDateTime::parse_from_str(&row.2, "%Y-%m-%d %H:%M:%S")
                    .expect("Failed to parse datetime");

                // Convert to a DateTime with Utc timezone
                let datetime: DateTime<Utc> = DateTime::from_naive_utc_and_offset(naive_datetime, Utc);

                // Format the DateTime into yyyy/mm/dd
                let formatted_date = datetime.format(" %m/%d/%Y").to_string();
                let split1 = formatted_date.split_once('/').unwrap_or_default();
                let split2 = split1.1.split_once('/').unwrap_or_default();
                ui.horizontal_centered(|ui| {
                    ui.colored_label(Color32::from_rgb(42, 195, 222), format!("{}/", split1.0));
                    ui.colored_label(ui.style().visuals.error_fg_color, format!("{}/", split2.0));
                    ui.colored_label(ui.style().visuals.warn_fg_color, split2.1)
                }).inner
            }).inner,
            3 => ui.horizontal_centered(|ui| ui.label(format!(" {}", row.3.clone()))).inner,
            4 => ui.horizontal_centered(|ui| ui.label(format!(" {}", row.4.clone()))).inner,
            5 => ui.horizontal_centered(|ui| ui.label(format!(" {}", row.5.clone()))).inner,
            6 => ui.vertical_centered(|ui| ui.checkbox(&mut false, "")).inner,
            _ => unreachable!(),
        };
    }

    fn column_render_config(&mut self, column: usize) -> Column {
        let col_config = Column::auto();
        match column {
            0 => col_config.resizable(true).at_least(60.).at_most(60.),
            1 => col_config.resizable(true).at_least(180.).at_most(225.),
            2 => col_config.resizable(true).at_least(90.).at_most(100.),
            3 => col_config.resizable(true).at_least(130.).at_most(130.),
            4 => col_config.resizable(true).at_least(130.).at_most(150.),
            5 => col_config.resizable(true).at_least(130.).at_most(150.),
            6 => col_config.resizable(true).at_least(80.).at_most(80.),
            _ => col_config,
        }
    }
    
    fn show_cell_editor(
        &mut self,
        ui: &mut eframe::egui::Ui,
        row: &mut ProcessTableData,
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
        row: &ProcessTableData,
        column: usize,
        resp: &eframe::egui::Response,
    ) -> Option<Box<ProcessTableData>> {
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
            .map(|_| Box::new(ProcessTableData::default()))
    }

    fn set_cell_value(
        &mut self,
        src: &ProcessTableData,
        dst: &mut ProcessTableData,
        column: usize,
    ) {
        match column {
            0 => dst.0 = src.0.clone(),
            1 => dst.1 = src.1.clone(),
            2 => dst.2 = src.2.clone(),
            3 => dst.3 = src.3.clone(),
            4 => dst.4 = src.4.clone(),
            5 => dst.5 = src.5.clone(),
            6 => dst.6 = src.6.clone(),
            _ => unreachable!(),
        }
    }

    fn compare_cell(
        &self,
        row_l: &ProcessTableData,
        row_r: &ProcessTableData,
        column: usize,
    ) -> std::cmp::Ordering {
        match column {
            0 => row_l.0.cmp(&row_r.0),
            1 => row_l.1.cmp(&row_r.1),
            2 => row_l.2.cmp(&row_r.2),
            3 => row_l.3.cmp(&row_r.3),
            4 => row_l.4.cmp(&row_r.4),
            5 => row_l.5.cmp(&row_r.5),
            6 => row_l.6.cmp(&row_r.6),
            _ => row_l.0.cmp(&row_r.0)
        }
    }

    fn new_empty_row(&mut self) -> ProcessTableData {
        // Instead of requiring `Default` trait for row data types, the viewer is
        // responsible of providing default creation method.
        ProcessTableData::default()
    }
}


/* -------------------------------------------- Codec ------------------------------------------- */

struct Codec;

impl RowCodec<ProcessTableData> for Codec {
    type DeserializeError = &'static str;

    fn encode_column(&mut self, src_row: &ProcessTableData, column: usize, dst: &mut String) {
        match column {
            0 => dst.push_str(&src_row.0),
            1 => dst.push_str(&src_row.1),
            2 => {
                // Parse the input into a NaiveDateTime
                let naive_datetime = NaiveDateTime::parse_from_str(
                    &src_row.2,
                    "%Y-%m-%d %H:%M:%S"
                )
                .expect("Failed to parse datetime");
                // Convert to a DateTime with Utc timezone
                let datetime: DateTime<Utc> = DateTime::from_naive_utc_and_offset(naive_datetime, Utc);
                // Format the DateTime into yyyy/mm/dd
                let formatted_date = datetime.format("%m/%d/%Y").to_string();
                dst.push_str(&formatted_date);
            },
            3 => dst.push_str(&src_row.3),
            4 => dst.push_str(&src_row.4),
            5 => dst.push_str(&src_row.5),
            6 => dst.push_str(&src_row.6),
            _ => unreachable!(),
        }
    }

    fn decode_column(
        &mut self,
        src_data: &str,
        column: usize,
        dst_row: &mut ProcessTableData,
    ) -> Result<(), DecodeErrorBehavior> {
        match column {
            0 => dst_row.0.replace_range(.., src_data),
            1 => dst_row.1 = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            2 => dst_row.2 = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            3 => dst_row.3 = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            4 => dst_row.4 = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            5 => dst_row.5 = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            6 => dst_row.6 = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            _ => unreachable!(),
        }

        Ok(())
    }

    fn create_empty_decoded_row(&mut self) -> ProcessTableData {
        ProcessTableData::default()
    }
}