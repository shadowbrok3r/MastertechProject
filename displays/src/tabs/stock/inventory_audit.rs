//! Inventory audit lifecycle for the Store Inventory tab.
//!
//! An "audit" is a frozen list of serials that should be in a store on a
//! given day. The user imports a list (CSV or paste), we look each serial
//! up in Odoo to enrich it with item code / last move / price, persist the
//! whole thing as one `inventory_audit` row, then let the user mark each
//! serial as `found` while walking the floor with a barcode scanner.
//!
//! The flattened columns are what the UI renders; the `raw_lot` / `raw_move`
//! blobs let a future column be re-derived without re-hitting Odoo.

use anyhow::{anyhow, Result};
use crossbeam::channel::Sender;
use database::schema::{random_record_id, Datetime, RecordId, Store};
use database::SurrealValue;
use database::{db, ODOO_API_KEY, ODOO_JSONRPC_URL};
use eframe::egui::{Button, Color32, Link, OpenUrl, Response, RichText, Spinner, Ui, Widget};
use egui_data_table::{
    viewer::{DecodeErrorBehavior, RowCodec},
    RowViewer,
};
use egui_extras::Column as TableColumnConfig;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::tabs::stock::everest_lookup::{OdooMoveLine, OdooSerialHistory};

pub const INVENTORY_AUDIT_TABLE: &str = "inventory_audit";

/* ------------------------------------- Data types ------------------------------------- */

/// One imported serial within an audit. Mirrors the schema in
/// `database/schema/inventory_audit.surql`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, SurrealValue)]
pub struct AuditSerialRow {
    pub serial: String,
    #[serde(default)]
    pub item_code: Option<String>,
    #[serde(default)]
    pub product_name: Option<String>,
    #[serde(default)]
    pub last_location: Option<String>,
    #[serde(default)]
    pub last_reference: Option<String>,
    #[serde(default)]
    pub last_move_date: Option<String>,
    #[serde(default)]
    pub std_price: Option<f64>,
    #[serde(default)]
    pub list_price: Option<f64>,
    #[serde(default)]
    pub found: bool,
    /// Raw `stock.lot` row from Odoo. Kept so we can re-derive new
    /// columns later without re-hitting the API.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_lot: Option<Value>,
    /// Raw `stock.move.line` results for this serial (newest first,
    /// trimmed to a small recent window).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_move: Option<Value>,
}

/// What we send to SurrealDB on a CREATE. Mirrors the audit schema
/// except for `id` / `created_at` (filled server-side via `time::now()`).
#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
struct NewAuditPayload {
    store: String,
    store_id: i32,
    created_by: Option<RecordId>,
    label: String,
    serials: Vec<AuditSerialRow>,
}

/// Lightweight metadata used to populate the audit-source combobox.
#[derive(Debug, Clone)]
pub struct InventoryAuditMeta {
    pub id: RecordId,
    /// Human-readable label, defaults to "YYYY-MM-DD HH:MM".
    pub label: String,
    pub serial_count: usize,
}

/// Whether the Store Inventory table is currently rendering the live Odoo
/// pull or a saved audit snapshot.
#[derive(Debug, Clone, Default)]
pub enum InventoryView {
    #[default]
    Live,
    Audit(RecordId),
}

/* --------------------------------- Floating windows ---------------------------------- */

/// One floating `egui::Window` showing the Odoo move history for a serial.
/// Multiple `HistoryWindow`s can be open at once; each owns its own
/// `egui_data_table::DataTable<SerialHistoryRow>`.
pub struct HistoryWindow {
    pub serial: String,
    pub product_name: Option<String>,
    pub open: bool,
    pub loading: bool,
    pub error: Option<String>,
    pub table: egui_data_table::DataTable<SerialHistoryRow>,
    pub viewer: SerialHistoryViewer,
}

impl HistoryWindow {
    pub fn loading(serial: String) -> Self {
        Self {
            serial,
            product_name: None,
            open: true,
            loading: true,
            error: None,
            table: egui_data_table::DataTable::default(),
            viewer: SerialHistoryViewer::default(),
        }
    }

    pub fn populate_from_history(&mut self, history: OdooSerialHistory) {
        self.loading = false;
        self.product_name = history.product_name;
        self.error = history.error;
        let rows: Vec<SerialHistoryRow> = history
            .moves
            .iter()
            .map(SerialHistoryRow::from_move)
            .collect();
        self.table.replace(rows);
    }
}

