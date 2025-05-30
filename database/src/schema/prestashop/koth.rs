

use crate::schema::prestashop::{Order, Prestashop};
use chrono::{Datelike, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Debug)]
pub struct FilteredOrder {
    pub id: String,
    pub delivery_date: String,
    pub total_paid_tax_excl: f64,
    pub total_paid: f64,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Default, Clone)]
pub enum PayPeriod {
    #[default]
    Current,
    Last
}

impl PayPeriod {
    pub fn as_str(&self) -> &str {
        match self {
            PayPeriod::Current => "Current",
            PayPeriod::Last => "Last",
        }
    }
}

pub async fn generate_orders_report(pay_period: PayPeriod, state: &str, id_employee: &str) -> anyhow::Result<Vec<Order>, anyhow::Error> {
    let prestashop = Prestashop::default();

    // Get today's date
    let today = Utc::now().naive_utc().date();
    let current_day = today.day();
    let current_month = today.month();
    let current_year = today.year();

    // Determine the date range based on pay_period
    let (start_date, end_date) = match pay_period {
        PayPeriod::Current => {
            if (1..=16).contains(&current_day) {
                (
                    NaiveDate::from_ymd_opt(current_year, current_month, 1).ok_or_else(|| anyhow::anyhow!("Invalid start date"))?,
                    today,
                )
            } else {
                (
                    NaiveDate::from_ymd_opt(current_year, current_month, 16).ok_or_else(|| anyhow::anyhow!("Invalid start date"))?,
                    today,
                )
            }
        }
        PayPeriod::Last => {
            if (1..=15).contains(&current_day) {
                // Last pay period is the second half of the previous month
                let (prev_year, prev_month) = if current_month > 1 {
                    (current_year, current_month - 1)
                } else {
                    (current_year - 1, 12)
                };
                let last_day = NaiveDate::from_ymd_opt(prev_year, prev_month + 1, 1)
                    .map(|d| d - Duration::days(1))
                    .map(|d| d.day())
                    .unwrap_or(30);
                (
                    NaiveDate::from_ymd_opt(prev_year, prev_month, 16).ok_or_else(|| anyhow::anyhow!("Invalid start date"))?,
                    NaiveDate::from_ymd_opt(prev_year, prev_month, last_day).ok_or_else(|| anyhow::anyhow!("Invalid end date"))?,
                )
            } else {
                // Last pay period is the first half of the current month
                (
                    NaiveDate::from_ymd_opt(current_year, current_month, 1).ok_or_else(|| anyhow::anyhow!("Invalid start date"))?,
                    NaiveDate::from_ymd_opt(current_year, current_month, 15).ok_or_else(|| anyhow::anyhow!("Invalid end date"))?,
                )
            }
        }
    };

    let mut filtered_orders = vec![];

    // Loop through each date in the range
    let mut current_date = start_date;
    while current_date <= end_date {
        // Format the date for the API filter
        let date_filter = current_date.format("%Y-%m-%d").to_string();
        let date = format!("%[{}]%", date_filter);
        // Construct query parameters
        let mut url_params = HashMap::new();
        url_params.insert("output_format", "JSON");
        url_params.insert("filter[id_employee_sales_rep]", id_employee);
        match state {
            "Accepted By Odoo" => url_params.insert("filter[delivery_date]", &date),
            _ => url_params.insert("filter[date_upd]", &date)
        };

        url_params.insert("filter[current_state]", state);

        // Make the API request
        filtered_orders.append(&mut prestashop.request_resources_wasm::<Order>("orders", url_params).await?);

        // Move to the next date
        current_date = current_date + Duration::days(1);
    }

    Ok(filtered_orders)
}