use crossbeam::channel::Sender;
use eframe::egui::RichText;
use egui_data_table::{viewer::{RowCodec, TableColumnConfig}, RowViewer};

use super::codec::{LineItemCodec, OrderCodec};
use super::data::{ShopifyLineItemRow, ShopifyOrderRow};

/// Recent-orders table viewer. Clicking an order # sends its lookup key on
/// `load_tx`; the owning tab loads the full order detail.
pub struct ShopifyOrderRowViewer {
    pub filter: String,
    load_tx: Sender<String>,
}

impl ShopifyOrderRowViewer {
    pub fn new(load_tx: Sender<String>) -> Self {
        Self { filter: String::new(), load_tx }
    }
}

impl RowViewer<ShopifyOrderRow> for ShopifyOrderRowViewer {
    fn try_create_codec(&mut self, _: bool) -> Option<impl RowCodec<ShopifyOrderRow>> { Some(OrderCodec) }

    fn num_columns(&mut self) -> usize { 6 }

    fn column_name(&mut self, column: usize) -> std::borrow::Cow<'static, str> {
        [" Order", " Status", " Customer", " Build", " Serials", " Placed"][column].into()
    }

    fn is_editable_cell(&mut self, _: usize, _: usize, _: &ShopifyOrderRow) -> bool { false }

    fn is_sortable_column(&mut self, _: usize) -> bool { true }

    fn row_filter_hash(&mut self) -> &impl std::hash::Hash { &self.filter }

    fn filter_row(&mut self, row: &ShopifyOrderRow) -> bool {
        let f = self.filter.trim().to_lowercase();
        if f.is_empty() {
            return true;
        }
        row.reference.to_lowercase().contains(&f)
            || row.status.to_lowercase().contains(&f)
            || row.customer.to_lowercase().contains(&f)
            || row.build.to_lowercase().contains(&f)
    }

    fn show_cell_view(&mut self, ui: &mut eframe::egui::Ui, row: &ShopifyOrderRow, column: usize) {
        let style = ui.style_mut();
        style.interaction.multi_widget_text_select = false;
        style.interaction.selectable_labels = false;
        match column {
            0 => {
                if ui
                    .button(RichText::new(&row.reference).monospace())
                    .on_hover_text("Load this order")
                    .clicked()
                {
                    let _ = self.load_tx.try_send(row.lookup.clone());
                }
            }
            1 => { ui.label(&row.status); }
            2 => { ui.label(&row.customer); }
            3 => { ui.label(&row.build); }
            4 => { ui.label(&row.serials); }
            5 => { ui.label(&row.placed); }
            _ => {}
        }
    }

    fn show_cell_editor(
        &mut self,
        _ui: &mut eframe::egui::Ui,
        _row: &mut ShopifyOrderRow,
        _column: usize,
    ) -> Option<eframe::egui::Response> {
        None
    }

    fn on_cell_view_response(
        &mut self,
        _row: &ShopifyOrderRow,
        _column: usize,
        _resp: &eframe::egui::Response,
    ) -> Option<Box<ShopifyOrderRow>> {
        None
    }

    fn set_cell_value(&mut self, src: &ShopifyOrderRow, dst: &mut ShopifyOrderRow, _column: usize) {
        *dst = src.clone();
    }

    fn compare_cell(&self, l: &ShopifyOrderRow, r: &ShopifyOrderRow, column: usize) -> std::cmp::Ordering {
        match column {
            0 => l.reference.cmp(&r.reference),
            1 => l.status.cmp(&r.status),
            2 => l.customer.cmp(&r.customer),
            3 => l.build.cmp(&r.build),
            4 => l.serials.cmp(&r.serials),
            5 => l.placed.cmp(&r.placed),
            _ => std::cmp::Ordering::Equal,
        }
    }

    fn new_empty_row(&mut self) -> ShopifyOrderRow { ShopifyOrderRow::default() }

    fn column_render_config(&mut self, column: usize, _is_last_visible_column: bool) -> TableColumnConfig {
        let col = TableColumnConfig::auto();
        match column {
            0 => col.resizable(true).at_least(80.).at_most(110.),
            1 => col.resizable(true).at_least(120.).at_most(220.),
            2 => col.resizable(true).at_least(120.).at_most(240.),
            3 => col.resizable(true).at_least(120.).at_most(260.),
            4 => col.resizable(true).at_least(60.).at_most(80.),
            5 => col.resizable(true).at_least(90.).at_most(120.),
            _ => col,
        }
    }
}

/// Line-item detail table viewer (read-only).
#[derive(Default)]
pub struct ShopifyLineItemRowViewer {
    pub filter: String,
}

impl RowViewer<ShopifyLineItemRow> for ShopifyLineItemRowViewer {
    fn try_create_codec(&mut self, _: bool) -> Option<impl RowCodec<ShopifyLineItemRow>> { Some(LineItemCodec) }

    fn num_columns(&mut self) -> usize { 4 }

    fn column_name(&mut self, column: usize) -> std::borrow::Cow<'static, str> {
        [" Item", " Ref", " Qty", " Serials"][column].into()
    }

    fn is_editable_cell(&mut self, _: usize, _: usize, _: &ShopifyLineItemRow) -> bool { false }

    fn is_sortable_column(&mut self, _: usize) -> bool { true }

    fn row_filter_hash(&mut self) -> &impl std::hash::Hash { &self.filter }

    fn filter_row(&mut self, _row: &ShopifyLineItemRow) -> bool { true }

    fn show_cell_view(&mut self, ui: &mut eframe::egui::Ui, row: &ShopifyLineItemRow, column: usize) {
        let style = ui.style_mut();
        style.interaction.multi_widget_text_select = false;
        style.interaction.selectable_labels = false;
        match column {
            0 => { ui.label(&row.name); }
            1 => { ui.label(RichText::new(&row.reference).monospace()); }
            2 => { ui.label(&row.quantity); }
            3 => { ui.label(&row.serials); }
            _ => {}
        }
    }

    fn show_cell_editor(
        &mut self,
        _ui: &mut eframe::egui::Ui,
        _row: &mut ShopifyLineItemRow,
        _column: usize,
    ) -> Option<eframe::egui::Response> {
        None
    }

    fn on_cell_view_response(
        &mut self,
        _row: &ShopifyLineItemRow,
        _column: usize,
        _resp: &eframe::egui::Response,
    ) -> Option<Box<ShopifyLineItemRow>> {
        None
    }

    fn set_cell_value(&mut self, src: &ShopifyLineItemRow, dst: &mut ShopifyLineItemRow, _column: usize) {
        *dst = src.clone();
    }

    fn compare_cell(&self, l: &ShopifyLineItemRow, r: &ShopifyLineItemRow, column: usize) -> std::cmp::Ordering {
        match column {
            0 => l.name.cmp(&r.name),
            1 => l.reference.cmp(&r.reference),
            2 => l.quantity.cmp(&r.quantity),
            3 => l.serials.cmp(&r.serials),
            _ => std::cmp::Ordering::Equal,
        }
    }

    fn new_empty_row(&mut self) -> ShopifyLineItemRow { ShopifyLineItemRow::default() }

    fn column_render_config(&mut self, column: usize, _is_last_visible_column: bool) -> TableColumnConfig {
        let col = TableColumnConfig::auto();
        match column {
            0 => col.resizable(true).at_least(160.),
            1 => col.resizable(true).at_least(90.).at_most(160.),
            2 => col.resizable(true).at_least(40.).at_most(60.),
            3 => col.resizable(true).at_least(120.),
            _ => col,
        }
    }
}