/// Flat row for the per-serial history table.
#[derive(Default, Serialize, Clone, Debug)]
pub struct SerialHistoryRow {
    pub date: String,
    pub state: String,
    pub from: String,
    pub to: String,
    pub reference: String,
    pub qty: f64,
}

impl SerialHistoryRow {
    pub fn from_move(m: &OdooMoveLine) -> Self {
        let date = m
            .date
            .clone()
            .unwrap_or_default()
            .split('.')
            .next()
            .unwrap_or("")
            .to_string();
        let state = m.state.clone().unwrap_or_default();
        let reference = {
            let r = m.reference_str();
            if r.is_empty() { m.picking_name() } else { r }
        };
        Self {
            date,
            state,
            from: m.location_name(),
            to: m.dest_name(),
            reference,
            qty: m.qty_done.unwrap_or(0.0),
        }
    }
}

/// "2025-12-22 18:09:29" → "12/22/2025". Falls back to the raw string
/// if the input doesn't parse as `YYYY-MM-DD ...`.
pub fn format_date_short(odoo: &str) -> String {
    let date_part = odoo.split_whitespace().next().unwrap_or(odoo);
    let segs: Vec<&str> = date_part.split('-').collect();
    if segs.len() == 3 && segs[0].len() == 4 {
        format!("{}/{}/{}", segs[1], segs[2], segs[0])
    } else {
        date_part.to_string()
    }
}

/// "2025-12-22 18:09:29" → "12/22/2025 18:09:29". Date-only inputs
/// round-trip identically to `format_date_short`.
pub fn format_date_long(odoo: &str) -> String {
    let mut parts = odoo.splitn(2, char::is_whitespace);
    let date_part = parts.next().unwrap_or(odoo);
    let time_part = parts.next().unwrap_or("");
    let short = format_date_short(date_part);
    if time_part.is_empty() {
        short
    } else {
        format!("{short} {time_part}")
    }
}

#[derive(Default, Serialize)]
pub struct SerialHistoryViewer {
    pub filter: String,
    /// Raw Odoo timestamps the user has clicked to expand. Cells in
    /// `expanded_dates` render with the time appended; others render as
    /// MM/DD/YYYY only.
    #[serde(skip)]
    pub expanded_dates: std::collections::HashSet<String>,
}

impl RowViewer<SerialHistoryRow> for SerialHistoryViewer {
    fn try_create_codec(&mut self, _: bool) -> Option<impl RowCodec<SerialHistoryRow>> {
        Some(SerialHistoryCodec)
    }

    fn num_columns(&mut self) -> usize { 6 }

