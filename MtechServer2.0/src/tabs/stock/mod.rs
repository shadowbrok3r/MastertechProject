use crate::app_state::MtechServerContext;
use anyhow::{Error, Result};
use crossbeam::channel::Sender;
use database::DATABASE;
use displays::egui_data_table::{
    viewer::{default_hotkeys, TrivialConfig, UiActionContext},
    Renderer, RowViewer, UiAction,
};
use eframe::egui::{
    Button, CentralPanel, Color32, KeyboardShortcut, Response, RichText, SidePanel, TextEdit, Ui,
    Widget,
};

use egui_extras::Column as TableColumnConfig;
use log::info;
use serde::{Deserialize, Serialize};
use wasm_bindgen_futures::spawn_local;

// https://github.com/rerun-io/egui_table/blob/main/egui_table/src/table.rs
impl MtechServerContext {
    pub fn stock_viewer(&mut self, ui: &mut Ui) {
        SidePanel::right("Hotkeys")
            .default_width(500.)
            .show_inside(ui, |ui| {
                ui.vertical_centered_justified(|ui| {
                    ui.heading("Hotkeys");
                    ui.separator();
                    ui.add_space(0.);

                    for (k, a) in &self.data_viewer.hotkeys {
                        Button::new(format!("{a:?}"))
                            .shortcut_text(ui.ctx().format_shortcut(k))
                            // .wrap_mode(TextWrapMode::Wrap)
                            // .sense(Sense::hover())
                            .ui(ui);
                        ui.add_space(10.);
                    }
                });
            });

        CentralPanel::default().show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                if Button::new("Refresh").ui(ui).clicked() {
                    let stock_tx = self.stock_channel.0.clone();
                    spawn_local(async move {
                        // let login_odoo = odoo_auth().await;
                        // if let Ok(cookie) = login_odoo {
                        let stock = get_stock(stock_tx.clone()).await;
                        info!("Stock call: {stock:?}");
                    });
                }

                TextEdit::singleline(&mut self.data_viewer.filter).ui(ui);
            });

            ui.add(Renderer::new(&mut self.data_table, &mut self.data_viewer));
        });
    }
}

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct StockData {
    result: Vec<RawStockData>,
}

#[derive(Default, Debug, Serialize, Deserialize, Clone)]
pub struct RawStockData {
    pub available_quantity: f32,
    pub id: u64,
    pub inventory_diff_quantity: f32,
    pub inventory_quantity: f32,
    // #[serde(deserialize_with = "deserialize_to_lot_id")]
    pub lot_id: LotID,
    // #[serde(deserialize_with = "deserialize_to_product_id")]
    pub product_id: ProductID,
    pub quantity: f32,
    pub reserved_quantity: f32,
}

#[derive(Default, Debug, Serialize, Deserialize, Clone)]
pub struct LotID(pub i32, pub String);

#[derive(Default, Debug, Serialize, Deserialize, Clone)]
pub struct ProductID(pub i32, pub String);

// Don't need to implement any trait on row data itself.
#[derive(Default, Serialize)]
pub struct MyRowData(pub String, pub String, pub String, pub String, pub bool);

/// Every logic is defined in `Viewer`
#[derive(Default, Serialize)]
pub struct MyRowViewer {
    filter: String,
    row_protection: bool,
    hotkeys: Vec<(KeyboardShortcut, UiAction)>,
}

// There are several methods that MUST be implemented to make the viewer work correctly.
impl RowViewer<MyRowData> for MyRowViewer {
    fn num_columns(&mut self) -> usize {
        5
    }

