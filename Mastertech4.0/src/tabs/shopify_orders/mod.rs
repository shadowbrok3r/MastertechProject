//! Shopify (Xidax) order browser. Lists recent build-intake orders from the
//! XBM queue in an egui-data-table and pulls a selected order's customer +
//! line items into the TUR Sheet form. Self-contained: owns its async channel
//! and drains it in `ui`.

use crossbeam::channel::{Receiver, Sender, unbounded};
use database::orders::{OrderKey, OrderSummary, QcBackend, QcOrder, QcOrderItem};
use database::schema::prestashop_schema::{OrderRow, ServiceOrder};
use displays::tabs::TabId;
use displays::ui_tools::{
    controls::{
        self, BarKind, ButtonKind, IconBox, Metrics, SpinnerSize, TextButton, TextField, Tokens,
    },
    fonts, glass_card,
    icons::svg,
};
use eframe::egui::{self, CentralPanel, Id, RichText, Ui, Widget, scroll_area};
use egui_data_table::{DataTable, Renderer};

use crate::app_state::MastertechContext;

pub mod codec;
pub mod data;
pub mod row_viewer;

use data::{ShopifyLineItemRow, ShopifyOrderRow};
use row_viewer::{ShopifyLineItemRowViewer, ShopifyOrderRowViewer};

/// Width of the loaded-orders filter.
const FILTER_W: f32 = 180.0;
/// Width of the order lookup field.
const LOOKUP_W: f32 = 150.0;
/// Order title size in the detail panel.
const ORDER_TITLE_PT: f32 = 15.0;

enum ShopifyMsg {
    Recent(Result<Vec<OrderSummary>, String>),
    Detail(Result<Box<QcOrder>, String>),
}

/// Action bubbled up to the context, which owns the TUR form fields.
pub enum ShopifyOrderAction {
    PullIntoTur(Box<QcOrder>),
}

pub struct ShopifyOrdersTab {
    orders_table: DataTable<ShopifyOrderRow>,
    orders_viewer: ShopifyOrderRowViewer,
    items_table: DataTable<ShopifyLineItemRow>,
    items_viewer: ShopifyLineItemRowViewer,
    detail: Option<Box<QcOrder>>,
    detail_error: Option<String>,
    recent_error: Option<String>,
    recent_busy: bool,
    detail_busy: bool,
    recent_attempted: bool,
    limit: usize,
    loaded_count: usize,
    has_more: bool,
    lookup_input: String,
    lookup_error: Option<String>,
    tx: Sender<ShopifyMsg>,
    rx: Receiver<ShopifyMsg>,
    load_rx: Receiver<String>,
}

impl Default for ShopifyOrdersTab {
    fn default() -> Self {
        let (tx, rx) = unbounded();
        let (load_tx, load_rx) = unbounded();
        Self {
            orders_table: DataTable::new(),
            orders_viewer: ShopifyOrderRowViewer::new(load_tx),
            items_table: DataTable::new(),
            items_viewer: ShopifyLineItemRowViewer::default(),
            detail: None,
            detail_error: None,
            recent_error: None,
            recent_busy: false,
            detail_busy: false,
            recent_attempted: false,
            limit: 10,
            loaded_count: 0,
            has_more: false,
            lookup_input: String::new(),
            lookup_error: None,
            tx,
            rx,
            load_rx,
        }
    }
}

