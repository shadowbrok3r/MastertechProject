use crate::egui_data_table::{viewer::{default_hotkeys, DecodeErrorBehavior, RowCodec, UiActionContext}, RowViewer, UiAction};
use database::schema::utilities::PhoneNumberFormatter;
use eframe::egui::{Color32, KeyboardShortcut, Ui};
// use database::schema::{helper_traits::{parse_email_user, EmployeeHelper, TaskNotePayloadHelper}, prestashop_schema::{self, Employee, PrestashopPayload}, utilities::{create_full_task_payload, get_prestashop_payload, get_task_notes_from_db_with_service_number}, ComputerData, CustomerData, TaskNotePayload, TaskPayload, TicketPayload, User, TASK_NOTE_TABLE, TASK_TABLE, TICKET_TABLE};
use chrono::{DateTime, NaiveDateTime, Utc};
use crate::app_state::SharedContext;
use serde::{Deserialize, Serialize};
use egui_extras::Column;

#[derive(Serialize, Default)]
pub struct CustomerRowViewer {
    phone_number_cache: PhoneNumberFormatter,
    filter: String,
    row_protection: bool,
    hotkeys: Vec<(KeyboardShortcut, UiAction)>,
    open_hotkeys: bool,
}

impl SharedContext{
    pub fn customer_view(&mut self, _ui: &mut Ui){

    }
}

// Don't need to implement any trait on row data itself.
#[derive(Default, Serialize, Clone, Deserialize, PartialEq, Debug)]
pub struct CustomerTableData(pub String, pub String, pub String, pub String, pub String, pub String, pub String);

impl RowViewer<CustomerTableData> for CustomerRowViewer {
    fn try_create_codec(&mut self, _: bool) -> Option<impl RowCodec<CustomerTableData>> {
        Some(Codec)
    }

    fn num_columns(&mut self) -> usize {
        7
    }

    fn column_name(&mut self, column: usize) -> std::borrow::Cow<'static, str> {
        ["Customer Name", "Phone Number", "Status", "Sales Rep", "Split Rep", "Needs Call", ""][column].into()
    }

    fn is_sortable_column(&mut self, column: usize) -> bool {
        [true, true, true, true, true, true, true][column]
    }

    fn row_filter_hash(&mut self) -> &impl std::hash::Hash {
        &self.filter
    }

    fn filter_row(&mut self, row: &CustomerTableData) -> bool {
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

    fn show_cell_view(&mut self, ui: &mut eframe::egui::Ui, row: &CustomerTableData, column: usize) {
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
        _ui: &mut eframe::egui::Ui,
        _row: &mut CustomerTableData,
        column: usize,
    ) -> Option<eframe::egui::Response> {
        match column {
            0 => None,
            _ => None,
        }
    }

    fn on_cell_view_response(
        &mut self,
        _row: &CustomerTableData,
        column: usize,
        resp: &eframe::egui::Response,
    ) -> Option<Box<CustomerTableData>> {
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
            .map(|_| Box::new(CustomerTableData::default()))
    }

    fn set_cell_value(
        &mut self,
        src: &CustomerTableData,
        dst: &mut CustomerTableData,
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
        row_l: &CustomerTableData,
        row_r: &CustomerTableData,
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

    fn new_empty_row(&mut self) -> CustomerTableData {
        // Instead of requiring `Default` trait for row data types, the viewer is
        // responsible of providing default creation method.
        CustomerTableData::default()
    }
}


/* -------------------------------------------- Codec ------------------------------------------- */

struct Codec;

impl RowCodec<CustomerTableData> for Codec {
    type DeserializeError = &'static str;

    fn encode_column(&mut self, src_row: &CustomerTableData, column: usize, dst: &mut String) {
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
        dst_row: &mut CustomerTableData,
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

    fn create_empty_decoded_row(&mut self) -> CustomerTableData {
        CustomerTableData::default()
    }
}