/// One row of the recent Shopify orders table. `lookup` is not displayed; it
/// reloads the full order on row click.
#[derive(Default, Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ShopifyOrderRow {
    pub reference: String,
    pub status: String,
    pub customer: String,
    pub build: String,
    pub serials: String,
    pub placed: String,
    pub lookup: String,
}

/// One line item of a loaded order's detail table.
#[derive(Default, Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ShopifyLineItemRow {
    pub name: String,
    pub reference: String,
    pub quantity: String,
    pub serials: String,
}
