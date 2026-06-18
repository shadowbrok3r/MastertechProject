use egui_data_table::viewer::{DecodeErrorBehavior, RowCodec};

use super::data::{ShopifyLineItemRow, ShopifyOrderRow};

pub struct OrderCodec;

impl RowCodec<ShopifyOrderRow> for OrderCodec {
    type DeserializeError = &'static str;

    fn encode_column(&mut self, src_row: &ShopifyOrderRow, column: usize, dst: &mut String) {
        match column {
            0 => dst.push_str(&src_row.reference),
            1 => dst.push_str(&src_row.status),
            2 => dst.push_str(&src_row.customer),
            3 => dst.push_str(&src_row.build),
            4 => dst.push_str(&src_row.serials),
            5 => dst.push_str(&src_row.placed),
            _ => {}
        }
    }

    fn decode_column(
        &mut self,
        src_data: &str,
        column: usize,
        dst_row: &mut ShopifyOrderRow,
    ) -> Result<(), DecodeErrorBehavior> {
        match column {
            0 => dst_row.reference = src_data.to_string(),
            1 => dst_row.status = src_data.to_string(),
            2 => dst_row.customer = src_data.to_string(),
            3 => dst_row.build = src_data.to_string(),
            4 => dst_row.serials = src_data.to_string(),
            5 => dst_row.placed = src_data.to_string(),
            _ => {}
        }
        Ok(())
    }

    fn create_empty_decoded_row(&mut self) -> ShopifyOrderRow {
        ShopifyOrderRow::default()
    }
}

pub struct LineItemCodec;

impl RowCodec<ShopifyLineItemRow> for LineItemCodec {
    type DeserializeError = &'static str;

    fn encode_column(&mut self, src_row: &ShopifyLineItemRow, column: usize, dst: &mut String) {
        match column {
            0 => dst.push_str(&src_row.name),
            1 => dst.push_str(&src_row.reference),
            2 => dst.push_str(&src_row.quantity),
            3 => dst.push_str(&src_row.serials),
            _ => {}
        }
    }

    fn decode_column(
        &mut self,
        src_data: &str,
        column: usize,
        dst_row: &mut ShopifyLineItemRow,
    ) -> Result<(), DecodeErrorBehavior> {
        match column {
            0 => dst_row.name = src_data.to_string(),
            1 => dst_row.reference = src_data.to_string(),
            2 => dst_row.quantity = src_data.to_string(),
            3 => dst_row.serials = src_data.to_string(),
            _ => {}
        }
        Ok(())
    }

    fn create_empty_decoded_row(&mut self) -> ShopifyLineItemRow {
        ShopifyLineItemRow::default()
    }
}
