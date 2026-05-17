use egui_data_table::{viewer::{default_hotkeys, DecodeErrorBehavior, RowCodec, UiActionContext}, RowViewer, UiAction};
use eframe::egui::{Button, Color32, Hyperlink, KeyboardShortcut, OpenUrl, Response, RichText, Ui, Widget};
use egui_extras::Column as TableColumnConfig;
use serde::{Deserialize, Serialize};
use database::schema::ComputerData;
use crossbeam::channel::Sender;
use database::SurrealValue;
use database::{xidax_order_url, xidax_product_url};
use regex::Regex;

/// Extract only relevant RAM info: DDR type (DDR4/DDR5), speed (MHz), and capacity (GB)
fn format_ram_display(ram: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    let upper = ram.to_uppercase();
    
    // Extract DDR type (DDR4, DDR5)
    if let Some(caps) = Regex::new(r"DDR[45]").ok().and_then(|re| re.find(&upper)) {
        parts.push(caps.as_str().to_string());
    }
    
    // Extract capacity (e.g., 32GB, 16GB, 64GB)
    if let Some(caps) = Regex::new(r"(\d+)\s*GB").ok().and_then(|re| re.captures(&upper)) {
        if let Some(m) = caps.get(1) {
            parts.push(format!("{}GB", m.as_str()));
        }
    }
    
    // Extract speed (e.g., 3200MHz, 6000MHz)
    if let Some(caps) = Regex::new(r"(\d{4,5})\s*MHZ").ok().and_then(|re| re.captures(&upper)) {
        if let Some(m) = caps.get(1) {
            parts.push(format!("{}MHz", m.as_str()));
        }
    }
    
    if parts.is_empty() {
        ram.to_string() // Fallback to original if no patterns matched
    } else {
        parts.join(" ")
    }
}

#[derive(Default, Debug, Serialize, Deserialize, database::SurrealValue)]
pub struct StockData {
    pub result: Vec<RawStockData>,
}

#[derive(Default, Debug, Serialize, Deserialize, database::SurrealValue)]
pub struct SerialData {
    pub result: Vec<SerialInfo>,
}

#[derive(Default, Debug, Serialize, Deserialize, Clone, database::SurrealValue)]
pub struct SerialInfo {
    pub id: u64,
    pub bs_prest_ref: BoolOrString,
    // pub bs_sale_line_id: BoolOrString,
    pub product_id: ProductID,
    pub name: String,
}

#[derive(Default, Debug, Serialize, Deserialize, Clone, database::SurrealValue)]
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

#[derive(Default, Debug, Serialize, Deserialize, Clone, database::SurrealValue)]
pub struct LotID(pub i32, pub String);

#[derive(Default, Debug, Serialize, Deserialize, Clone, database::SurrealValue)]
pub struct ProductID(pub i32, pub String);

// Don't need to implement any trait on row data itself.
#[derive(Default, Serialize, Clone)]
pub struct SerialsData(pub String, pub String, pub String, pub String, pub bool);

/// Every logic is defined in `Viewer`
#[derive(Default, Serialize)]
pub struct SerialsViewer {
    pub filter: String,
    pub row_protection: bool,
    #[serde(skip)]
    pub hotkeys: Vec<(KeyboardShortcut, UiAction)>,
    #[serde(skip)]
    pub stock_tx: Option<Sender<SerialData>>,
    pub show_hotkeys: bool,
}

// There are several methods that MUST be implemented to make the viewer work correctly.
impl RowViewer<SerialsData> for SerialsViewer {
    fn try_create_codec(&mut self, _: bool) -> Option<impl RowCodec<SerialsData>> { Some(Codec) }

    fn num_columns(&mut self) -> usize { 5 }

