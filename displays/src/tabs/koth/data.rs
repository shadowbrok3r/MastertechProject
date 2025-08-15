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

#[derive(Default, serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct AllEmployeesTableData {
    pub employee_id: String,
    pub employee_name: String,
    pub total_sales: usize,         // laptops + desktops
    pub total_orders: usize,        // orders count for this employee
    pub laptops: usize,
    pub desktops: usize,
    pub finance_ratio: f64,         // percent
    pub warranties: usize,          // count of orders with a warranty line
    pub revenue: f64,               // sum of total_paid_tax_excl (split-adjusted)
    pub spiffs: f64,                // sum of spiffs (split-adjusted)
}