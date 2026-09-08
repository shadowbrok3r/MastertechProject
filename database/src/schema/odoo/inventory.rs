use crate::schema::prestashop::{PRESTASHOP_API_URL_WASM, OrderDetails, PrestashopId};

pub async fn search_open_orders_for_product(product: &str, store: &str) -> anyhow::Result<(), anyhow::Error> {
    let states: Vec<String> = crate::schema::prestashop::OPEN_ORDER_STATES
        .iter()
        .map(i64::to_string)
        .collect();

    let order_details_to_check = &mut vec![];

        for state in states.iter() {
            let responses: Vec<PrestashopId> = crate::prestashop_get(format!("{PRESTASHOP_API_URL_WASM}/orders?output_format=JSON&display=[id]&filter[id_store]={store}&filter[id_order_type]=1&filter[current_state]={state}"))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        for res in responses.iter() {
            let id_order = &res.id;

            let order_detail_responses: Vec<OrderDetails> = crate::prestashop_get(format!("{PRESTASHOP_API_URL_WASM}/order_details&output_format=JSON&filter[product_reference]={product}&filter[id_order]={id_order}"))
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;

            for order_detail in order_detail_responses.iter() {
                order_details_to_check.push(order_detail.clone());
            }
        }
    }

    log::debug!("Order Details To Check: {order_details_to_check:?}");

    Ok(())
}