    fn column_name(&mut self, column: usize) -> std::borrow::Cow<'static, str> {
        ["Item Code", "Serial Number", "Order", "Location", "     "][column].into()
    }

    fn is_sortable_column(&mut self, column: usize) -> bool {
        [true, true, true, true, true][column]
    }

    fn row_filter_hash(&mut self) -> &impl std::hash::Hash { &self.filter }

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
        let style = ui.style_mut();
        style.interaction.multi_widget_text_select = false;
        style.interaction.selectable_labels = false;

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
                    let is_clickable = &row.2 != "Not Attached" && &row.2 != "S/N Info ⮫";
                    let color = if !is_clickable {
                        Color32::from_rgb(191, 33, 101)
                    } else {
                        Color32::from_rgb(51, 255, 189)
                    };
                    let res = Button::new(RichText::new(&row.2).color(color)).ui(ui);
                    if is_clickable && res.clicked() {
                        ui.ctx().open_url(OpenUrl::new_tab(xidax_order_url(last_n(&row.2, 7))));
                    }
                    res
                })
                .inner
            }
            4 => ui.vertical_centered_justified(|ui| 
                    ui.checkbox(&mut { row.4 }, "")
                ).inner,
            _ => unreachable!(),
        };
    }

    fn show_cell_editor(
        &mut self,
        ui: &mut Ui,
        row: &mut SerialsData,
        column: usize,
    ) -> Option<Response> {
        match column {
            2 => {
                if &row.2 == "Not Attached" || &row.2 == "S/N Info ⮫" {
                    None
                } else {
                    let url = row.2.clone();
                    let res = Hyperlink::from_label_and_url(
                        format!(" {}", row.2.clone()), 
                        xidax_order_url(last_n(&url, 7))
                    )
                    .open_in_new_tab(true)
                    .ui(ui);
                    Some(res)
                }
            },
            _ => None
        }
    }
    
    fn on_cell_view_response(
        &mut self,
        row: &SerialsData,
        column: usize,
        resp: &eframe::egui::Response,
    ) -> Option<Box<SerialsData>> {
        match column {
            2 => {
                if resp.clicked() && &row.2 != "Not Attached" && &row.2 != "S/N Info ⮫" {
                    log::info!("Clicked on order: {}", row.2);
                    resp.ctx.open_url(OpenUrl::new_tab(xidax_order_url(last_n(&row.2, 7))));
                }
                None
            },
            _ => {
                None
            }
        }
    }

    fn set_cell_value(&mut self, src: &SerialsData, dst: &mut SerialsData, column: usize) {
        // info!("Source: {:?}\nDest: {:?}\nCol: {:?}", src.2, dst.2, column);
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

    fn column_render_config(&mut self, column: usize, _: bool) -> TableColumnConfig {
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

    fn is_editable_cell(&mut self, _: usize, _row: usize, _row_value: &SerialsData) -> bool { false }
}

fn last_n(s: &str, n: usize) -> &str {
    if s.len() < n {
        s
    } else {
        &s[s.len() - n..]
    }
}

/* -------------------------------------------- Codec ------------------------------------------- */

struct Codec;

impl RowCodec<SerialsData> for Codec {
    type DeserializeError = &'static str;

    fn encode_column(&mut self, src_row: &SerialsData, column: usize, dst: &mut String) {
        match column {
            0 => {
                let re = Regex::new(r"\[([^\]]+)\]").unwrap();

                if let Some(caps) = re.captures(&src_row.0) {
                    let inner_text = &caps[1];
                    log::info!("Text inside brackets: {}", inner_text);
                    dst.push_str(&inner_text);
                } else {
                    log::info!("No brackets found");
                    dst.push_str(&src_row.0);
                }
            },
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
            0 => {
                let re = Regex::new(r"\[([^\]]+)\]").unwrap();
                if let Some(caps) = re.captures(src_data) {
                    dst_row.0 = caps[1].to_string();
                }
            },
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

use serde::de::{Deserializer, MapAccess, Visitor};
use serde::ser::Serializer;
use std::fmt;

#[derive(Debug, Clone)]
pub enum BoolOrString {
    Bool(bool),
    String(String),
}

impl Default for BoolOrString {
    fn default() -> Self {
        BoolOrString::Bool(false)
    }
}

// Manual SurrealValue implementation for BoolOrString
impl database::SurrealValue for BoolOrString {
    fn kind_of() -> surrealdb::types::Kind {
        // Can be either bool or string, so use Any
        surrealdb::types::Kind::Any
    }

    fn into_value(self) -> surrealdb::types::Value {
        match self {
            BoolOrString::Bool(b) => surrealdb::types::Value::Bool(b),
            BoolOrString::String(s) => surrealdb::types::Value::String(s),
        }
    }

    fn from_value(value: surrealdb::types::Value) -> Result<Self, surrealdb::Error> {
        match value {
            surrealdb::types::Value::Bool(b) => Ok(BoolOrString::Bool(b)),
            surrealdb::types::Value::String(s) => Ok(BoolOrString::String(s)),
            surrealdb::types::Value::None | surrealdb::types::Value::Null => Ok(BoolOrString::Bool(false)),
            other => Err(surrealdb::Error::validation(format!("Expected bool or string for BoolOrString, got {:?}", other), None).into()),
        }
    }
}

// Custom Serialize to output raw values (not tagged enum format)
// This ensures round-trip compatibility with SurrealDB
impl Serialize for BoolOrString {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            BoolOrString::Bool(b) => serializer.serialize_bool(*b),
            BoolOrString::String(s) => serializer.serialize_str(s),
        }
    }
}

impl<'de> Deserialize<'de> for BoolOrString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct BoolOrStringVisitor;

        impl<'de> Visitor<'de> for BoolOrStringVisitor {
            type Value = BoolOrString;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a bool, a string, or a tagged enum {\"Bool\": bool} / {\"String\": string}")
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

            // Handle tagged enum format from SurrealDB: {"Bool": false} or {"String": "value"}
            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                if let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "Bool" => {
                            let value: bool = map.next_value()?;
                            Ok(BoolOrString::Bool(value))
                        }
                        "String" => {
                            let value: String = map.next_value()?;
                            Ok(BoolOrString::String(value))
                        }
                        _ => Err(serde::de::Error::unknown_variant(&key, &["Bool", "String"])),
                    }
                } else {
                    Err(serde::de::Error::custom("expected a non-empty map"))
                }
            }
        }

        deserializer.deserialize_any(BoolOrStringVisitor)
    }
}

