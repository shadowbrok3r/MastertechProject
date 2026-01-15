use egui_data_table::{viewer::TableColumnConfig, RowViewer};
use crate::tabs::koth::data::AllEmployeesTableData;
use eframe::egui::Layout;

#[derive(Default, serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct AllEmployeesRowViewer {
    pub filter: String,
}

impl RowViewer<AllEmployeesTableData> for AllEmployeesRowViewer {
    fn num_columns(&mut self) -> usize { 7 }

    fn column_name(&mut self, column: usize) -> std::borrow::Cow<'static, str> {
        match column {
            0 => " Employee Name".into(),
            1 => " Total Sales / Total Orders".into(),
            2 => " Laptops / Desktops".into(),
            3 => " Finance ratio".into(),
            4 => " Warranty Ratio".into(),
            5 => " Revenue $".into(),
            6 => " Spiffs $".into(),
            _ => "".into(),
        }
    }

    fn is_sortable_column(&mut self, _column: usize) -> bool { true }

    fn row_filter_hash(&mut self) -> &impl std::hash::Hash { &self.filter }

    fn filter_row(&mut self, row: &AllEmployeesTableData) -> bool {
        let f = self.filter.trim().to_lowercase();
        if f.is_empty() { return true; }
        row.employee_name.to_lowercase().contains(&f)
    }

    fn show_cell_view(&mut self, ui: &mut eframe::egui::Ui, row: &AllEmployeesTableData, column: usize) {
        let style = ui.style_mut();
        style.interaction.multi_widget_text_select = false;
        style.interaction.selectable_labels = false;
        match column {
            0 => { ui.label(&row.employee_name); }
            1 => { ui.vertical_centered(|ui| ui.label(format!("{} / {}", row.total_sales, row.total_orders))); }
            2 => { ui.vertical_centered(|ui| ui.label(format!("{} / {}", row.laptops, row.desktops))); }
            3 => { ui.colored_label(ui.style().visuals.error_fg_color, format!("{:.2}%", row.finance_ratio)); }
            4 => {
                let ratio = if row.total_sales > 0 { (row.warranties as f64 / row.total_sales as f64) * 100.0 } else { 0.0 };
                ui.horizontal_top(|ui| {
                    ui.colored_label(ui.style().visuals.error_fg_color, format!(" {} / {}", row.warranties, row.total_sales));
                    ui.with_layout(Layout::right_to_left(eframe::egui::Align::Center), |ui| {
                        ui.label(format!("({ratio:.2}%)"))
                    });
                });
            }
            5 => { ui.colored_label(ui.style().visuals.warn_fg_color, format!("$ {:.2}", row.revenue)); }
            6 => { ui.label(format!("$ {:.2}", row.spiffs)); }
            _ => {}
        }
    }

    fn show_cell_editor(&mut self, _ui: &mut eframe::egui::Ui, _row: &mut AllEmployeesTableData, _column: usize) -> Option<eframe::egui::Response> { None }

    fn set_cell_value(&mut self, src: &AllEmployeesTableData, dst: &mut AllEmployeesTableData, _column: usize) { *dst = src.clone(); }

    fn compare_cell(&self, l: &AllEmployeesTableData, r: &AllEmployeesTableData, column: usize) -> std::cmp::Ordering {
        use std::cmp::Ordering::*;
        match column {
            0 => l.employee_name.cmp(&r.employee_name),
            1 => l.total_sales.cmp(&r.total_sales).then(l.total_orders.cmp(&r.total_orders)),
            2 => l.laptops.cmp(&r.laptops).then(l.desktops.cmp(&r.desktops)),
            3 => l.finance_ratio.partial_cmp(&r.finance_ratio).unwrap_or(Equal),
            4 => (l.warranties as f64).partial_cmp(&(r.warranties as f64)).unwrap_or(Equal),
            5 => l.revenue.partial_cmp(&r.revenue).unwrap_or(Equal),
            6 => l.spiffs.partial_cmp(&r.spiffs).unwrap_or(Equal),
            _ => Equal,
        }
    }

    fn new_empty_row(&mut self) -> AllEmployeesTableData { AllEmployeesTableData::default() }

    fn column_render_config(&mut self, column: usize, _is_last_visible_column: bool) -> TableColumnConfig {
        let col = TableColumnConfig::auto();
        match column {
            0 => col.resizable(true).at_least(160.).at_most(220.),
            1 => col.resizable(true).at_least(170.).at_most(200.),
            2 => col.resizable(true).at_least(150.).at_most(170.),
            3 => col.resizable(true).at_least(130.).at_most(150.),
            4 => col.resizable(true).at_least(170.).at_most(210.),
            5 => col.resizable(true).at_least(120.).at_most(140.),
            6 => col.resizable(true).at_least(110.).at_most(130.),
            _ => col,
        }
    }
}