impl ShopifyOrdersTab {
    fn drain(&mut self, ctx: &egui::Context) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                ShopifyMsg::Recent(Ok(orders)) => {
                    self.recent_busy = false;
                    self.recent_error = None;
                    self.loaded_count = orders.len();
                    self.has_more = orders.len() >= self.limit;
                    let rows: Vec<ShopifyOrderRow> = orders.iter().map(row_from_summary).collect();
                    self.orders_table.replace(rows);
                }
                ShopifyMsg::Recent(Err(e)) => {
                    self.recent_busy = false;
                    self.recent_error = Some(e);
                }
                ShopifyMsg::Detail(Ok(order)) => {
                    self.detail_busy = false;
                    self.detail_error = None;
                    let rows: Vec<ShopifyLineItemRow> =
                        order.items.iter().map(row_from_item).collect();
                    self.items_table.replace(rows);
                    self.detail = Some(order);
                }
                ShopifyMsg::Detail(Err(e)) => {
                    self.detail_busy = false;
                    self.detail_error = Some(e);
                }
            }
        }
        while let Ok(lookup) = self.load_rx.try_recv() {
            if let Some(key) = OrderKey::parse(&lookup) {
                self.start_detail(ctx, key);
            }
        }
    }

    fn start_recent(&mut self, ctx: &egui::Context) {
        self.recent_busy = true;
        self.recent_attempted = true;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let limit = self.limit;
        tokio::spawn(async move {
            let result = QcBackend::shopify()
                .recent_orders(limit)
                .await
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(ShopifyMsg::Recent(result));
            ctx.request_repaint();
        });
    }

    fn start_detail(&mut self, ctx: &egui::Context, key: OrderKey) {
        self.detail_busy = true;
        self.detail_error = None;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        tokio::spawn(async move {
            let result = QcBackend::for_key(&key)
                .find_order(&key)
                .await
                .map(Box::new)
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(ShopifyMsg::Detail(result));
            ctx.request_repaint();
        });
    }

    /// Returns an action when the user opts to pull the loaded order into TUR.
    pub fn ui(&mut self, ui: &mut Ui) -> Option<ShopifyOrderAction> {
        let ctx = ui.ctx().clone();
        self.drain(&ctx);
        if !self.recent_attempted && !self.recent_busy {
            self.start_recent(&ctx);
        }

        let mut action = None;

        let t = Tokens::get(&ctx);
        let m = Metrics::get(&ctx);
        egui::Panel::top("shopify_orders_top")
            .resizable(false)
            .show(ui, |ui| {
                controls::bar(ui, BarKind::Tool, |ui| {
                    let refresh = TextButton::new("Refresh")
                        .icon(svg::ARROW_CLOCKWISE)
                        .enabled(!self.recent_busy)
                        .show(ui);
                    if refresh.clicked() {
                        self.limit = 10;
                        self.start_recent(&ctx);
                    }
                    let more = TextButton::new("Load +10 more")
                        .icon(svg::PLUS)
                        .enabled(self.has_more && !self.recent_busy)
                        .show(ui);
                    if more.clicked() {
                        self.limit += 10;
                        self.start_recent(&ctx);
                    }
                    if self.recent_busy || self.detail_busy {
                        controls::spinner(ui, SpinnerSize::Sm);
                    }
                    ui.add(svg::STOREFRONT.image(m.icon_sm).tint(t.text_dim));
                    ui.label(
                        RichText::new(format!(
                            "{} loaded · Order Placed / Ready to Build",
                            self.loaded_count
                        ))
                        .color(t.text_dim),
                    );
                });
                controls::bar(ui, BarKind::Tool, |ui| {
                    controls::search_field(
                        ui,
                        &mut self.orders_viewer.filter,
                        "Filter loaded orders",
                        FILTER_W,
                    );
                    controls::vline(ui, m.control_h);
                    controls::icon_label(ui, svg::RECEIPT, "Look up order");
                    let resp = TextField::new(&mut self.lookup_input)
                        .hint("#1032 / XBS-…")
                        .icon(svg::HASH)
                        .width(LOOKUP_W)
                        .id_salt("shopify_orders.lookup")
                        .show(ui);
                    let submit = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    let load = TextButton::new("Load")
                        .icon(svg::DOWNLOAD_SIMPLE)
                        .kind(ButtonKind::Primary)
                        .show(ui);
                    if (load.clicked() || submit) && !self.lookup_input.trim().is_empty() {
                        let raw = self.lookup_input.trim().to_string();
                        // The tab is Shopify-only; force the Shopify route for bare numbers.
                        let normalized =
                            if raw.starts_with('#') || raw.to_uppercase().starts_with("XBS-") {
                                raw
                            } else {
                                format!("#{raw}")
                            };
                        match OrderKey::parse(&normalized) {
                            Some(key) => {
                                self.lookup_error = None;
                                self.start_detail(&ctx, key);
                            }
                            None => {
                                self.lookup_error =
                                    Some("Enter an order # or XBS- serial.".to_string())
                            }
                        }
                    }
                    if let Some(e) = self.lookup_error.as_ref() {
                        controls::status_label(ui, svg::WARNING_CIRCLE, e, t.error);
                    }
                });
            });

        if self.detail.is_some() || self.detail_busy || self.detail_error.is_some() {
            egui::Panel::right(Id::new("shopify_order_detail"))
                .default_size(360.0)
                .resizable(true)
                .show(ui, |ui| {
                    action = self.ui_detail(ui);
                });
        }

        CentralPanel::default().show(ui, |ui| {
            if let Some(e) = self.recent_error.as_ref() {
                controls::status_label(ui, svg::WARNING_CIRCLE, e, t.error);
            }
            Renderer::new(&mut self.orders_table, &mut self.orders_viewer)
                .with_style_modify(|s| {
                    s.scroll_bar_visibility = scroll_area::ScrollBarVisibility::AlwaysVisible;
                    s.auto_shrink = [false, false].into();
                })
                .ui(ui);
        });

        action
    }

    fn ui_detail(&mut self, ui: &mut Ui) -> Option<ShopifyOrderAction> {
        let t = Tokens::get(ui.ctx());
        let m = Metrics::get(ui.ctx());
        let mut action = None;
        controls::bar(ui, BarKind::Header, |ui| {
            controls::icon_label(ui, svg::RECEIPT, "Order detail");
            if self.detail_busy {
                controls::spinner(ui, SpinnerSize::Sm);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if controls::icon_button(ui, svg::X, IconBox::Sm, false, "Close").clicked() {
                    self.detail = None;
                    self.detail_error = None;
                    self.items_table.clear();
                }
            });
        });
        if let Some(e) = self.detail_error.as_ref() {
            controls::status_label(ui, svg::WARNING_CIRCLE, e, t.error);
            return None;
        }
        let Some(order) = self.detail.as_ref() else {
            return None;
        };

        ui.add_space(m.gap.y);
        ui.label(
            RichText::new(format!("Order {}", order.reference))
                .font(fonts::semibold(ORDER_TITLE_PT)),
        );
        ui.horizontal_wrapped(|ui| {
            controls::badge(ui, order.kind.as_str(), t.secondary);
            controls::badge(ui, &order.status.name, t.primary);
        });
        if !order.customer_name.is_empty() {
            controls::icon_label(ui, svg::USER, &format!("Customer: {}", order.customer_name));
        }
        if !order.total_paid.is_empty() {
            controls::icon_label(
                ui,
                svg::CURRENCY_DOLLAR,
                &format!("Total ${}", order.total_paid),
            );
        }
        if let Some(serial) = order.build_serial.as_ref() {
            ui.horizontal(|ui| {
                ui.add(svg::BARCODE.image(m.icon_md).tint(t.icon));
                ui.label(RichText::new(serial).monospace());
            });
        }

        ui.add_space(m.gap.y);
        if TextButton::new("Pull into TUR Sheet")
            .icon(svg::CLIPBOARD_TEXT)
            .kind(ButtonKind::Primary)
            .prominent(true)
            .show(ui)
            .clicked()
        {
            action = Some(ShopifyOrderAction::PullIntoTur(order.clone()));
        }
        ui.add_space(m.gap.y);
        glass_card::card_header(
            ui,
            svg::PACKAGE,
            &format!("Line items ({})", order.items.len()),
            None,
        );

        Renderer::new(&mut self.items_table, &mut self.items_viewer)
            .with_style_modify(|s| {
                s.auto_shrink = [false, false].into();
            })
            .ui(ui);

        action
    }
}