/* ---------------------------------------- Cost Breakdown ---------------------------------------- */

/// Row data for the cost breakdown table
/// (Odoo Product ID, Prestashop Product ID, Product Name, Quantity, Unit Price, Cost)
#[derive(Default, Serialize, Clone, Debug)]
pub struct CostBreakdownData(pub String, pub String, pub String, pub f64, pub f64, pub f64);

/// Viewer for the cost breakdown table
#[derive(Default, Serialize)]
pub struct CostBreakdownViewer {
    pub filter: String,
    pub row_protection: bool,
    /// Selected product IDs (for sum calculation) - uses product_id as unique key
    #[serde(skip)]
    pub selected_products: std::collections::HashSet<String>,
}

impl CostBreakdownViewer {
    /// Clear all selections
    pub fn clear_selection(&mut self) {
        self.selected_products.clear();
    }
}

impl RowViewer<CostBreakdownData> for CostBreakdownViewer {
    fn try_create_codec(&mut self, _: bool) -> Option<impl RowCodec<CostBreakdownData>> { 
        Some(CostBreakdownCodec) 
    }

    fn num_columns(&mut self) -> usize { 6 }

    fn column_name(&mut self, column: usize) -> std::borrow::Cow<'static, str> {
        ["Odoo ID", "Presta ID", "Product Name", "Quantity", "Unit Price", "Cost"][column].into()
    }

    fn is_sortable_column(&mut self, column: usize) -> bool {
        [true, true, true, true, true, true][column]
    }

    fn is_editable_cell(&mut self, _: usize, _row: usize, _row_value: &CostBreakdownData) -> bool { 
        false 
    }

    fn row_filter_hash(&mut self) -> &impl std::hash::Hash { &self.filter }

    fn filter_row(&mut self, row: &CostBreakdownData) -> bool {
        let filter = &self.filter.to_uppercase();
        row.0.to_uppercase().contains(filter) || row.1.to_uppercase().contains(filter) || row.2.to_uppercase().contains(filter)
    }

    fn show_cell_view(&mut self, ui: &mut Ui, row: &CostBreakdownData, column: usize) {
        let style = ui.style_mut();
        style.interaction.multi_widget_text_select = false;
        style.interaction.selectable_labels = false;

        let _ = match column {
            // Odoo Product ID - links to Odoo product page
            0 => {
                if row.0.is_empty() || row.0 == "0" {
                    ui.label(RichText::new("-").color(Color32::GRAY))
                } else {
                    Hyperlink::from_label_and_url(
                        RichText::new(format!(" {}", row.0)).color(Color32::from_rgb(42, 195, 222)),
                        format!("https://odoo.pclaptops.com/web#id={}&menu_id=244&cids=1&action=443&model=product.template&view_type=form", row.0)
                    )
                    .open_in_new_tab(true)
                    .ui(ui)
                }
            }
            // Prestashop Product ID - links to Prestashop product page
            1 => {
                if row.1.is_empty() || row.1 == "0" {
                    ui.label(RichText::new("-").color(Color32::GRAY))
                } else {
                    Hyperlink::from_label_and_url(
                        RichText::new(format!(" {}", row.1)).color(Color32::from_rgb(255, 165, 0)),
                        xidax_product_url(&row.1)
                    )
                    .open_in_new_tab(true)
                    .ui(ui)
                }
            }
            2 => ui.label(&row.2),
            3 => ui.label(format!("{:.0}", row.3)),
            4 => ui.label(format!("$ {:.2}", row.4)),
            5 => {
                let color = if row.5 > 0.0 {
                    Color32::from_rgb(51, 255, 189)
                } else {
                    Color32::from_rgb(150, 150, 150)
                };
                ui.label(RichText::new(format!("$ {:.2}", row.5)).color(color))
            }
            _ => unreachable!(),
        };
    }
    
    fn on_highlight_change(&mut self, highlighted: &[&CostBreakdownData], unhighlighted: &[&CostBreakdownData]) {
        // Remove unhighlighted rows from selection (use Odoo ID + Presta ID as unique key)
        for row in unhighlighted.iter() {
            self.selected_products.remove(&format!("{}:{}", row.0, row.1));
        }
        // Add highlighted rows to selection
        for row in highlighted.iter() {
            self.selected_products.insert(format!("{}:{}", row.0, row.1));
        }
    }

    fn show_cell_editor(
        &mut self,
        ui: &mut Ui,
        row: &mut CostBreakdownData,
        column: usize,
    ) -> Option<Response> {
        ui.vertical_centered_justified(|ui| {
            match column {
                0 => {
                    if row.0.is_empty() || row.0 == "0" {
                        ui.label("-")
                    } else {
                        Hyperlink::from_label_and_url(
                            format!(" {}", row.0), 
                            format!("https://odoo.pclaptops.com/web#id={}&menu_id=244&cids=1&action=443&model=product.template&view_type=form", row.0)
                        )
                        .open_in_new_tab(true)
                        .ui(ui)
                    }
                }
                1 => {
                    if row.1.is_empty() || row.1 == "0" {
                        ui.label("-")
                    } else {
                        Hyperlink::from_label_and_url(
                            format!(" {}", row.1), 
                            xidax_product_url(&row.1)
                        )
                        .open_in_new_tab(true)
                        .ui(ui)
                    }
                }
                2 => ui.label(&row.2),
                3 => ui.label(format!("{:.0}", row.3)),
                4 => ui.label(format!("$ {:.2}", row.4)),
                5 => ui.label(format!("$ {:.2}", row.5)),
                _ => unreachable!(),
            }
            .into()
        })
        .inner
    }

    fn set_cell_value(
        &mut self,
        src: &CostBreakdownData,
        dst: &mut CostBreakdownData,
        column: usize,
    ) {
        match column {
            0 => dst.0 = src.0.clone(),
            1 => dst.1 = src.1.clone(),
            2 => dst.2 = src.2.clone(),
            3 => dst.3 = src.3,
            4 => dst.4 = src.4,
            5 => dst.5 = src.5,
            _ => unreachable!(),
        }
    }

    fn compare_cell(
        &self,
        row_l: &CostBreakdownData,
        row_r: &CostBreakdownData,
        column: usize,
    ) -> std::cmp::Ordering {
        match column {
            0 => row_l.0.cmp(&row_r.0),
            1 => row_l.1.cmp(&row_r.1),
            2 => row_l.2.cmp(&row_r.2),
            3 => row_l.3.partial_cmp(&row_r.3).unwrap_or(std::cmp::Ordering::Equal),
            4 => row_l.4.partial_cmp(&row_r.4).unwrap_or(std::cmp::Ordering::Equal),
            5 => row_l.5.partial_cmp(&row_r.5).unwrap_or(std::cmp::Ordering::Equal),
            _ => unreachable!(),
        }
    }

    fn new_empty_row(&mut self) -> CostBreakdownData {
        CostBreakdownData::default()
    }

    fn column_render_config(&mut self, column: usize, _is_last_visible_column: bool) -> TableColumnConfig {
        let col_config = TableColumnConfig::auto();
        match column {
            0 => col_config.resizable(true).at_least(70.).at_most(100.),  // Odoo ID
            1 => col_config.resizable(true).at_least(70.).at_most(100.),  // Presta ID
            2 => col_config.resizable(true).at_least(150.).at_most(250.), // Product Name
            3 => col_config.resizable(true).at_least(50.).at_most(70.),   // Quantity
            4 => col_config.resizable(true).at_least(80.).at_most(120.),  // Unit Price
            5 => col_config.resizable(true).at_least(80.).at_most(120.),  // Cost
            _ => col_config,
        }
    }
}

