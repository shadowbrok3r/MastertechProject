use crate::egui_data_table::{viewer::{default_hotkeys, DecodeErrorBehavior, RowCodec, TrivialConfig, UiActionContext}, RowViewer, UiAction};
use eframe::egui::{Button, Color32, KeyboardShortcut, OpenUrl, Response, RichText, TextEdit, Ui, Widget};
use egui_extras::Column as TableColumnConfig;
use serde::{Deserialize, Serialize};
use crossbeam::channel::Sender;
use log::info;

const BASE_URL: &str = "https://pclaptops.mojo11.com/pcladmin/index.php?controller=AdminOrders&vieworder=&id_order=";

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct StockData {
    pub result: Vec<RawStockData>,
}

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct SerialData {
    pub result: Vec<SerialInfo>,
}

#[derive(Default, Debug, Serialize, Deserialize, Clone)]
pub struct SerialInfo {
    pub id: u64,
    pub bs_prest_ref: BoolOrString,
    // pub bs_sale_line_id: BoolOrString,
    pub product_id: ProductID,
    pub name: String,
}

#[derive(Default, Debug, Serialize, Deserialize, Clone)]
pub struct RawStockData {
    pub available_quantity: f32,
    pub id: u64,
    pub inventory_diff_quantity: f32,
    pub inventory_quantity: f32,
    pub lot_id: LotID,
    pub product_id: ProductID,
    pub quantity: f32,
    pub reserved_quantity: f32,
    pub location_id: LotID,
}

#[derive(Default, Debug, Serialize, Deserialize, Clone)]
pub struct LotID(pub i32, pub String);

#[derive(Default, Debug, Serialize, Deserialize, Clone)]
pub struct ProductID(pub i32, pub String);

// Don't need to implement any trait on row data itself.
#[derive(Default, Serialize, Clone)]
pub struct SerialsData(pub String, pub String, pub String, pub String, pub bool);
/// Every logic is defined in `Viewer`
#[derive(Default, Serialize)]
pub struct SerialsViewer {
    pub filter: String,
    pub row_protection: bool,
    pub hotkeys: Vec<(KeyboardShortcut, UiAction)>,
    #[serde(skip)]
    pub stock_tx: Option<Sender<SerialData>>,
}

// There are several methods that MUST be implemented to make the viewer work correctly.
impl RowViewer<SerialsData> for SerialsViewer {
    fn try_create_codec(&mut self, _: bool) -> Option<impl RowCodec<SerialsData>> {
        Some(Codec)
    }

    fn num_columns(&mut self) -> usize {
        5
    }

