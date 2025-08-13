#[derive(Default, serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct KothTableData {
    pub order_id: String,
    pub date: String,
    pub order_state: String,
    pub product: String,
    pub payment: String,
    pub warranty: String,
    pub total_paid: f64,
    pub total_without_tax: f64,
    pub spiffs: f64,
}