struct CostBreakdownCodec;

impl RowCodec<CostBreakdownData> for CostBreakdownCodec {
    type DeserializeError = &'static str;

    fn encode_column(&mut self, src_row: &CostBreakdownData, column: usize, dst: &mut String) {
        match column {
            0 => dst.push_str(&src_row.0),
            1 => dst.push_str(&src_row.1),
            2 => dst.push_str(&src_row.2),
            3 => dst.push_str(&format!("{}", src_row.3)),
            4 => dst.push_str(&format!("{}", src_row.4)),
            5 => dst.push_str(&format!("{}", src_row.5)),
            _ => unreachable!(),
        }
    }

    fn decode_column(
        &mut self,
        src_data: &str,
        column: usize,
        dst_row: &mut CostBreakdownData,
    ) -> Result<(), DecodeErrorBehavior> {
        match column {
            0 => dst_row.0 = src_data.to_string(),
            1 => dst_row.1 = src_data.to_string(),
            2 => dst_row.2 = src_data.to_string(),
            3 => dst_row.3 = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            4 => dst_row.4 = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            5 => dst_row.5 = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            _ => unreachable!(),
        }
        Ok(())
    }

    fn create_empty_decoded_row(&mut self) -> CostBreakdownData {
        CostBreakdownData::default()
    }
}