    fn column_name(&mut self, column: usize) -> std::borrow::Cow<'static, str> {
        ["Date", "State", "From", "To", "Reference", "Qty"][column].into()
    }

    fn is_sortable_column(&mut self, column: usize) -> bool { column < 6 }

    fn is_editable_cell(&mut self, _: usize, _: usize, _: &SerialHistoryRow) -> bool { false }

    fn row_filter_hash(&mut self) -> &impl std::hash::Hash { &self.filter }

    fn filter_row(&mut self, row: &SerialHistoryRow) -> bool {
        let filter = self.filter.to_uppercase();
        if filter.is_empty() { return true; }
        row.from.to_uppercase().contains(&filter)
            || row.to.to_uppercase().contains(&filter)
            || row.reference.to_uppercase().contains(&filter)
            || row.state.to_uppercase().contains(&filter)
    }

    fn show_cell_view(&mut self, ui: &mut Ui, row: &SerialHistoryRow, column: usize) {
        let style = ui.style_mut();
        style.interaction.multi_widget_text_select = false;
        style.interaction.selectable_labels = false;
        match column {
            0 => {
                let expanded = self.expanded_dates.contains(&row.date);
                let display = if expanded {
                    format_date_long(&row.date)
                } else {
                    format_date_short(&row.date)
                };
                let res = Link::new(display)
                    .ui(ui)
                    .on_hover_text("Click to toggle time");
                if res.clicked() {
                    if expanded {
                        self.expanded_dates.remove(&row.date);
                    } else {
                        self.expanded_dates.insert(row.date.clone());
                    }
                }
            }
            1 => {
                let color = match row.state.as_str() {
                    "done" => Color32::LIGHT_GREEN,
                    "cancel" => Color32::from_rgb(200, 100, 100),
                    "draft" => Color32::GRAY,
                    _ => Color32::LIGHT_GRAY,
                };
                ui.label(RichText::new(&row.state).color(color));
            }
            2 => { ui.label(&row.from); }
            3 => { ui.label(&row.to); }
            4 => { ui.label(&row.reference); }
            5 => { ui.label(format!("{:.0}", row.qty)); }
            _ => unreachable!(),
        }
    }

    fn show_cell_editor(&mut self, ui: &mut Ui, row: &mut SerialHistoryRow, column: usize) -> Option<Response> {
        ui.vertical_centered_justified(|ui| {
            match column {
                0 => {
                    let expanded = self.expanded_dates.contains(&row.date);
                    let display = if expanded {
                        format_date_long(&row.date)
                    } else {
                        format_date_short(&row.date)
                    };
                    ui.label(display)
                }
                1 => ui.label(&row.state),
                2 => ui.label(&row.from),
                3 => ui.label(&row.to),
                4 => ui.label(&row.reference),
                5 => ui.label(format!("{}", row.qty)),
                _ => unreachable!(),
            }.into()
        }).inner
    }

    fn set_cell_value(&mut self, src: &SerialHistoryRow, dst: &mut SerialHistoryRow, column: usize) {
        match column {
            0 => dst.date = src.date.clone(),
            1 => dst.state = src.state.clone(),
            2 => dst.from = src.from.clone(),
            3 => dst.to = src.to.clone(),
            4 => dst.reference = src.reference.clone(),
            5 => dst.qty = src.qty,
            _ => unreachable!(),
        }
    }

    fn compare_cell(&self, l: &SerialHistoryRow, r: &SerialHistoryRow, column: usize) -> std::cmp::Ordering {
        match column {
            0 => l.date.cmp(&r.date),
            1 => l.state.cmp(&r.state),
            2 => l.from.cmp(&r.from),
            3 => l.to.cmp(&r.to),
            4 => l.reference.cmp(&r.reference),
            5 => l.qty.partial_cmp(&r.qty).unwrap_or(std::cmp::Ordering::Equal),
            _ => unreachable!(),
        }
    }

    fn new_empty_row(&mut self) -> SerialHistoryRow { SerialHistoryRow::default() }

    fn column_render_config(&mut self, column: usize, _: bool) -> TableColumnConfig {
        let c = TableColumnConfig::auto();
        match column {
            // Date column needs to fit both "12/22/2025" (collapsed) and
            // "12/22/2025 18:09:29" (expanded after a click).
            0 => c.resizable(true).at_least(90.).at_most(200.),
            1 => c.resizable(true).at_least(70.).at_most(90.),
            2 => c.resizable(true).at_least(130.).at_most(180.),
            3 => c.resizable(true).at_least(130.).at_most(180.),
            4 => c.resizable(true).at_least(140.).at_most(260.),
            5 => c.resizable(true).at_least(45.).at_most(70.),
            _ => c,
        }
    }
}

struct SerialHistoryCodec;

impl RowCodec<SerialHistoryRow> for SerialHistoryCodec {
    type DeserializeError = &'static str;

    fn encode_column(&mut self, src: &SerialHistoryRow, column: usize, dst: &mut String) {
        match column {
            0 => dst.push_str(&src.date),
            1 => dst.push_str(&src.state),
            2 => dst.push_str(&src.from),
            3 => dst.push_str(&src.to),
            4 => dst.push_str(&src.reference),
            5 => dst.push_str(&format!("{}", src.qty)),
            _ => unreachable!(),
        }
    }

    fn decode_column(&mut self, src: &str, column: usize, dst: &mut SerialHistoryRow) -> Result<(), DecodeErrorBehavior> {
        match column {
            0 => dst.date = src.to_string(),
            1 => dst.state = src.to_string(),
            2 => dst.from = src.to_string(),
            3 => dst.to = src.to_string(),
            4 => dst.reference = src.to_string(),
            5 => dst.qty = src.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            _ => unreachable!(),
        }
        Ok(())
    }

    fn create_empty_decoded_row(&mut self) -> SerialHistoryRow { SerialHistoryRow::default() }
}

