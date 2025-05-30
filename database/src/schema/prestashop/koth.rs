

use crate::schema::prestashop::{Order, Prestashop};
use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
    match state {
        "239" => {
            // Loop through each date in the range
            let mut current_date = start_date;
            while current_date <= end_date {
                let mut url_params = HashMap::new();
                // Format the date for the API filter
                let date_filter = current_date.format("%Y-%m-%d").to_string();
                let date = format!("%[{}]%", date_filter);
                // Construct query parameters
                url_params.insert("output_format", "JSON");
                url_params.insert("filter[id_employee_sales_rep]", id_employee);
                url_params.insert("filter[current_state]", state);
                url_params.insert("filter[delivery_date]", &date);

                // Make the API request
                filtered_orders.append(&mut prestashop.request_resources_wasm::<Order>("orders", url_params).await?);

                // Move to the next date
                current_date = current_date + Duration::days(1);
            }
        },
        _ => {
            let start = &mut 0;
            let limit = 5;
            loop {
               // Construct query parameters
                let mut url_params = HashMap::new();
                url_params.insert("output_format", "JSON");
                url_params.insert("filter[id_employee_sales_rep]", id_employee);
                url_params.insert("filter[current_state]", state);
                url_params.insert("sort", "[id_DESC]");
                let limit_str = format!("{},{}", start, limit);
                url_params.insert("limit", &limit_str);
                
                let orders = prestashop.request_resources_wasm::<Order>("orders", url_params).await?;
                if orders.is_empty() {
                    break; // No more orders to fetch
                }

                // Track if any order is within the date range
                let has_orders_in_range = &mut false;
                let all_before_start = &mut true;

                for order in orders {
                    let order_id = order.id.clone();
                    // Parse date_upd to check if it's in range
                    let order_date = match NaiveDateTime::parse_from_str(&order.date_upd, "%Y-%m-%d %H:%M:%S") {
                        Ok(dt) => dt.date(),
                        Err(e) => {
                            log::error!(
                                "Failed to parse date_upd '{}' for order {}: {}",
                                order.date_upd,
                                order_id,
                                e
                            );
                            continue; // Skip orders with invalid date_upd
                        }
                    };

                    // Check if order_date is within range
                    if order_date >= start_date && order_date <= end_date {
                        filtered_orders.push(order);
                        *has_orders_in_range = true;
                    }
                    // Track if order is before start_date
                    if order_date >= start_date {
                        *all_before_start = false;
                    }
                }

                // Break if all orders are before start_date (no more relevant orders)
                if *all_before_start {
                    break;
                }

                // Move to the next page
                *start += limit;

                // Optional: Break if no orders in range and we’ve passed end_date
                if !*has_orders_in_range {
                    let latest_order_date = match NaiveDateTime::parse_from_str(
                        &filtered_orders.last().map(|o| o.date_upd.as_str()).unwrap_or(""),
                        "%Y-%m-%d %H:%M:%S",
                    ) {
                        Ok(dt) => dt.date(),
                        Err(_) => continue,
                    };
                    if latest_order_date > end_date {
                        break;
                    }
                }
            }
        },
    };

    Ok(filtered_orders)
}