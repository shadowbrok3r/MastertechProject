use egui_data_table::viewer::{DecodeErrorBehavior, RowCodec};
use super::data::{AllEmployeesTableData, KothTableData};

/* -------------------------------------------- Codec ------------------------------------------- */
pub struct Codec;

/* ------------------------------- KothTableData Codec ------------------------------- */
impl RowCodec<KothTableData> for Codec {
    type DeserializeError = &'static str;

    fn encode_column(&mut self, src_row: &KothTableData, column: usize, dst: &mut String) {
        match column {
            0 => dst.push_str(&src_row.order_id),
            1 => dst.push_str(&src_row.date),
            2 => dst.push_str(&src_row.order_state),
            3 => dst.push_str(&src_row.product),
            4 => dst.push_str(&src_row.payment),
            5 => dst.push_str(&src_row.warranty),
            6 => dst.push_str(&format!("{:.2}", src_row.spiffs)),
            7 => dst.push_str(&format!("{:.2}", src_row.total_paid)),
            8 => dst.push_str(&format!("{:.2}", src_row.total_without_tax)),
            _ => {}
        }
    }

    fn decode_column(
        &mut self,
        src_data: &str,
        column: usize,
        dst_row: &mut KothTableData,
    ) -> Result<(), DecodeErrorBehavior> {
        match column {
            0 => dst_row.order_id = src_data.to_string(),
            1 => dst_row.date = src_data.to_string(),
            2 => dst_row.order_state = src_data.to_string(),
            3 => dst_row.product = src_data.to_string(),
            4 => dst_row.payment = src_data.to_string(),
            5 => dst_row.warranty = src_data.to_string(),
            6 => dst_row.spiffs = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            7 => dst_row.total_paid = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            8 => dst_row.total_without_tax = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            _ => {}
        }
        Ok(())
    }

    fn create_empty_decoded_row(&mut self) -> KothTableData { KothTableData::default() }
}

/* --------------------------- AllEmployeesTableData Codec --------------------------- */
impl RowCodec<AllEmployeesTableData> for Codec {
    type DeserializeError = &'static str;

    fn encode_column(&mut self, src_row: &AllEmployeesTableData, column: usize, dst: &mut String) {
        match column {
            0 => dst.push_str(&src_row.employee_name),
            1 => dst.push_str(&format!("{} / {}", src_row.total_sales, src_row.total_orders)),
            2 => dst.push_str(&format!("{} / {}", src_row.laptops, src_row.desktops)),
            3 => dst.push_str(&format!("{:.2}", src_row.finance_ratio)),
            4 => dst.push_str(&format!("{} / {}", src_row.warranties, src_row.total_sales)),
            5 => dst.push_str(&format!("{:.2}", src_row.revenue)),
            6 => dst.push_str(&format!("{:.2}", src_row.spiffs)),
            _ => {}
        }
    }

    fn decode_column(
        &mut self,
        src_data: &str,
        column: usize,
        dst_row: &mut AllEmployeesTableData,
    ) -> Result<(), DecodeErrorBehavior> {
        match column {
            0 => dst_row.employee_name = src_data.to_string(),
            1 => {
                // format: "sales / orders"
                let parts: Vec<&str> = src_data.split('/').map(|s| s.trim()).collect();
                if let Some(s) = parts.get(0) { dst_row.total_sales = s.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?; }
                if let Some(o) = parts.get(1) { dst_row.total_orders = o.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?; }
            }
            2 => {
                // format: "laptops / desktops"
                let parts: Vec<&str> = src_data.split('/').map(|s| s.trim()).collect();
                if let Some(l) = parts.get(0) { dst_row.laptops = l.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?; }
                if let Some(d) = parts.get(1) { dst_row.desktops = d.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?; }
            }
            3 => {
                // finance ratio as number or with '%'
                let s = src_data.trim_end_matches('%').trim();
                dst_row.finance_ratio = s.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?;
            }
            4 => {
                // format: "warranties / total_sales" (total_sales retained from col 1)
                let parts: Vec<&str> = src_data.split('/').map(|s| s.trim()).collect();
                if let Some(w) = parts.get(0) { dst_row.warranties = w.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?; }
                // do not override total_sales here to avoid conflicts
            }
            5 => dst_row.revenue = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            6 => dst_row.spiffs = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            _ => {}
        }
        Ok(())
    }

    fn create_empty_decoded_row(&mut self) -> AllEmployeesTableData { AllEmployeesTableData::default() }
}