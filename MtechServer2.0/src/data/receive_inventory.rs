use crate::{
    app_state::{AppState, MtechServer},
    pages::downloads_page::get_github_releases,
    tabs::stock::{find_attached_serials, get_stock, BoolOrString, MyRowData},
    utilities::ModalType,
};
use database::{
    live_data::{handle_live_delete, listen_data, update_or_insert_anything},
    schema::{
        utilities::{get_connected_clients, get_store_users, get_tasks},
        TaskNotePayload, CONNECTED_CLIENT_TABLE, TASK_NOTE_TABLE, TASK_TABLE, TICKET_TABLE,
    },
    DATABASE,
};
use database::{schema::Store, STORAGE_URL};
use displays::ui_tools::toasts::{Toast, ToastKind, ToastOptions};
use eframe::{
    egui::{Color32, RichText},
    Frame,
};
use egui_dock::DockState;
use log::info;
use log::{debug, error};
use mtechserver::webworker::Input;
use surrealdb::{Action, RecordId};
use wasm_bindgen_futures::spawn_local;

// #[cfg(target_arch="wasm32")]
use {
    crate::app_state::check_authentication,
    // mtechserver::live_worker::LiveInput,
};

impl MtechServer {
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

            spawn_local(async move {
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
    }
}