    fn column_name(&mut self, column: usize) -> std::borrow::Cow<'static, str> {
        ["Item Code", "Serial Number", "Order", "Location", "     "][column].into()
    }

    fn is_sortable_column(&mut self, column: usize) -> bool {
        [true, true, true, true, true][column]
    }

    fn row_filter_hash(&mut self) -> &impl std::hash::Hash {
        &self.filter
    }

    fn filter_row(&mut self, row: &SerialsData) -> bool {
        let filter = &self.filter.to_uppercase();

        row.0.contains(&format!("[{}]", filter))
            // || row.0.contains(filter.to_string() + "]")
            || row.0.contains(filter)
            || row.1.contains(filter)
    }

    fn hotkeys(&mut self, context: &UiActionContext) -> Vec<(KeyboardShortcut, UiAction)> {
        let hotkeys = default_hotkeys(context);
        self.hotkeys.clone_from(&hotkeys);
        hotkeys
    }

    fn show_cell_view(&mut self, ui: &mut Ui, row: &SerialsData, column: usize) {
        let _ = match column {
            0 => {
                ui.horizontal_centered(|ui| {
                    if let Some(splt) = row.0.split_once(']') {
                        let strings = splt.0.split_terminator('/').collect::<Vec<&str>>();
                        if strings.len() == 2 {
                            if let Some(s) = strings.get(0) {
                                ui.colored_label(Color32::LIGHT_GREEN, s.to_string() + "/");
                            }
                            if let Some(s) = strings.get(1) {
                                ui.colored_label(
                                    Color32::from_rgb(42, 195, 222),
                                    s.to_string() + "]",
                                );
                            }
                        } else if strings.len() == 3 {
                            if let Some(s) = strings.get(0) {
                                ui.colored_label(Color32::LIGHT_GREEN, s.to_string() + "/");
                            }

                            if let Some(s) = strings.get(1) {
                                ui.colored_label(Color32::LIGHT_BLUE, s.to_string() + "/");
                            }

                            if let Some(s) = strings.get(2) {
                                ui.colored_label(
                                    Color32::from_rgb(42, 195, 222),
                                    s.to_string() + "]",
                                );
                            }
                        } else {
                            if let Some(s) = strings.get(0) {
                                ui.colored_label(
                                    Color32::from_rgb(42, 195, 222),
                                    s.to_string() + "]",
                                );
                            }
                        }
                        ui.add_space(10.);
                        ui.label(splt.1)
                    } else {
                        ui.label(&row.0)
                    }
                })
                .inner
            }
            1 => {
                ui.horizontal_centered(|ui| {
                    ui.add_space(5.);
                    ui.colored_label(Color32::from_rgb(42, 195, 222), &row.1)
                })
                .inner
            }
            3 => ui.vertical_centered(|ui| ui.label(&row.3)).inner,
            2 => {
                ui.vertical_centered_justified(|ui| {
                    let color = if &row.2 == "Not Attached" {
                        Color32::from_rgb(191, 33, 101)
                    } else if &row.2 == "S/N Info ⮫" {
                        Color32::from_rgb(191, 33, 101)
                    } else {
                        Color32::from_rgb(51, 255, 189)
                    };
                    Button::new(RichText::new(&row.2).color(color)).ui(ui)
                })
                .inner
            }
            4 => {
                ui.vertical_centered_justified(|ui| ui.checkbox(&mut { row.4 }, ""))
                    .inner
            }
            _ => unreachable!(),
        };
    }

    fn show_cell_editor(
        &mut self,
        ui: &mut Ui,
        row: &mut SerialsData,
        column: usize,
    ) -> Option<Response> {
        ui.vertical_centered_justified(|ui| {
            match column {
                0 => {
                    TextEdit::multiline(&mut row.0)
                        .desired_rows(1)
                        .code_editor()
                        .show(ui)
                        .response
                }
                1 => {
                    TextEdit::multiline(&mut row.1)
                        .desired_rows(1)
                        .code_editor()
                        .show(ui)
                        .response
                }
                3 => {
                    TextEdit::multiline(&mut row.3)
                        .desired_rows(1)
                        .code_editor()
                        .show(ui)
                        .response
                }
                // 2 => {
                //     if row.2.ne("Not Attached") {
                //         Hyperlink::from_label_and_url(
                //             format!(" {}", row.2.clone()), 
                //             format!("{BASE_URL}{}", row.2.clone())
                //         ).open_in_new_tab(true).ui(ui)
                //     }
                // },
                4 => ui.checkbox(&mut row.4, ""),
                _ => unreachable!(),
            }
            .into() // To make focusing work correctly, valid response must be returned.
        })
        .inner
    }
    
    fn on_cell_view_response(
        &mut self,
        row: &SerialsData,
        column: usize,
        resp: &eframe::egui::Response,
    ) -> Option<Box<SerialsData>> {
        match column {
            2 => {
                if resp.clicked() {
                    OpenUrl::new_tab(format!("{BASE_URL}{}", row.2.clone()));
                    None
                } else { None }
            },
            _ => { 
                None 
            }
        }
    }

    fn set_cell_value(&mut self, src: &SerialsData, dst: &mut SerialsData, column: usize) {
        info!("Source: {:?}\nDest: {:?}\nCol: {:?}", src.2, dst.2, column);
        match column {
            0 => dst.0 = src.0.clone(),
            1 => dst.1 = src.1.clone(),
            2 => dst.2 = src.2.clone(),
            3 => dst.3 = src.3.clone(),
            4 => dst.4 = src.4,
            _ => unreachable!(),
        }
    }

    fn compare_cell(
        &self,
        row_l: &SerialsData,
        row_r: &SerialsData,
        column: usize,
    ) -> std::cmp::Ordering {
        match column {
            0 => row_l.0.cmp(&row_r.0),
            1 => row_l.1.cmp(&row_r.1),
            2 => {
                let l_contains_not_attached = row_l.2.contains("Not Attached");
                let r_contains_not_attached = row_r.2.contains("Not Attached");

                match (l_contains_not_attached, r_contains_not_attached) {
                    // If both contain "Not Attached", treat them as equal
                    (true, true) => std::cmp::Ordering::Equal,
                    // If row_l contains "Not Attached" but row_r doesn't, consider row_r "greater"
                    (true, false) => std::cmp::Ordering::Less,
                    // If row_r contains "Not Attached" but row_l doesn't, consider row_l "greater"
                    (false, true) => std::cmp::Ordering::Greater,
                    // Otherwise, compare the actual values
                    (false, false) => row_l.2.cmp(&row_r.2),
                }
            }
            3 => row_l.3.cmp(&row_r.3),
            4 => row_l.4.cmp(&row_r.4),
            _ => unreachable!(),
        }
    }

    fn new_empty_row(&mut self) -> SerialsData {
        // Instead of requiring `Default` trait for row data types, the viewer is
        // responsible of providing default creation method.
        SerialsData::default()
    }

    fn column_render_config(&mut self, column: usize) -> TableColumnConfig {
        let col_config = TableColumnConfig::auto();
        match column {
            0 => col_config.resizable(true).at_least(400.).at_most(550.),
            1 => col_config.resizable(true).at_least(200.).at_most(250.),
            3 => col_config.resizable(false).at_least(60.).at_most(60.),
            2 => col_config.resizable(false).at_least(150.).at_most(150.),
            4 => col_config.resizable(false).at_most(50.),
            _ => col_config,
        }
    }

    fn trivial_config(&mut self) -> TrivialConfig {
        TrivialConfig {
            table_row_height: Some(20.),
            ..Default::default()
        }
    }
}