/// Render every open history window. Closes are handled via the `open`
/// field of each window; closed windows are pruned after the loop.
pub fn render_history_windows(
    ctx: &eframe::egui::Context,
    windows: &mut Vec<HistoryWindow>,
) {
    for win in windows.iter_mut() {
        let mut open = win.open;
        let title = if let Some(product) = &win.product_name {
            format!("History — {} ({})", win.serial, product)
        } else {
            format!("History — {}", win.serial)
        };
        eframe::egui::Window::new(title)
            .id(eframe::egui::Id::new(("inv_hist", win.serial.clone())))
            .resizable(true)
            .default_size([760., 360.])
            .open(&mut open)
            .show(ctx, |ui| {
                if win.loading {
                    ui.vertical_centered(|ui| {
                        ui.add_space(20.);
                        Spinner::new().size(28.).color(Color32::from_rgb(150, 10, 150)).ui(ui);
                        ui.label("Fetching Odoo movements...");
                    });
                    return;
                }
                if let Some(err) = &win.error {
                    ui.colored_label(ui.global_style().visuals.error_fg_color, err);
                    return;
                }
                if win.table.len() < 1 {
                    ui.label(RichText::new("No movements found.").color(Color32::GRAY));
                    return;
                }
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Filter:").color(Color32::GRAY));
                    eframe::egui::TextEdit::singleline(&mut win.viewer.filter)
                        .desired_width(220.)
                        .ui(ui);
                    if Button::new("Open in Odoo").ui(ui).clicked() {
                        // Best-effort: open the lot list filtered by serial.
                        let url = format!(
                            "https://odoo.pclaptops.com/odoo/inventory/lots?search={}",
                            win.serial
                        );
                        ui.ctx().open_url(OpenUrl::new_tab(url));
                    }
                });
                ui.add_space(2.);
                egui_data_table::Renderer::new(&mut win.table, &mut win.viewer)
                    .with_style_modify(|s| {
                        s.scroll_bar_visibility =
                            eframe::egui::scroll_area::ScrollBarVisibility::AlwaysVisible;
                        s.single_click_edit_mode = true;
                        s.auto_shrink = [false, false].into();
                    })
                    .ui(ui);
            });
        win.open = open;
    }
    windows.retain(|w| w.open);
}

/* ----------------------------------- Async ops ----------------------------------- */

/// Replicates `Inventory/src/main.rs`: for each serial, run `stock.lot
/// search_read` then `stock.move.line search_read`, returning a fully
/// populated `AuditSerialRow` (including the raw blobs).
///
/// `extra_stock_prices` is the in-memory map of item-code → (std, list)
/// price that the Company Stock tab has already pulled. Saves us a
/// per-serial `product.template` lookup for products we've seen.
pub async fn lookup_serials_in_odoo(
    serials: Vec<String>,
    extra_stock_prices: std::collections::HashMap<String, (f64, f64)>,
    tx: Sender<Vec<AuditSerialRow>>,
) -> Result<()> {
    let client = Client::builder().build()?;
    let mut rows = Vec::with_capacity(serials.len());

    for serial in serials.into_iter() {
        let serial = serial.trim().to_string();
        if serial.is_empty() {
            continue;
        }
        let mut row = AuditSerialRow {
            serial: serial.clone(),
            ..Default::default()
        };

        match fetch_stock_lot(&client, &serial).await {
            Ok(Some(lot_blob)) => {
                row.item_code = extract_item_code(&lot_blob);
                row.product_name = lot_blob
                    .get("product_id")
                    .and_then(|v| v.as_array())
                    .and_then(|a| a.get(1))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                if let Some(item_code) = row.item_code.as_ref() {
                    if let Some((std_price, list_price)) = extra_stock_prices.get(item_code) {
                        row.std_price = Some(*std_price);
                        row.list_price = Some(*list_price);
                    }
                }

                let lot_id = lot_blob.get("id").and_then(|v| v.as_i64());
                row.raw_lot = Some(lot_blob);

                if let Some(lot_id) = lot_id {
                    match fetch_serial_move_lines(&client, lot_id).await {
                        Ok(moves) => {
                            if let Some(first) = moves.as_array().and_then(|a| a.first()) {
                                row.last_move_date = first
                                    .get("date")
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string());
                                row.last_reference = first
                                    .get("reference")
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string())
                                    .or_else(|| {
                                        first
                                            .get("picking_id")
                                            .and_then(|v| v.as_array())
                                            .and_then(|a| a.get(1))
                                            .and_then(|v| v.as_str())
                                            .map(|s| s.to_string())
                                    });
                                row.last_location = first
                                    .get("location_dest_id")
                                    .and_then(|v| v.as_array())
                                    .and_then(|a| a.get(1))
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string());
                            }
                            row.raw_move = Some(moves);
                        }
                        Err(e) => {
                            log::warn!("move.line lookup failed for {serial}: {e:?}");
                        }
                    }
                }
            }
            Ok(None) => {
                log::info!("No stock.lot found in Odoo for serial {serial}");
            }
            Err(e) => {
                log::warn!("stock.lot lookup failed for {serial}: {e:?}");
            }
        }

        rows.push(row);
    }

    tx.try_send(rows)
        .map_err(|e| anyhow!("send audit lookup result: {e:?}"))?;
    Ok(())
}

