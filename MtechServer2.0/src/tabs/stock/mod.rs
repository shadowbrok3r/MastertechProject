use crate::app_state::MtechServerContext;
use anyhow::{Error, Result};
use crossbeam::channel::Sender;
use database::{schema::Store, DATABASE};
use displays::egui_data_table::{
    viewer::{default_hotkeys, TrivialConfig, UiActionContext},
    Renderer, RowViewer, UiAction,
};
use eframe::egui::{
    Button, CentralPanel, Color32, ComboBox, KeyboardShortcut, Response, RichText, ScrollArea,
    SidePanel, TextEdit, TopBottomPanel, Ui, Widget,
};

use egui_extras::Column as TableColumnConfig;
use log::info;
use serde::{Deserialize, Serialize};
use wasm_bindgen_futures::spawn_local;

impl MtechServerContext {
    pub fn stock_viewer(&mut self, ui: &mut Ui) {
        SidePanel::right("Hotkeys")
            .default_width(500.)
            .show_inside(ui, |ui| {
                ui.vertical_centered_justified(|ui| {
                    ui.heading("Hotkeys");
                    ui.separator();
                    ui.add_space(0.);
                    ScrollArea::new([false, true]).show(ui, |ui| {
                        for (k, a) in &self.data_viewer.hotkeys {
                            Button::new(format!("{a:?}"))
                                .shortcut_text(ui.ctx().format_shortcut(k))
                                .ui(ui);
                            ui.add_space(10.);
                        }
                    });
                });
            });
        TopBottomPanel::top("StockTopPanel")
            .exact_height(30.)
            .show_inside(ui, |ui| {
                ui.horizontal_top(|ui| {
                    TextEdit::singleline(&mut self.data_viewer.filter).ui(ui);

                    ui.add_space(10.);

                    let selected = &mut self.store_selection;
                    let current = selected.clone();

                    let selected_text = match selected {
                        76 => Store::RIV.as_str(),
                        73 => Store::LTN.as_str(),
                        74 => Store::MUR.as_str(),
                        78 => Store::WJ.as_str(),
                        75 => Store::ORE.as_str(),
                        72 => Store::AF.as_str(),
                        77 => Store::SAN.as_str(),
                        _ => Store::RIV.as_str(),
                    };

                    ComboBox::new("Store_Selection", "")
                        .selected_text(selected_text)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(selected, 76, "RIV");
                            ui.selectable_value(selected, 73, "LTN");
                            ui.selectable_value(selected, 74, "MUR");
                            ui.selectable_value(selected, 78, "WJ");
                            ui.selectable_value(selected, 75, "ORE");
                            ui.selectable_value(selected, 72, "AF");
                            ui.selectable_value(selected, 77, "SAN");
                        });

                    if *selected != current {
                        let stock_tx = self.stock_channel.0.clone();
                        let store_selection = self.store_selection;
                        spawn_local(async move {
                            info!("Store: {:?}", store_selection);
                            let stock = get_stock(stock_tx.clone(), store_selection).await;
                            info!("Stock call: {stock:?}");
                        });
                    }
                    ui.add_space(10.);

                    if Button::new("Refresh").ui(ui).clicked() {
                        let stock_tx = self.stock_channel.0.clone();
                        let store_selection = self.store_selection;
                        spawn_local(async move {
                            let stock = get_stock(stock_tx.clone(), store_selection).await;
                            info!("Stock call: {stock:?}");
                        });
                    }
                    ui.add_space(10.);

                    if Button::new("Refresh S/N Info").ui(ui).clicked() {
                        let tx = self.serial_channel.0.clone();
                        let data_table = self.data_table.iter();
                        let sns = data_table.map(|r| r.1.clone()).collect::<Vec<String>>();
                        spawn_local(async move {
                            let _res = find_attached_serials(sns, tx.clone()).await;
                        });
                    }
                });
            });

        CentralPanel::default().show_inside(ui, |ui| {
            ui.add(Renderer::new(&mut self.data_table, &mut self.data_viewer));
        });
    }
}

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
pub struct MyRowData(pub String, pub String, pub String, pub String, pub bool);

/// Every logic is defined in `Viewer`
#[derive(Default, Serialize)]
pub struct MyRowViewer {
    filter: String,
    row_protection: bool,
    hotkeys: Vec<(KeyboardShortcut, UiAction)>,
    #[serde(skip)]
    pub stock_tx: Option<Sender<SerialData>>,
}

// There are several methods that MUST be implemented to make the viewer work correctly.
impl RowViewer<MyRowData> for MyRowViewer {
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
                2 => Button::new(&row.2).ui(ui),
                4 => ui.checkbox(&mut row.4, ""),
                _ => unreachable!(),
            }
            .into() // To make focusing work correctly, valid response must be returned.
        })
        .inner
    }

    // fn on_cell_view_response(
    //     &mut self,
    //     row: &MyRowData,
    //     column: usize,
    //     resp: &eframe::egui::Response,
    // ) -> Option<Box<MyRowData>> {
    //     match column {
    //         2 => {
    //             // if resp.clicked() {
    //             // Some(Box::new(row.clone()))
    //             None
    //         }
    //         _ => None,
    //     }
    // }

    fn set_cell_value(&mut self, src: &MyRowData, dst: &mut MyRowData, column: usize) {
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

pub async fn get_stock(stock_tx: Sender<Vec<RawStockData>>, location: u64) -> Result<(), Error> {
    let res: Option<StockData> = DATABASE
        .query("RETURN fn::store_stock($location, 5000)")
        .bind(("location", location))
        .await?
        .take(0)?;

    // info!("Result: {res:?}");

    stock_tx.try_send(res.unwrap().result)?;
    Ok(())
}

pub async fn find_attached_serial(
    serial: String,
    stock_tx: Sender<SerialData>,
) -> Result<(), Error> {
    // info!("Finding S/N info: {serial}");
    let res: Option<SerialData> = DATABASE
        .query("RETURN fn::find_attached_serial($serial)")
        .bind(("serial", serial))
        .await?
        .take(0)?;

    // info!("Result: {res:?}");

    stock_tx.try_send(res.unwrap())?;
    Ok(())
}

pub async fn find_attached_serials(
    serials: Vec<String>,
    stock_tx: Sender<SerialData>,
) -> Result<(), Error> {
    // info!("Finding S/N info: {serials:?}");
    let res: Option<SerialData> = DATABASE
        .query("RETURN fn::find_attached_serials($serials)")
        .bind(("serials", serials))
        .await?
        .take(0)?;

    // info!("Result: {res:?}");

    stock_tx.try_send(res.unwrap())?;
    Ok(())
}

pub async fn find_products_by_name(
    serial: String,
    stock_tx: Sender<StockData>,
) -> Result<(), Error> {
    let res: Option<StockData> = DATABASE
        .query("RETURN fn::search_stock($serial)")
        .bind(("serial", serial))
        .await?
        .take(0)?;

    // info!("Result: {res:?}");

    stock_tx.try_send(res.unwrap())?;
    Ok(())
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