/* ---------------------------------------- Systems In-Store ---------------------------------------- */

/// System type classification
#[derive(Default, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum SystemType {
    #[default]
    ReadyToRoll,
    Rci,
    Bsd,
    Demo,
    CustomBuild,
}

impl SystemType {
    pub fn as_str(&self) -> &str {
        match self {
            SystemType::ReadyToRoll => "R2R",
            SystemType::Rci => "RCI",
            SystemType::Bsd => "BSD",
            SystemType::Demo => "Demo",
            SystemType::CustomBuild => "Custom",
        }
    }
    
    pub fn from_order_type_id(id: &str) -> Self {
        match id {
            "12" => SystemType::ReadyToRoll,
            "13" => SystemType::Bsd,
            "14" => SystemType::Rci,
            _ => SystemType::CustomBuild,
        }
    }
}

/// Row data for Systems In-Store table
#[derive(Default, Serialize, Deserialize, Clone, Debug)]
pub struct SystemInStoreData {
    pub order_id: String,
    pub customer_id: String,
    pub customer_name: String,
    pub model: String,
    pub price: f64,
    pub cost: f64,
    pub revenue: f64,
    pub spiff: f64,
    pub system_type: SystemType,
    pub cpu: String,
    pub gpu: String,
    pub ram: String,
    pub warranty: String,
    pub store_id: String,
    /// Computer data for task creation (detachable for use with CreateTaskModal)
    pub computer_data: ComputerData,
}