/// Bare `stock.lot search_read`, returning the full first JSON object so
/// we can keep it as the `raw_lot` blob.
async fn fetch_stock_lot(client: &Client, serial: &str) -> Result<Option<Value>> {
    let payload = json!({
        "jsonrpc": "2.0",
        "method": "call",
        "id": 1,
        "params": {
            "service": "object",
            "method": "execute_kw",
            "args": [
                "pcl_live", 374, ODOO_API_KEY,
                "stock.lot", "search_read",
                [[ ["name", "=", serial] ]],
                { "limit": 1 }
            ]
        }
    });
    let resp = client.post(ODOO_JSONRPC_URL).json(&payload).send().await?;
    let v: Value = resp.json().await?;
    let arr = v.get("result").and_then(|r| r.as_array()).cloned().unwrap_or_default();
    Ok(arr.into_iter().next())
}

async fn fetch_serial_move_lines(client: &Client, lot_id: i64) -> Result<Value> {
    let payload = json!({
        "jsonrpc": "2.0",
        "method": "call",
        "id": 2,
        "params": {
            "service": "object",
            "method": "execute_kw",
            "args": [
                "pcl_live", 374, ODOO_API_KEY,
                "stock.move.line", "search_read",
                [[ ["lot_id", "=", lot_id] ]],
                {
                    "fields": [
                        "id", "date", "state", "qty_done", "reserved_qty",
                        "reference", "location_id", "location_dest_id",
                        "picking_id", "product_id"
                    ],
                    "limit": 10,
                    "order": "date desc"
                }
            ]
        }
    });
    let resp = client.post(ODOO_JSONRPC_URL).json(&payload).send().await?;
    let v: Value = resp.json().await?;
    Ok(v.get("result").cloned().unwrap_or(Value::Array(Vec::new())))
}

/// Extract the bracket prefix from an Odoo product display name, e.g.
/// `"[M.2/M371/1TB] XIDAX PERFORMANCE NVME M.2 SSD"` → `"[M.2/M371/1TB]"`.
/// Falls back to the bare product_id string if no brackets.
fn extract_item_code(lot_blob: &Value) -> Option<String> {
    let product = lot_blob.get("product_id").and_then(|v| v.as_array())?;
    let display = product.get(1)?.as_str()?;
    if let Some(close) = display.find(']') {
        Some(format!("[{}]", &display[1..close]))
    } else {
        Some(display.to_string())
    }
}

/// Persist a freshly-imported audit to SurrealDB and ship back its
/// metadata so the UI can swap into the new view.
pub async fn save_audit(
    store: Store,
    created_by: Option<RecordId>,
    rows: Vec<AuditSerialRow>,
    tx: Sender<(InventoryAuditMeta, Vec<AuditSerialRow>)>,
) -> Result<()> {
    let id = random_record_id(INVENTORY_AUDIT_TABLE);
    let store_tag = store.as_str().to_string();
    let store_id = store.into_odoo_store_id();
    let now = chrono::Utc::now();
    let label = now.format("%Y-%m-%d %H:%M").to_string();
    let serial_count = rows.len();

    let payload = NewAuditPayload {
        store: store_tag,
        store_id,
        created_by,
        label: label.clone(),
        serials: rows.clone(),
    };

    let _: Option<NewAuditPayload> = db().create(id.clone()).content(payload).await?;

    let meta = InventoryAuditMeta {
        id,
        label: format!("{label} ({serial_count} serials)"),
        serial_count,
    };
    tx.try_send((meta, rows))
        .map_err(|e| anyhow!("send save_audit: {e:?}"))?;
    Ok(())
}

