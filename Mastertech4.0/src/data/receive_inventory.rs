use crate::{
    MasterTechApp,
    tabs::{
        stock::{find_attached_serials, BoolOrString, MyRowData},
        stock_quantities::StockQuantityData,
    },
};

use database::schema::Store;
use log::debug;
use tokio::spawn;

impl MasterTechApp {
    pub fn receive_inventory(&mut self) {
        if let Ok(stock_data) = self.context.stock_channel.1.try_recv() {
            let data: Vec<MyRowData> = stock_data
                .iter()
                .map(|stock_data| {
                    MyRowData(
                        stock_data.product_id.clone().1.clone(),
                        stock_data.lot_id.clone().1.parse::<String>().unwrap(),
                        "S/N Info ⮫".to_string(),
                        match stock_data.location_id.0 {
                            76 => Store::RIV.as_str(),
                            73 => Store::LTN.as_str(),
                            74 => Store::MUR.as_str(),
                            78 => Store::WJ.as_str(),
                            75 => Store::ORE.as_str(),
                            72 => Store::AF.as_str(),
                            77 => Store::SAN.as_str(),
                            _ => Store::RIV.as_str(),
                        }
                        .to_string(),
                        false,
                    )
                })
                .collect();

            let tx = self.context.serial_channel.0.clone();

            let sns = data.iter().map(|r| r.1.clone()).collect::<Vec<String>>();

            spawn(async move {
                let _res = find_attached_serials(sns, tx.clone()).await;
            });

            self.context.data_table.replace(data);
        }

        if let Ok(serial_data) = self.context.serial_channel.1.try_recv() {
            debug!("Serial Data: {:?}", serial_data);
            let mut data_table = self.context.data_table.take();
            for data in data_table.iter_mut() {
                for serial_info in serial_data.result.iter() {
                    if data.1 == serial_info.name {
                        match serial_info.clone().bs_prest_ref {
                            BoolOrString::Bool(_) => {
                                data.2 = "Not Attached".to_string();
                                data.4 = false;
                            }
                            BoolOrString::String(order_num) => {
                                if !order_num.is_empty() {
                                    data.2 = order_num;
                                    data.4 = true;
                                } else {
                                    data.2 = "Not Attached".to_string();
                                    data.4 = false;
                                }
                            }
                        };
                    }
                }
            }
            self.context.data_table.replace(data_table);
        }

        if let Ok(stock_inf) = self.context.extra_stock_channel.1.try_recv() {
            debug!("Serial Data: {:?}", stock_inf);
            let data: Vec<StockQuantityData> = stock_inf
                .iter()
                .map(|stock_data| {
                    StockQuantityData(
                        stock_data.display_name.clone(),
                        stock_data.qty_available.clone(),
                        stock_data.virtual_available.clone(),
                        stock_data.standard_price.clone(),
                        stock_data.list_price.clone(),
                    )
                })
                .collect();
            self.context.stock_quantity_table.replace(data);
        }
    }
}
