use crossbeam::channel::Sender;
use egui_data_table::{viewer::{DecodeErrorBehavior, RowCodec}, RowViewer};
use eframe::egui::{Color32, Link, Response, RichText, Ui, Widget};
use egui_extras::Column as TableColumnConfig;
use database::SurrealValue;
use regex::Regex;
use serde::{Deserialize, Serialize};
use crate::tabs::stock::ProductID;

#[derive(Default, Debug, Serialize, Deserialize, Clone, database::SurrealValue)]
pub struct ExtraInventoryData {
    pub display_name: String,   // Display name is a String
    // pub id: f64,             // ID is a positive integer
    pub list_price: f64,        // Monetary value (with decimals), so f64 is appropriate
    /// Company-wide on-hand units.
    pub qty_available: f64,
    pub standard_price: f64,    // Monetary value (with decimals), so f64 is appropriate
    /// Company-wide forecast units.
    pub virtual_available: f64,
    pub product_variant_id: ProductID,
    pub name: String,
    /// On-hand units in the bare `WAR/Stock` bin.
    #[serde(default)]
    #[surreal(default)]
    pub warehouse_available: f64,
    /// Forecast units in the bare `WAR/Stock` bin.
    #[serde(default)]
    #[surreal(default)]
    pub warehouse_virtual_available: f64,
}

/// Company Stock row; `available` and `virtual_available` cover the bare `WAR/Stock` bin only.
#[derive(Default, Serialize, Clone)]
pub struct StockQuantityData {
    pub name: String,
    pub available: f64,
    pub virtual_available: f64,
    pub std_price: f64,
    pub list_price: f64,
    pub product_id: i32,
    pub company_available: f64,
    pub company_virtual_available: f64,
}

/// `(product_id, name)` of a clicked `# Available` cell.
pub type BinClick = (i32, String);

#[derive(Default, Serialize)]
pub struct StockQuantityViewer {
    pub filter: String,
    pub row_protection: bool,
    #[serde(skip)]
    pub bins_click_tx: Option<Sender<BinClick>>,
}

fn qty_color(qty: f64) -> Color32 {
    if qty <= 10.0 {
        Color32::from_rgb(191, 33, 101)
    } else if qty <= 40.0 {
        Color32::LIGHT_RED
    } else {
        Color32::from_rgb(51, 255, 189)
    }
}

// There are several methods that MUST be implemented to make the viewer work correctly.
impl RowViewer<StockQuantityData> for StockQuantityViewer {
    fn try_create_codec(&mut self, _: bool) -> Option<impl RowCodec<StockQuantityData>> { Some(Codec) }

    fn num_columns(&mut self) -> usize { 5 }

