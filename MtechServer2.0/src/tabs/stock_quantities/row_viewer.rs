use displays::egui_data_table::{
    viewer::{default_hotkeys, TrivialConfig, UiActionContext},
    RowViewer, UiAction,
};
use eframe::egui::{Color32, KeyboardShortcut, Response, RichText, TextEdit, Ui};

use egui_extras::Column as TableColumnConfig;
use log::info;
use serde::{Deserialize, Serialize};

use crate::tabs::stock::ProductID;

#[derive(Default, Debug, Serialize, Deserialize, Clone)]
pub struct ExtraInventoryData {
    pub display_name: String,   // Display name is a String
    pub id: f64,                // ID is a positive integer
    pub list_price: f64,        // Monetary value (with decimals), so f64 is appropriate
    pub qty_available: f64,     // Quantities should remain as u64 for non-negative integers
    pub standard_price: f64,    // Monetary value (with decimals), so f64 is appropriate
    pub virtual_available: f64, // Quantities should remain as u64 for non-negative integers
    pub product_variant_id: ProductID,
    pub name: String,
}

// Don't need to implement any trait on row data itself.
#[derive(Default, Serialize, Clone)]
pub struct StockQuantityData(pub String, pub f64, pub f64, pub f64, pub f64);

#[derive(Default, Serialize)]
pub struct StockQuantityViewer {
    pub filter: String,
    pub row_protection: bool,
    pub hotkeys: Vec<(KeyboardShortcut, UiAction)>,
    // #[serde(skip)]
    // pub stock_tx: Option<Sender<SerialData>>,
}

// There are several methods that MUST be implemented to make the viewer work correctly.
impl RowViewer<StockQuantityData> for StockQuantityViewer {
    fn num_columns(&mut self) -> usize {
        5
    }

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

    fn is_sortable_column(&mut self, column: usize) -> bool {
        [true, true, true, true, true][column]
    }

    fn row_filter_hash(&mut self) -> &impl std::hash::Hash {
        &self.filter
    }

    fn filter_row(&mut self, row: &StockQuantityData) -> bool {
        let filter = &self.filter.to_uppercase();

        row.0.contains(&format!("[{}]", filter)) || row.0.contains(filter)
    }

    fn hotkeys(&mut self, context: &UiActionContext) -> Vec<(KeyboardShortcut, UiAction)> {
        let hotkeys = default_hotkeys(context);
        self.hotkeys.clone_from(&hotkeys);
        hotkeys
    }

    fn show_cell_view(&mut self, ui: &mut Ui, row: &StockQuantityData, column: usize) {
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
                let color = if row.1 <= 10.0 {
                    Color32::from_rgb(191, 33, 101)
                } else if row.1 > 10.0 && row.1 <= 40.0 {
                    Color32::LIGHT_RED
                } else {
                    Color32::from_rgb(51, 255, 189)
                };
                ui.label(RichText::new(format!(" {}", &row.1)).color(color))
            }
            2 => {
                let color = if row.2 <= 10.0 {
                    Color32::from_rgb(191, 33, 101)
                } else if row.2 > 10.0 && row.2 <= 40.0 {
                    Color32::LIGHT_RED
                } else {
                    Color32::from_rgb(51, 255, 189)
                };
                ui.label(RichText::new(format!(" {}", &row.2)).color(color))
            }
            3 => ui.label(format!(" $ {}", round_to_two_decimal_places(row.3))),
            4 => ui.label(format!(" $ {}", round_to_two_decimal_places(row.4))),
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
                0 => {
                    TextEdit::multiline(&mut format!("{}", row.0))
                        .desired_rows(1)
                        .code_editor()
                        .show(ui)
                        .response
                }
                1 => {
                    TextEdit::multiline(&mut format!("{}", row.1))
                        .desired_rows(1)
                        .code_editor()
                        .show(ui)
                        .response
                }
                2 => {
                    TextEdit::multiline(&mut format!("{}", row.2))
                        .desired_rows(1)
                        .code_editor()
                        .show(ui)
                        .response
                }
                3 => {
                    TextEdit::multiline(&mut format!("{}", row.3))
                        .desired_rows(1)
                        .code_editor()
                        .show(ui)
                        .response
                }
                4 => {
                    TextEdit::multiline(&mut format!("{}", row.4))
                        .desired_rows(1)
                        .code_editor()
                        .show(ui)
                        .response
                }
                _ => unreachable!(),
            }
            .into() // To make focusing work correctly, valid response must be returned.
        })
        .inner
    }

    // fn on_cell_view_response(
    //     &mut self,
    //     row: &StockQuantityData,
    //     column: usize,
    //     resp: &eframe::egui::Response,
    // ) -> Option<Box<StockQuantityData>> {
    //     match column {
    //         2 => {
    //             // if resp.clicked() {
    //             // Some(Box::new(row.clone()))
    //             None
    //         }
    //         _ => None,
    //     }
    // }

    fn set_cell_value(
        &mut self,
        src: &StockQuantityData,
        dst: &mut StockQuantityData,
        column: usize,
    ) {
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
        row_l: &StockQuantityData,
        row_r: &StockQuantityData,
        column: usize,
    ) -> std::cmp::Ordering {
        match column {
            0 => row_l.0.cmp(&row_r.0),
            1 => row_l
                .1
                .partial_cmp(&row_r.1)
                .unwrap_or(std::cmp::Ordering::Equal),
            2 => row_l
                .2
                .partial_cmp(&row_r.2)
                .unwrap_or(std::cmp::Ordering::Equal),
            3 => row_l
                .3
                .partial_cmp(&row_r.3)
                .unwrap_or(std::cmp::Ordering::Equal),
            4 => row_l
                .4
                .partial_cmp(&row_r.4)
                .unwrap_or(std::cmp::Ordering::Equal),
            _ => unreachable!(),
        }
    }

    fn new_empty_row(&mut self) -> StockQuantityData {
        // Instead of requiring `Default` trait for row data types, the viewer is
        // responsible of providing default creation method.
        StockQuantityData(
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
        )
    }

    fn column_render_config(&mut self, column: usize) -> TableColumnConfig {
        let col_config = TableColumnConfig::auto();
        match column {
            0 => col_config.resizable(true).at_least(400.).at_most(550.),
            1 => col_config.resizable(true).at_most(120.),
            3 => col_config.resizable(true).at_most(120.),
            2 => col_config.resizable(true).at_most(140.),
            4 => col_config.resizable(true).at_most(120.),
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

fn round_to_two_decimal_places(value: f64) -> f64 {
    if value > 0.0 {
        (value * 100.0).round() / 100.0
    } else {
        value
    }
}
