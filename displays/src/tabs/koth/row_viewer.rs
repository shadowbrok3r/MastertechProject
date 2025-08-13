use crate::tabs::koth::data::KothTableData;
use crate::tabs::task_audit::row_viewer::BASE_URL;
use eframe::egui::{Color32, Hyperlink, RichText};
use egui_data_table::{viewer::TableColumnConfig, RowViewer};

#[derive(Default, serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct KothRowViewer {
    pub filter: String,
    pub date_label: String,
}

impl RowViewer<KothTableData> for KothRowViewer {
    fn num_columns(&mut self) -> usize {
        9
    }

    fn column_name(&mut self, column: usize) -> std::borrow::Cow<'static, str> {
        match column {
            0 => " #".into(),
            1 => " ID".into(),
            2 => {
                let lbl = if self.date_label.is_empty() {
                    " Date Updated".to_string()
                } else {
                    self.date_label.clone()
                };
                std::borrow::Cow::Owned(lbl)
            }
            3 => " Order State".into(),
            4 => " Product".into(),
            5 => " Payment Type".into(),
            6 => " Warranty".into(),
            7 => " Total Paid".into(),
            8 => " Total Without Tax".into(),
            _ => "".into(),
        }
    }

    fn is_sortable_column(&mut self, column: usize) -> bool {
        column != 0
    }

    fn row_filter_hash(&mut self) -> &impl std::hash::Hash {
        &self.filter
    }

    fn filter_row(&mut self, row: &KothTableData) -> bool {
        let f = self.filter.trim().to_lowercase();
        if f.is_empty() {
            return true;
        }
        row.order_id.to_lowercase().contains(&f)
            || row.product.to_lowercase().contains(&f)
            || row.order_state.to_lowercase().contains(&f)
            || row.payment.to_lowercase().contains(&f)
            || row.warranty.to_lowercase().contains(&f)
    }

    fn show_cell_view(&mut self, ui: &mut eframe::egui::Ui, row: &KothTableData, column: usize) {
    match column {
            0 => {
                ui.label(row.idx.to_string());
            }
            1 => {
                ui.add(
                    Hyperlink::from_label_and_url(
                        RichText::new(row.order_id.clone())
                            .underline()
                            .strong()
                            .color(Color32::LIGHT_RED),
                        format!("{BASE_URL}{}", row.order_id),
                    )
                    .open_in_new_tab(true),
                );
            }
            2 => {
                ui.label(&row.date);
            }
            3 => {
                ui.label(&row.order_state);
            }
            4 => {
                ui.label(&row.product);
            }
            5 => {
                ui.label(&row.payment);
            }
            6 => {
                ui.label(&row.warranty);
            }
            7 => {
                ui.label(format!("$ {:.2}", row.total_paid));
            }
            8 => {
                ui.label(format!("$ {:.2}", row.total_without_tax));
            }
            _ => {}
    };
    }

    fn show_cell_editor(
        &mut self,
        _ui: &mut eframe::egui::Ui,
        _row: &mut KothTableData,
        _column: usize,
    ) -> Option<eframe::egui::Response> {
        None
    }

    fn set_cell_value(&mut self, src: &KothTableData, dst: &mut KothTableData, _column: usize) {
        *dst = src.clone();
    }

    fn compare_cell(
        &self,
        l: &KothTableData,
        r: &KothTableData,
        column: usize,
    ) -> std::cmp::Ordering {
        use std::cmp::Ordering::*;
        match column {
            0 => l.idx.cmp(&r.idx),
            1 => l.order_id.cmp(&r.order_id),
            2 => l.date.cmp(&r.date),
            3 => l.order_state.cmp(&r.order_state),
            4 => l.product.cmp(&r.product),
            5 => l.payment.cmp(&r.payment),
            6 => l.warranty.cmp(&r.warranty),
            7 => l.total_paid.partial_cmp(&r.total_paid).unwrap_or(Equal),
            8 => l.total_without_tax.partial_cmp(&r.total_without_tax).unwrap_or(Equal),
            _ => Equal,
        }
    }

    fn new_empty_row(&mut self) -> KothTableData {
        KothTableData::default()
    }

    fn column_render_config(&mut self, column: usize, _is_last_visible_column: bool) -> TableColumnConfig {
        let col = TableColumnConfig::auto();
        match column {
            0 => col.resizable(false).at_least(24.).at_most(32.),
            1 => col.resizable(true).at_least(90.).at_most(120.),
            2 => col.resizable(true).at_least(130.).at_most(160.),
            3 => col.resizable(true).at_least(120.).at_most(150.),
            4 => col.resizable(true).at_least(160.).at_most(260.),
            5 => col.resizable(true).at_least(120.).at_most(160.),
            6 => col.resizable(true).at_least(120.).at_most(150.),
            7 => col.resizable(true).at_least(110.).at_most(130.),
            8 => col.resizable(true).at_least(150.).at_most(170.),
            _ => col,
        }
    }
}