/// Data for customer change request
#[derive(Clone, Debug)]
pub struct CustomerChangeRequest {
    pub order_id: String,
    pub customer_name: String,
}

/// Viewer for Systems In-Store table
#[derive(Serialize)]
pub struct SystemInStoreViewer {
    pub filter: String,
    pub row_protection: bool,
    /// Callback sender for creating tasks
    #[serde(skip)]
    pub task_create_tx: Sender<SystemInStoreData>,
    /// Callback sender for customer change requests (opens modal)
    #[serde(skip)]
    pub customer_change_tx: Sender<CustomerChangeRequest>,
}

impl SystemInStoreViewer {
    pub fn new(
        task_create_tx: Sender<SystemInStoreData>,
        customer_change_tx: Sender<CustomerChangeRequest>,
    ) -> Self {
        Self {
            filter: Default::default(),
            row_protection: Default::default(),
            task_create_tx,
            customer_change_tx,
        }
    }
}

impl RowViewer<SystemInStoreData> for SystemInStoreViewer {
    fn try_create_codec(&mut self, _: bool) -> Option<impl RowCodec<SystemInStoreData>> { 
        Some(SystemInStoreCodec) 
    }

    fn num_columns(&mut self) -> usize { 13 }

    fn column_name(&mut self, column: usize) -> std::borrow::Cow<'static, str> {
        [
            "Order ID", "Customer", "Model", "Price", "Cost", "Revenue", "Spiff",
            "Type", "CPU", "GPU", "RAM", "Warranty", "Create Task"
        ][column].into()
    }

    fn is_sortable_column(&mut self, column: usize) -> bool {
        column < 12 // All columns except the button column
    }

    fn is_editable_cell(&mut self, _: usize, _row: usize, _row_value: &SystemInStoreData) -> bool { 
        false 
    }

    fn row_filter_hash(&mut self) -> &impl std::hash::Hash { &self.filter }

    fn filter_row(&mut self, row: &SystemInStoreData) -> bool {
        let filter = &self.filter.to_uppercase();
        row.order_id.to_uppercase().contains(filter) 
            || row.model.to_uppercase().contains(filter)
            || row.cpu.to_uppercase().contains(filter)
            || row.gpu.to_uppercase().contains(filter)
            || row.customer_name.to_uppercase().contains(filter)
    }

    fn show_cell_view(&mut self, ui: &mut Ui, row: &SystemInStoreData, column: usize) {
        let style = ui.style_mut();
        style.interaction.multi_widget_text_select = false;
        style.interaction.selectable_labels = false;

        match column {
            // Order ID - hyperlink to Prestashop
            0 => {
                Hyperlink::from_label_and_url(
                    RichText::new(&row.order_id).color(Color32::from_rgb(42, 195, 222)),
                    xidax_order_url(&row.order_id)
                )
                .open_in_new_tab(true)
                .ui(ui);
            }
            // Customer - clickable button to change customer (shows ID and name)
            1 => {
                let display_text = if row.customer_name.is_empty() {
                    if row.customer_id.is_empty() {
                        "Unknown".to_string()
                    } else {
                        format!("[{}]", row.customer_id)
                    }
                } else {
                    format!("[{}] {}", row.customer_id, row.customer_name)
                };
                if Button::new(RichText::new(&display_text).color(Color32::from_rgb(180, 180, 255)))
                    .ui(ui)
                    .on_hover_text("Click to change customer")
                    .clicked() 
                {
                    let _ = self.customer_change_tx.try_send(CustomerChangeRequest {
                        order_id: row.order_id.clone(),
                        customer_name: row.customer_name.clone(),
                    });
                }
            }
            2 => { ui.label(&row.model); }
            3 => { ui.label(format!("${:.2}", row.price)); }
            4 => {
                let color = if row.cost > 0.0 {
                    Color32::from_rgb(200, 100, 100)
                } else {
                    Color32::GRAY
                };
                ui.label(RichText::new(format!("${:.2}", row.cost)).color(color));
            }
            5 => {
                let color = if row.revenue >= 0.0 {
                    Color32::LIGHT_GREEN
                } else {
                    Color32::from_rgb(255, 80, 80)
                };
                ui.label(RichText::new(format!("${:.2}", row.revenue)).color(color));
            }
            6 => {
                ui.label(RichText::new(format!("${:.2}", row.spiff)).color(Color32::GOLD));
            }
            7 => {
                let (color, text) = match row.system_type {
                    SystemType::ReadyToRoll => (Color32::from_rgb(100, 200, 100), "R2R"),
                    SystemType::Rci => (Color32::from_rgb(200, 150, 50), "RCI"),
                    SystemType::Bsd => (Color32::from_rgb(100, 150, 200), "BSD"),
                    SystemType::Demo => (Color32::from_rgb(200, 100, 200), "Demo"),
                    SystemType::CustomBuild => (Color32::GRAY, "Custom"),
                };
                ui.label(RichText::new(text).color(color));
            }
            8 => { ui.label(&row.cpu); }
            9 => { ui.label(&row.gpu); }
            10 => { ui.label(format_ram_display(&row.ram)); }
            11 => { 
                let color = if row.warranty.is_empty() || row.warranty == "None" {
                    Color32::GRAY
                } else {
                    Color32::from_rgb(100, 180, 255)
                };
                ui.label(RichText::new(&row.warranty).color(color)); 
            }
            12 => {
                if Button::new("Create Task").ui(ui).clicked() {
                    let _ = self.task_create_tx.try_send(row.clone());
                }
            }
            _ => unreachable!(),
        };
    }

    fn show_cell_editor(
        &mut self,
        ui: &mut Ui,
        row: &mut SystemInStoreData,
        column: usize,
    ) -> Option<Response> {
        ui.vertical_centered_justified(|ui| {
            match column {
                0 => Hyperlink::from_label_and_url(&row.order_id, xidax_order_url(&row.order_id))
                    .open_in_new_tab(true).ui(ui),
                1 => ui.label(&row.customer_name),
                2 => ui.label(&row.model),
                3 => ui.label(format!("${:.2}", row.price)),
                4 => ui.label(format!("${:.2}", row.cost)),
                5 => ui.label(format!("${:.2}", row.revenue)),
                6 => ui.label(format!("${:.2}", row.spiff)),
                7 => ui.label(row.system_type.as_str()),
                8 => ui.label(&row.cpu),
                9 => ui.label(&row.gpu),
                10 => ui.label(format_ram_display(&row.ram)),
                11 => ui.label(&row.warranty),
                12 => ui.label(""),
                _ => unreachable!(),
            }
            .into()
        })
        .inner
    }

    fn set_cell_value(
        &mut self,
        src: &SystemInStoreData,
        dst: &mut SystemInStoreData,
        column: usize,
    ) {
        match column {
            0 => dst.order_id = src.order_id.clone(),
            1 => {
                dst.customer_id = src.customer_id.clone();
                dst.customer_name = src.customer_name.clone();
            }
            2 => dst.model = src.model.clone(),
            3 => dst.price = src.price,
            4 => dst.cost = src.cost,
            5 => dst.revenue = src.revenue,
            6 => dst.spiff = src.spiff,
            7 => dst.system_type = src.system_type.clone(),
            8 => dst.cpu = src.cpu.clone(),
            9 => dst.gpu = src.gpu.clone(),
            10 => dst.ram = src.ram.clone(),
            11 => dst.warranty = src.warranty.clone(),
            12 => {}
            _ => unreachable!(),
        }
    }

    fn compare_cell(
        &self,
        row_l: &SystemInStoreData,
        row_r: &SystemInStoreData,
        column: usize,
    ) -> std::cmp::Ordering {
        match column {
            0 => row_l.order_id.cmp(&row_r.order_id),
            1 => row_l.model.cmp(&row_r.model),
            2 => row_l.price.partial_cmp(&row_r.price).unwrap_or(std::cmp::Ordering::Equal),
            3 => row_l.cost.partial_cmp(&row_r.cost).unwrap_or(std::cmp::Ordering::Equal),
            4 => row_l.revenue.partial_cmp(&row_r.revenue).unwrap_or(std::cmp::Ordering::Equal),
            5 => row_l.spiff.partial_cmp(&row_r.spiff).unwrap_or(std::cmp::Ordering::Equal),
            6 => row_l.system_type.as_str().cmp(row_r.system_type.as_str()),
            7 => row_l.cpu.cmp(&row_r.cpu),
            8 => row_l.gpu.cmp(&row_r.gpu),
            9 => row_l.ram.cmp(&row_r.ram),
            10 => row_l.warranty.cmp(&row_r.warranty),
            11 => std::cmp::Ordering::Equal,
            _ => unreachable!(),
        }
    }

    fn new_empty_row(&mut self) -> SystemInStoreData {
        SystemInStoreData::default()
    }

    fn column_render_config(&mut self, column: usize, _is_last_visible_column: bool) -> TableColumnConfig {
        let col_config = TableColumnConfig::auto();
        match column {
            0 => col_config.resizable(true).at_least(80.).at_most(100.),   // Order ID
            1 => col_config.resizable(true).at_least(200.).at_most(220.),  // Customer
            2 => col_config.resizable(true).at_least(200.).at_most(220.),  // Model
            3 => col_config.resizable(true).at_least(70.).at_most(100.),   // Price
            4 => col_config.resizable(true).at_least(70.).at_most(100.),   // Cost
            5 => col_config.resizable(true).at_least(70.).at_most(100.),   // Revenue
            6 => col_config.resizable(true).at_least(60.).at_most(80.),    // Spiff
            7 => col_config.resizable(true).at_least(50.).at_most(70.),    // Type
            8 => col_config.resizable(true).at_least(150.).at_most(200.),  // CPU
            9 => col_config.resizable(true).at_least(150.).at_most(200.),   // GPU
            10 => col_config.resizable(true).at_least(200.).at_most(200.),   // RAM
            11 => col_config.resizable(true).at_least(150.).at_most(180.), // Warranty
            12 => col_config.resizable(false).at_least(70.).at_most(90.),  // Create Task button
            _ => col_config,
        }
    }
}