/* -------------------------------------------- Codec ------------------------------------------- */

struct Codec;

impl RowCodec<SerialsData> for Codec {
    type DeserializeError = &'static str;

    fn encode_column(&mut self, src_row: &SerialsData, column: usize, dst: &mut String) {
        match column {
            0 => dst.push_str(&src_row.0),
            1 => dst.push_str(&src_row.1),
            2 => dst.push_str(&src_row.2),
            3 => dst.push_str(&src_row.3),
            4 => dst.push_str(&format!("{}", src_row.4)),
            _ => unreachable!(),
        }
    }

    fn decode_column(
        &mut self,
        src_data: &str,
        column: usize,
        dst_row: &mut SerialsData,
    ) -> Result<(), DecodeErrorBehavior> {
        match column {
            0 => dst_row.0.replace_range(.., src_data),
            1 => dst_row.1 = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            2 => dst_row.2 = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            3 => dst_row.3 = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            4 => dst_row.4 = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            _ => unreachable!(),
        }

        Ok(())
    }

    fn create_empty_decoded_row(&mut self) -> SerialsData {
        SerialsData::default()
    }
}

fn _round_to_two_decimal_places(value: f64) -> f64 {
    if value > 0.0 {
        (value * 100.0).round() / 100.0
    } else {
        value
    }
}


use serde::de::Deserializer;
use std::fmt;

#[derive(Debug, Serialize, Clone)]
pub enum BoolOrString {
    Bool(bool),
    String(String),
}

impl Default for BoolOrString {
    fn default() -> Self {
        BoolOrString::Bool(false)
    }
}

impl<'de> Deserialize<'de> for BoolOrString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct BoolOrStringVisitor;

        impl<'de> serde::de::Visitor<'de> for BoolOrStringVisitor {
            type Value = BoolOrString;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a bool or a string")
            }

            fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
                Ok(BoolOrString::Bool(value))
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(BoolOrString::String(value.to_string()))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(BoolOrString::String(value))
            }
        }

        deserializer.deserialize_any(BoolOrStringVisitor)
    }
}