    fn column_name(&mut self, column: usize) -> std::borrow::Cow<'static, str> {
        [
            "Item Code",
            "# Available",
            "# Virtual Available",
            "Std Price",
            "List Price",
        ][column]
            .into()
    }

    fn is_editable_cell(&mut self, _: usize, _row: usize, _row_value: &StockQuantityData) -> bool { false }

    fn is_sortable_column(&mut self, column: usize) -> bool { [true, true, true, true, true][column] }

    fn row_filter_hash(&mut self) -> &impl std::hash::Hash { &self.filter }

    fn filter_row(&mut self, row: &StockQuantityData) -> bool {
        let filter = &self.filter.to_uppercase();

        row.name.contains(&format!("[{}]", filter)) || row.name.contains(filter)
    }

    fn show_cell_view(&mut self, ui: &mut Ui, row: &StockQuantityData, column: usize) {
        let style = ui.style_mut();
        style.interaction.multi_widget_text_select = false;
        style.interaction.selectable_labels = false;

        let _ = match column {
            0 => {
                ui.horizontal_centered(|ui| {
                    if let Some(splt) = row.name.split_once(']') {
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
                        ui.label(&row.name)
                    }
                })
                .inner
            }
            1 => {
                let res = Link::new(RichText::new(format!(" {}", row.available)).color(qty_color(row.available)))
                    .ui(ui)
                    .on_hover_text(format!(
                        "WAR/Stock bin only\nCompany-wide: {}\nClick for every bin holding this item",
                        row.company_available
                    ));
                if res.clicked() && let Some(tx) = self.bins_click_tx.as_ref() {
                    let _ = tx.try_send((row.product_id, row.name.clone()));
                }
                res
            }
            2 => ui
                .label(RichText::new(format!(" {}", row.virtual_available)).color(qty_color(row.virtual_available)))
                .on_hover_text(format!(
                    "WAR/Stock bin only\nCompany-wide: {}",
                    row.company_virtual_available
                )),
            3 => ui.label(format!(" $ {}", round_to_two_decimal_places(row.std_price))),
            4 => ui.label(format!(" $ {}", round_to_two_decimal_places(row.list_price))),
            _ => unreachable!(),
        };
    }

    fn show_cell_editor(
        &mut self,
        ui: &mut Ui,
        row: &mut StockQuantityData,
        column: usize,
    ) -> Option<Response> {
        ui.vertical_centered_justified(|ui| {
            match column {
                0 => ui.label(&row.name),
                1 => ui.label(format!("{}", row.available)),
                2 => ui.label(format!("{}", row.virtual_available)),
                3 => ui.label(format!("{}", row.std_price)),
                4 => ui.label(format!("{}", row.list_price)),
                _ => unreachable!(),
            }
            .into() // To make focusing work correctly, valid response must be returned.
        })
        .inner
    }

    fn set_cell_value(
        &mut self,
        src: &StockQuantityData,
        dst: &mut StockQuantityData,
        column: usize,
    ) {
        match column {
            0 => dst.name = src.name.clone(),
            1 => dst.available = src.available,
            2 => dst.virtual_available = src.virtual_available,
            3 => dst.std_price = src.std_price,
            4 => dst.list_price = src.list_price,
            _ => unreachable!(),
        }
    }

    // fn on_cell_view_response(
    //     &mut self,
    //     row: &StockQuantityData,
    //     column: usize,
    //     resp: &eframe::egui::Response,
    // ) -> Option<Box<StockQuantityData>> {

    // }

    fn compare_cell(
        &self,
        row_l: &StockQuantityData,
        row_r: &StockQuantityData,
        column: usize,
    ) -> std::cmp::Ordering {
        match column {
            0 => row_l.name.cmp(&row_r.name),
            1 => row_l
                .available
                .partial_cmp(&row_r.available)
                .unwrap_or(std::cmp::Ordering::Equal),
            2 => row_l
                .virtual_available
                .partial_cmp(&row_r.virtual_available)
                .unwrap_or(std::cmp::Ordering::Equal),
            3 => row_l
                .std_price
                .partial_cmp(&row_r.std_price)
                .unwrap_or(std::cmp::Ordering::Equal),
            4 => row_l
                .list_price
                .partial_cmp(&row_r.list_price)
                .unwrap_or(std::cmp::Ordering::Equal),
            _ => unreachable!(),
        }
    }

    fn new_empty_row(&mut self) -> StockQuantityData {
        // Instead of requiring `Default` trait for row data types, the viewer is
        // responsible of providing default creation method.
        StockQuantityData::default()
    }

    
    fn column_render_config(&mut self, column: usize, _is_last_visible_column: bool) -> TableColumnConfig {
        let col_config = TableColumnConfig::auto();
        match column {
            0 => col_config.resizable(true).at_least(400.).at_most(550.),
            1 => col_config.resizable(true).at_least(120.).at_most(120.),
            3 => col_config.resizable(true).at_least(120.).at_most(120.),
            2 => col_config.resizable(true).at_least(140.).at_most(140.),
            4 => col_config.resizable(true).at_least(120.).at_most(120.),
            _ => col_config,
        }
    }
}


/* -------------------------------------------- Codec ------------------------------------------- */

struct Codec;

impl RowCodec<StockQuantityData> for Codec {
    type DeserializeError = &'static str;

    fn encode_column(&mut self, src_row: &StockQuantityData, column: usize, dst: &mut String) {
        match column {
            0 => {
                let re = Regex::new(r"\[([^\]]+)\]").unwrap();

                if let Some(caps) = re.captures(&src_row.name) {
                    let inner_text = &caps[1];
                    dst.push_str(inner_text);
                } else {
                    dst.push_str(&src_row.name);
                }
            },
            1 => dst.push_str(&format!("{}", src_row.available)),
            2 => dst.push_str(&format!("{}", src_row.virtual_available)),
            3 => dst.push_str(&format!("{}", src_row.std_price)),
            4 => dst.push_str(&format!("{}", src_row.list_price)),
            _ => unreachable!(),
        }
    }

    fn decode_column(
        &mut self,
        src_data: &str,
        column: usize,
        dst_row: &mut StockQuantityData,
    ) -> Result<(), DecodeErrorBehavior> {
        match column {
            0 => {
                let re = Regex::new(r"\[([^\]]+)\]").unwrap();
                if let Some(caps) = re.captures(&dst_row.name) {
                    dst_row.name = caps[1].to_string();
                }
            },
            1 => dst_row.available = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            2 => dst_row.virtual_available = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            3 => dst_row.std_price = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            4 => dst_row.list_price = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            _ => unreachable!(),
        }

        Ok(())
    }

    fn create_empty_decoded_row(&mut self) -> StockQuantityData {
        StockQuantityData::default()
    }
}

fn round_to_two_decimal_places(value: f64) -> f64 {
    if value > 0.0 {
        (value * 100.0).round() / 100.0
    } else {
        value
    }
}
