use crate::schema::prestashop::{OrderDetails, PrestashopId};
use reqwest::Client;

pub async fn search_open_orders_for_product(product: &str, store: &str) -> anyhow::Result<(), anyhow::Error> {
    let client = Client::new();

    let states: Vec<&str> = vec![
        "4", "238", "40", "73", "70", "224", "71", "236", "84", "30", "29",
    ];

    let order_details_to_check = &mut vec![];

    for state in states.iter() {
        let responses: Vec<PrestashopId> = client.get(format!("https://pclaptops.mojo11.com/api/orders?output_format=JSON&display=[id]&filter[id_store]={store}&filter[id_order_type]=1&filter[current_state]={state}"))
            .send()
            .await?
            .json()
            .await?;

        for res in responses.iter() {
            let id_order = &res.id;

            let order_detail_responses: Vec<OrderDetails> = client.get(format!("https://pclaptops.mojo11.com/api/order_details&output_format=JSON&filter[product_reference]={product}&filter[id_order]={id_order}"))
                .send()
                .await?
                .json()
                .await?;

            for order_detail in order_detail_responses.iter() {
                order_details_to_check.push(order_detail.clone());
            }
        }
    }

    log::info!("Order Details To Check: {order_details_to_check:?}");

    Ok(())
}