fn row_from_summary(o: &OrderSummary) -> ShopifyOrderRow {
    ShopifyOrderRow {
        reference: if o.reference.is_empty() {
            o.id.clone()
        } else {
            o.reference.clone()
        },
        status: o.status.name.clone(),
        customer: o.customer_name.clone(),
        build: o.model.clone(),
        serials: format!("{}/{}", o.attached_serials, o.expected_serials),
        placed: short_date(o.created_at.as_deref()),
        lookup: o.lookup_input(),
    }
}

fn row_from_item(item: &QcOrderItem) -> ShopifyLineItemRow {
    ShopifyLineItemRow {
        name: item.name.clone(),
        reference: item.reference.clone(),
        quantity: format!("{:.0}", item.quantity),
        serials: if item.serials.is_empty() {
            "—".to_string()
        } else {
            item.serials.join(", ")
        },
    }
}

/// Date portion of an ISO-8601 timestamp (`2026-06-15T17:54:35Z` → `2026-06-15`).
fn short_date(iso: Option<&str>) -> String {
    iso.map(|s| s.split('T').next().unwrap_or(s).to_string())
        .unwrap_or_default()
}

impl MastertechContext {
    pub fn shopify_orders(&mut self, ui: &mut Ui) {
        if let Some(action) = self.shopify_orders_tab.ui(ui) {
            match action {
                ShopifyOrderAction::PullIntoTur(order) => {
                    self.apply_shopify_order_to_form(&order);
                    self.pending_tab_opens.push(TabId::TurSheet);
                    self.pending_activate_tab = Some(TabId::TurSheet);
                    log::info!(
                        "Pulled Shopify order {} into the TUR Sheet",
                        order.reference
                    );
                }
            }
        }
    }

    /// Map a Shopify [`QcOrder`] onto the TUR Sheet form fields.
    fn apply_shopify_order_to_form(&mut self, order: &QcOrder) {
        self.customer_data.name = order.customer_name.clone();
        self.ticket_data.service_number = order.reference.clone();
        self.ticket_data.ticket_total = order.total_paid.clone();
        self.ticket_data.doc_alias = "sales".to_string();
        self.order_rows = order
            .items
            .iter()
            .map(|item| OrderRow {
                id: item.row_id.clone(),
                id_order_config: String::new(),
                product_id: item.product_id.clone(),
                product_quantity: format!("{:.0}", item.quantity),
                product_name: item.name.clone(),
                product_price: item.unit_price.clone(),
                product_reference: item.reference.clone(),
            })
            .collect();
        self.service_details = order
            .service_info
            .as_ref()
            .map(|svc| {
                vec![ServiceOrder {
                    device_name: svc.device_name.clone(),
                    device_mfg: svc.device_mfg.clone(),
                    device_model: svc.device_model.clone(),
                    device_serial: svc.device_serial.clone(),
                    physical_damage: svc.physical_damage.clone(),
                    ..Default::default()
                }]
            })
            .unwrap_or_default();
    }
}