struct SystemInStoreCodec;

impl RowCodec<SystemInStoreData> for SystemInStoreCodec {
    type DeserializeError = &'static str;

    fn encode_column(&mut self, src_row: &SystemInStoreData, column: usize, dst: &mut String) {
        match column {
            0 => dst.push_str(&src_row.order_id),
            1 => dst.push_str(&src_row.customer_name),
            2 => dst.push_str(&src_row.model),
            3 => dst.push_str(&format!("{}", src_row.price)),
            4 => dst.push_str(&format!("{}", src_row.cost)),
            5 => dst.push_str(&format!("{}", src_row.revenue)),
            6 => dst.push_str(&format!("{}", src_row.spiff)),
            7 => dst.push_str(src_row.system_type.as_str()),
            8 => dst.push_str(&src_row.cpu),
            9 => dst.push_str(&src_row.gpu),
            10 => dst.push_str(&src_row.ram),
            11 => dst.push_str(&src_row.warranty),
            12 => {}
            _ => unreachable!(),
        }
    }

    fn decode_column(
        &mut self,
        src_data: &str,
        column: usize,
        dst_row: &mut SystemInStoreData,
    ) -> Result<(), DecodeErrorBehavior> {
        match column {
            0 => dst_row.order_id = src_data.to_string(),
            1 => dst_row.customer_name = src_data.to_string(),
            2 => dst_row.model = src_data.to_string(),
            3 => dst_row.price = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            4 => dst_row.cost = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            5 => dst_row.revenue = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            6 => dst_row.spiff = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            7 => dst_row.system_type = match src_data {
                "R2R" => SystemType::ReadyToRoll,
                "RCI" => SystemType::Rci,
                "BSD" => SystemType::Bsd,
                "Demo" => SystemType::Demo,
                _ => SystemType::CustomBuild,
            },
            8 => dst_row.cpu = src_data.to_string(),
            9 => dst_row.gpu = src_data.to_string(),
            10 => dst_row.ram = src_data.to_string(),
            11 => dst_row.warranty = src_data.to_string(),
            12 => {}
            _ => unreachable!(),
        }
        Ok(())
    }

    fn create_empty_decoded_row(&mut self) -> SystemInStoreData {
        SystemInStoreData::default()
    }
}