    fn column_name(&mut self, column: usize) -> std::borrow::Cow<'static, str> {
        ["Item Code", "Serial Number", "Attached", "Location", ""][column].into()
    }

    fn is_sortable_column(&mut self, column: usize) -> bool {
        [true, true, true, true, true][column]
    }

    fn row_filter_hash(&mut self) -> &impl std::hash::Hash {
        &self.filter
    }

    fn filter_row(&mut self, row: &MyRowData) -> bool {
        row.0.contains(&self.filter) || row.1.contains(&self.filter)
    }

    fn hotkeys(&mut self, context: &UiActionContext) -> Vec<(KeyboardShortcut, UiAction)> {
        let hotkeys = default_hotkeys(context);
        self.hotkeys.clone_from(&hotkeys);
        hotkeys
    }

    fn show_cell_view(&mut self, ui: &mut Ui, row: &MyRowData, column: usize) {
        let _ = match column {
            0 => {
                ui.horizontal_centered(|ui| {
                    if let Some(splt) = row.0.split_once(']') {
                        // ui.label("[");

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

                        // ui.label(text)
                    } else {
                        ui.label(&row.0)
                    }
                })
                .inner
            }
            1 => {
                ui.horizontal_centered(|ui| {
                    ui.colored_label(Color32::from_rgb(42, 195, 222), &row.1)
                })
                .inner
            }
            3 => ui.vertical_centered(|ui| ui.label(&row.3)).inner,
            2 => {
                ui.vertical_centered_justified(|ui| {
                    Button::new(RichText::new("S/N Info ⮫").color(Color32::from_rgb(155, 50, 227)))
                        .ui(ui)
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
        row: &mut MyRowData,
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
                2 => Button::new("S/N Info ⮫").ui(ui),
                4 => ui.checkbox(&mut row.4, ""),
                _ => unreachable!(),
            }
            .into() // To make focusing work correctly, valid response must be returned.
        })
        .inner
    }

    fn set_cell_value(&mut self, src: &MyRowData, dst: &mut MyRowData, column: usize) {
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
        row_l: &MyRowData,
        row_r: &MyRowData,
        column: usize,
    ) -> std::cmp::Ordering {
        match column {
            0 => row_l.0.cmp(&row_r.0),
            1 => row_l.1.cmp(&row_r.1),
            2 => row_l.2.cmp(&row_r.2),
            3 => row_l.3.cmp(&row_r.3),
            4 => row_l.4.cmp(&row_r.4),
            _ => unreachable!(),
        }
    }

    fn new_empty_row(&mut self) -> MyRowData {
        // Instead of requiring `Default` trait for row data types, the viewer is
        // responsible of providing default creation method.
        MyRowData(
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
            1 => col_config.resizable(true).at_least(200.).at_most(250.),
            3 => col_config.resizable(false).at_least(50.).at_most(50.),
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

pub async fn get_stock(stock_tx: Sender<Vec<RawStockData>>) -> Result<(), Error> {
    let res: Option<StockData> = DATABASE
        .query("RETURN fn::store_stock('session_id=d3c1efd52d94f1cd185eba423f1835cc60f09473', 76, 1000)")
        // .bind(("cookie", cookie))
        .await?
        .take(0)?;

    info!("Result: {res:?}");

    stock_tx.try_send(res.unwrap().result)?;
    Ok(())
}

pub async fn find_attached_serial(
    serial: String,
    stock_tx: Sender<StockData>,
) -> Result<(), Error> {
    let res: Option<StockData> = DATABASE
        .query("RETURN fn::find_attached_serial('session_id=d3c1efd52d94f1cd185eba423f1835cc60f09473', $serial)")
        .bind(("serial", serial))
        .await?
        .take(0)?;

    info!("Result: {res:?}");

    stock_tx.try_send(res.unwrap())?;
    Ok(())
}

pub async fn find_products_by_name(
    serial: String,
    stock_tx: Sender<StockData>,
) -> Result<(), Error> {
    let res: Option<StockData> = DATABASE
        .query("RETURN fn::search_stock('session_id=d3c1efd52d94f1cd185eba423f1835cc60f09473', $serial)")
        .bind(("serial", serial))
        .await?
        .take(0)?;

    info!("Result: {res:?}");

    stock_tx.try_send(res.unwrap())?;
    Ok(())
}
