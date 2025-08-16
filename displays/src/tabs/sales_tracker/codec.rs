use egui_data_table::viewer::{DecodeErrorBehavior, RowCodec};
use super::data::SalesTableData;

pub struct Codec;

impl RowCodec<SalesTableData> for Codec {
    type DeserializeError = &'static str;

    fn encode_column(&mut self, src_row: &SalesTableData, column: usize, dst: &mut String) {
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
            9 => dst.push_str(&src_row.notes),
            _ => {}
        }
    }

    fn decode_column(&mut self, src_data: &str, column: usize, dst_row: &mut SalesTableData) -> Result<(), DecodeErrorBehavior> {
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
            9 => dst_row.notes = src_data.to_string(),
            _ => {}
        }
        Ok(())
    }

    fn create_empty_decoded_row(&mut self) -> SalesTableData { SalesTableData::default() }
}