/// Lightweight projection for the audit-selection combobox.
///
/// `created_at` is the SurrealDB `datetime` type — deserializing it as
/// `Option<String>` silently fails (`take(0)` returns Err which we then
/// swallowed via `unwrap_or_default`), which is why the combobox was
/// empty after a restart. Using the native `Datetime` round-trips
/// cleanly.
#[derive(Debug, Deserialize, Serialize, SurrealValue)]
struct AuditListProjection {
    id: RecordId,
    label: Option<String>,
    #[serde(default)]
    serial_count: i64,
    created_at: Option<Datetime>,
}

pub async fn list_audits(
    store_id: i32,
    tx: Sender<Vec<InventoryAuditMeta>>,
) -> Result<()> {
    let mut response = db()
        .query(
            "SELECT id, label, created_at, array::len(serials) AS serial_count \
             FROM inventory_audit \
             WHERE store_id = $store \
             ORDER BY created_at DESC \
             LIMIT 100",
        )
        .bind(("store", store_id))
        .await?;
    // Don't swallow deserialization errors here — a silent unwrap_or_default
    // hides Datetime/RecordId mismatches as "no audits exist".
    let rows: Vec<AuditListProjection> = response.take(0)?;

    let metas: Vec<InventoryAuditMeta> = rows
        .into_iter()
        .map(|r| {
            let count = r.serial_count.max(0) as usize;
            let label = r.label.clone().unwrap_or_else(|| {
                r.created_at
                    .as_ref()
                    .map(format_datetime)
                    .unwrap_or_else(|| "audit".to_string())
            });
            InventoryAuditMeta {
                id: r.id,
                label: format!("{label} ({count} serials)"),
                serial_count: count,
            }
        })
        .collect();

    tx.try_send(metas)
        .map_err(|e| anyhow!("send list_audits: {e:?}"))?;
    Ok(())
}

/// `Datetime` → "YYYY-MM-DD HH:MM" without pulling in the surrealdb
/// formatting feature. Round-trips through chrono::DateTime<Utc>.
fn format_datetime(dt: &Datetime) -> String {
    let cd: chrono::DateTime<chrono::Utc> = dt.clone().into();
    cd.format("%Y-%m-%d %H:%M").to_string()
}

/// Load a single audit's full serial list.
///
/// Uses `db().select(id)` instead of a SELECT projection so we don't
/// have to mirror every field in a Rust struct — and so a single
/// type-mismatched field can't silently null out the whole row (which
/// is what was producing the `load_audit: audit not found` error).
pub async fn load_audit(
    id: RecordId,
    tx: Sender<(InventoryAuditMeta, Vec<AuditSerialRow>)>,
) -> Result<()> {
    #[derive(Debug, Deserialize, Serialize, SurrealValue)]
    struct AuditFullRow {
        id: RecordId,
        #[serde(default)]
        store: Option<String>,
        #[serde(default)]
        store_id: Option<i64>,
        #[serde(default)]
        created_by: Option<RecordId>,
        #[serde(default)]
        created_at: Option<Datetime>,
        #[serde(default)]
        label: Option<String>,
        #[serde(default)]
        serials: Vec<AuditSerialRow>,
    }

    let row: Option<AuditFullRow> = db().select(id.clone()).await?;
    let row = row.ok_or_else(|| anyhow!("load_audit: audit not found"))?;

    let serial_count = row.serials.len();
    let label = row.label.clone().unwrap_or_else(|| {
        row.created_at
            .as_ref()
            .map(format_datetime)
            .unwrap_or_else(|| "audit".to_string())
    });

    let meta = InventoryAuditMeta {
        id: row.id,
        label: format!("{label} ({serial_count} serials)"),
        serial_count,
    };
    tx.try_send((meta, row.serials))
        .map_err(|e| anyhow!("send load_audit: {e:?}"))?;
    Ok(())
}

/// Flip the `found` flag for one serial inside an audit. We rewrite only
/// the matching element via SurrealDB's array-filter update syntax so
/// concurrent scans on the same audit don't clobber each other.
pub async fn mark_found(id: RecordId, serial: String, found: bool) -> Result<()> {
    // SurrealDB supports `serials[WHERE serial = $s].found = $v` for
    // targeted array mutation.
    let _ = db()
        .query(
            "UPDATE $id SET serials[WHERE serial = $serial].found = $found",
        )
        .bind(("id", id))
        .bind(("serial", serial))
        .bind(("found", found))
        .await?;
    Ok(())
}
