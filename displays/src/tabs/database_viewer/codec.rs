use egui_data_table::viewer::{DecodeErrorBehavior, RowCodec};
use database::schema::prestashop_schema::PrestashopPayload;
use chrono::{DateTime, NaiveDateTime, Utc};

/* -------------------------------------------- Codec ------------------------------------------- */
pub struct Codec;

impl RowCodec<PrestashopPayload> for Codec {
    type DeserializeError = &'static str;

    fn encode_column(&mut self, src_row: &PrestashopPayload, column: usize, dst: &mut String) {
        match column {
            0 => dst.push_str(&src_row.order.id),
            1 => dst.push_str(&src_row.customer.name),
            2 => {
                // Parse the input into a NaiveDateTime
                let naive_datetime = NaiveDateTime::parse_from_str(
                    &src_row.order.date_add,
                    "%Y-%m-%d %H:%M:%S"
                )
                .expect("Failed to parse datetime");
            
                // Convert to a DateTime with Utc timezone
                let datetime: DateTime<Utc> = DateTime::from_naive_utc_and_offset(naive_datetime, Utc);
                // Format the DateTime into yyyy/mm/dd
                let formatted_date = datetime.format("%m/%d/%Y").to_string();
                dst.push_str(&formatted_date);
            },
            3 => dst.push_str(&src_row.order.current_state),
            4 => {
                let emp = src_row.sales_rep.clone().unwrap_or_default();
                log::info!("Employee: {emp:?}");
                let name = format!("{} {}", emp.firstname, emp.lastname);
                dst.push_str(&name);
            },
            5 => {
                let emp = src_row.split_rep.clone().unwrap_or_default();
                log::info!("Employee: {emp:?}");
                let name = format!("{} {}", emp.firstname, emp.lastname);
                dst.push_str(&name);
            },
            6 => dst.push_str(&src_row.order.associations.order_service.get(0).cloned().unwrap_or_default().check_in_notes),
            7 => dst.push_str(&src_row.order.associations.order_service.get(0).cloned().unwrap_or_default().device_mfg),
            8 => dst.push_str(&src_row.order.associations.order_service.get(0).cloned().unwrap_or_default().device_model),
            9 => dst.push_str("False"),
            _ => {},
        }
    }

    fn decode_column(
        &mut self,
        src_data: &str,
        column: usize,
        dst_row: &mut PrestashopPayload,
    ) -> Result<(), DecodeErrorBehavior> {
        match column {
            0 => dst_row.order.id.replace_range(.., src_data),
            1 => dst_row.customer.name = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            2 => dst_row.order.date_add = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            3 => dst_row.order.current_state = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            4 => {
                let dst = &mut dst_row.sales_rep.clone().unwrap_or_default().firstname;
                *dst = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?
            },
            5 => {
                let dst = &mut dst_row.split_rep.clone().unwrap_or_default().firstname;
                *dst = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?
            },
            6 => dst_row.order.associations.order_service.get(0).cloned().unwrap_or_default().check_in_notes = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            7 => dst_row.order.associations.order_service.get(0).cloned().unwrap_or_default().device_mfg = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            8 => dst_row.order.associations.order_service.get(0).cloned().unwrap_or_default().device_model = src_data.parse().map_err(|_| DecodeErrorBehavior::SkipRow)?,
            9 => {},
            _ => {},
        }
        Ok(())
    }

    fn create_empty_decoded_row(&mut self) -> PrestashopPayload {
        PrestashopPayload::default()
    }
}