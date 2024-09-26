use crate::{
    app_state::{AppState, MtechServer},
    pages::downloads_page::get_github_releases,
    tabs::stock::{get_stock, MyRowData},
    utilities::ModalType,
};
use anyhow::{Error, Result};
use database::STORAGE_URL;
use database::{
    live_data::{handle_live_delete, listen_data, update_or_insert_anything},
    schema::{
        utilities::{get_connected_clients, get_store_users, get_tasks},
        TaskNotePayload, CONNECTED_CLIENT_TABLE, TASK_NOTE_TABLE, TASK_TABLE, TICKET_TABLE,
    },
    DATABASE,
};
use displays::{
    egui_data_table::{DataTable, RowViewer},
    ui_tools::toasts::{Toast, ToastKind, ToastOptions},
};
use eframe::{
    egui::{Color32, RichText},
    Frame,
};
use egui_dock::DockState;
use log::info;
use log::{debug, error};
use mtechserver::webworker::Input;
use reqwest::Client;
use surrealdb::{Action, RecordId};
use wasm_bindgen_futures::spawn_local;

// #[cfg(target_arch="wasm32")]
use {
    crate::app_state::check_authentication,
    // mtechserver::live_worker::LiveInput,
};

impl MtechServer {
    pub fn first_run(&mut self, frame: &mut Frame) {
        self.context.first_run = false;

        if let Some(storage) = frame.storage_mut() {
            if let Some(settings) = storage.get_string("user_settings") {
                self.context.user_settings =
                    serde_json::from_str(settings.as_str()).unwrap_or_default();

                let mut startup_tabs = self.context.user_settings.startup_tabs.clone();
                if let Ok(state) = serde_json::from_value::<DockState<String>>(startup_tabs) {
                    for x in state.iter_all_tabs() {
                        info!("All Tabs: {:?}, {:?}, {:?}", x.1, x.0 .0, x.0 .1);
                    }
                    self.tree = state;
                } else {
                    info!("Setting startup tabs: {:?}", self.tree);
                    startup_tabs = serde_json::to_value(&self.tree).unwrap_or_default();
                    self.context.user_settings.startup_tabs = startup_tabs;
                    storage.set_string(
                        "user_settings",
                        serde_json::to_string(&self.context.user_settings).unwrap_or_default(),
                    );
                }
            }
        }

        // #[cfg(target_arch="wasm32")]
        match check_authentication(self.context.db_tx.clone()) {
            Ok(d) => {
                info!("1");
                self.state = d.0;
                if let Some(ref usr) = d.1 {
                    self.context.current_user = Some(usr.clone());
                    self.context.file_system.set_user(usr.clone());
                    spawn_local(async move {
                        match DATABASE.health().await {
                            Ok(_) => info!("Healthy connection"),
                            Err(e) => info!("Database connection health: {e:?}"),
                        }
                    });
                }
            }
            Err(e) => {
                info!("2");
                error!("Error with auth: {e:?}");
                self.state = AppState::NoAuth(e.to_string());
                self.context.current_user = None;
            }
        };
    }

    pub fn load_data(&mut self, frame: &mut Frame) {
        // get all of our channel Senders from crossbeam to get user/store/completed tasks,
        // as well as store users and live task notifications
        let live_tasks_tx = self.context.live_tasks_tx.clone();
        let live_clients_tx = self.context.live_clients_tx.clone();
        let initial_tasks_tx = self.context.initial_tasks_tx.clone();
        let store_users_tx = self.context.store_users_tx.clone();
        let tx = self.context.connected_clients_tx.clone();
        let notes_tx = self.context.notes_tx.clone();
        let github_releases_tx = self.context.github_releases_channel.0.clone();
        let stock_tx = self.context.stock_channel.0.clone();
        // let notification_tx = self.context.notification_tx.clone();
        // let live_output = self.context.live_output_tx.clone();

        if let Some(usr) = self.context.current_user.as_ref() {
            info!("Getting Initial data");
            let user = usr.clone();
            let name = usr.name.clone();

            if self.context.file_system.paths.is_empty() {
                let bridge_op = &self.context.bridge;

                if let (Some(access_key), Some(secret_key), Some(bridge)) = (
                    usr.minio_access_key.clone(),
                    usr.minio_secret_key.clone(),
                    bridge_op,
                ) {
                    self.context.file_system.access_key = access_key.clone();
                    self.context.file_system.secret_key = secret_key.clone();
                    let name = usr.email.clone();
                    let parsed = name.split_once('@').unwrap().0.to_string().clone();
                    bridge.send(Input {
                        url: STORAGE_URL.to_string(),
                        access_key,
                        secret_key,
                        name: parsed,
                    });
                }
            }

            spawn_local(async move {
                let listen_task_notes = listen_data(notes_tx, TASK_NOTE_TABLE).await;
                info!("listen_task_notes: {listen_task_notes:?}");
            });

            spawn_local(async move {
                let listen_tasks = listen_data(live_tasks_tx, TASK_TABLE).await;
                info!("listen_tasks: {listen_tasks:?}");
            });

            spawn_local(async move {
                let listen_data = listen_data(live_clients_tx, CONNECTED_CLIENT_TABLE).await;
                info!("listen_data: {listen_data:?}");
            });

            // spawn_local(async move { let listen_data = listen_notifications(notification_tx.clone()).await; info!("listen_notifications: {listen_notifications:?}"); });
            if self.context.tasks.is_empty() || self.context.store_users.is_empty() {
                spawn_local(async move {
                    let get_tasks = get_tasks(initial_tasks_tx).await;
                    let get_store_users = get_store_users(store_users_tx, user.clone().store).await;
                    let get_connected_clients = get_connected_clients(tx, user.clone()).await;
                    let get_releases = get_github_releases(github_releases_tx).await;
                    // let login_odoo = odoo_auth().await;
                    // if let Ok(cookie) = login_odoo {
                    odoo_auth().await.unwrap();
                    let stock = get_stock(stock_tx.clone()).await;
                    info!("Stock call: {stock:?}");
                    // }

                    // let get_notifications = get_notifications(notification_tx, user.clone().id.0).await;
                    // let get_custs = get_customer_data(live_output).await;
                    info!("get_connected_clients: {get_connected_clients:?}");
                    info!("get_store_users: {get_store_users:?}");
                    info!("get_tasks: {get_tasks:?}");
                    info!("get_releases: {get_releases:?}");
                    // info!("get_notifications: {get_notifications:?}");
                    // info!("get_custs: {get_custs:?}");
                });
            }

            // let live_bridge = &self.context.live_bridge;
            // if let Some(live_bridge) = live_bridge {
            //     live_bridge.send(LiveInput {
            //         url: "fuck if i know".to_string(),
            //     });
            // }

            let toast = &mut self.context.toasts;
            let auth_toast = Toast {
                kind: ToastKind::Success,
                text: format!("Logged in successfully\nWelcome, {}", name).into(),
                options: ToastOptions::default()
                    .show_progress(true)
                    .duration_in_seconds(6.0),
            };
            toast.add(auth_toast);
        } else {
            info!("4");
            self.context.first_run = true;
            self.first_run(frame);
            self.state = AppState::NoAuth("No user detected".to_string());
        }
    }

    pub fn receive(&mut self) {
        if let Ok(tasks) = self.context.initial_tasks_rx.try_recv() {
            self.context.tasks = tasks;
        }

        if let Ok(users) = self.context.store_users_rx.try_recv() {
            for (_, layout) in self.context.task_layouts.iter_mut() {
                layout.update_assignees(users.clone());
            }
            self.context.store_users = users;
        }

        // if let Ok(notifications) = self.context.notification_rx.try_recv(){
        //     self.context.notifications = notifications;
        // }

        if let Ok(live_output) = self.context.live_output_rx.try_recv() {
            info!("Customers: {live_output:?}");
            self.context.data_output = live_output;
        }

        if let Ok((action, new_client)) = self.context.live_clients_rx.try_recv() {
            info!("new_client: {action:?} // {new_client:?}");

            if let (Some(usr), Some(current_user)) =
                (&new_client.assigned_user, &self.context.current_user)
            {
                if usr == &current_user.id {
                    let toast = &mut self.context.toasts;
                    let txt = match action {
                        Action::Create => RichText::new(format!(
                            "Client has connected: {}",
                            &new_client.connection_string
                        ))
                        .color(Color32::LIGHT_GREEN),
                        // Action::Update => RichText::new(
                        //     format!("Client update: {:#?}", &new_client.clone())
                        // ).color(Color32::LIGHT_BLUE),
                        Action::Delete => RichText::new(format!(
                            "Client has disconnected: {}",
                            &new_client.connection_string
                        ))
                        .color(Color32::LIGHT_RED),
                        _ => RichText::new(format!(
                            "Client has connected: {}",
                            &new_client.connection_string
                        ))
                        .color(Color32::LIGHT_GREEN),
                    };
                    let toast_opts = ToastOptions::default()
                        .show_progress(true)
                        .duration_in_seconds(5.0);

                    let client_connected_toast = Toast {
                        kind: ToastKind::Success,
                        text: txt.into(),
                        options: toast_opts,
                    };

                    toast.add(client_connected_toast);
                }
            }

            match action {
                Action::Create => {
                    update_or_insert_anything(&mut self.context.clients, new_client.clone())
                        .unwrap_or(())
                }
                Action::Update => {
                    update_or_insert_anything(&mut self.context.clients, new_client.clone())
                        .unwrap_or(())
                }
                Action::Delete => {
                    handle_live_delete(&mut self.context.clients, new_client.clone()).unwrap_or(())
                }
                _ => (),
            };
        }

        if let Ok(connected_clients) = self.context.connected_clients_rx.try_recv() {
            self.context.clients = connected_clients.clone();
            for client in connected_clients {
                self.context
                    .undock_client
                    .insert(client.connection_string, false);
            }
        }

        if let Ok(releases) = self.context.github_releases_channel.1.try_recv() {
            debug!("Releases: {releases:?}");
            self.context.github_releases = releases;
        }

        if let Ok(stock_data) = self.context.stock_channel.1.try_recv() {
            let data: Vec<MyRowData> = stock_data
                .iter()
                .map(|stock_data| {
                    MyRowData(
                        stock_data.product_id.clone().1.clone(),
                        stock_data.lot_id.clone().1.parse::<String>().unwrap(),
                        "Riverdale".to_string(),
                        "".to_string(),
                        false,
                    )
                })
                .collect();
            self.context.data_table.replace(data);
        }

        if let Ok(presta_data) = self.context.tur_channel.1.try_recv() {
            self.context.tur.data = presta_data.clone();
            info!("{:?}", self.context.tur.data.clone());
            let customer = &mut self.context.tur.customer_data;
            let ticket = &mut self.context.tur.ticket_data;
            let task = &mut self.context.tur.task_data;
            let task_notes = &mut self.context.tur.task_notes;

            let service_details = presta_data.order.associations.order_service.clone();
            let mut services: Vec<RecordId> = Vec::new();

            let sales_rep = presta_data.sales_rep.clone().unwrap_or_default();
            let split_rep = presta_data.split_rep.clone().unwrap_or_default();

            let sales_rep_initials = sales_rep.initials.clone();
            let split_initials = split_rep.initials.clone();

            let email = sales_rep
                .email
                .split_once("@")
                .clone()
                .unwrap_or((&sales_rep_initials, ""))
                .0
                .to_string();

            let email_split_rep = split_rep
                .email
                .split_once("@")
                .clone()
                .unwrap_or((&split_initials, ""))
                .0
                .to_string();

            for msg in presta_data.customer_messages.iter() {
                task_notes.push(TaskNotePayload {
                    everest_initials: msg.id_employee.clone(),
                    note: msg.message.clone(),
                    ..Default::default()
                })
            }

            customer.id = presta_data.customer.id.clone();
            customer.cust_code = presta_data.customer.cust_code.clone();
            customer.email = presta_data.customer.email.clone();
            customer.name = presta_data.customer.name.clone();
            customer.phone_number = presta_data.customer.phone_number.clone();
            ticket.salesman = email_split_rep;
            ticket.sales_rep = email.clone();
            ticket.tech = email.clone();
            info!(
                "Salesman: {:?}\nTech: {:?}",
                ticket.salesman.clone(),
                ticket.tech.clone()
            );
            ticket.customer = Some(customer.clone());
            ticket.checkin_rep = email;
            ticket.terms = presta_data.order.payment.clone();
            ticket.ticket_total = presta_data.order.total_products_wt.clone();
            ticket.doc_alias = presta_data.order.order_type.clone();
            ticket.service_number = presta_data.order.id.clone();
            ticket.id = Some(RecordId::from((
                TICKET_TABLE.to_string(),
                ticket.service_number.clone(),
            )));

            if let Some(ticket_id) = &ticket.id {
                services.push(ticket_id.clone());
            }

            if !service_details.is_empty() {
                if service_details.len() == 1 {
                    let svc = service_details.get(0);
                    if let Some(service) = svc {
                        ticket.checkin_notes = service.check_in_notes.clone();
                    }
                } else {
                    info!("Theres a couple.... {:?}", service_details);
                }
            }

            task.service_ticket = Some(ticket.clone());

            if let ModalType::CreateTaskModal(ref mut create_task_modal) =
                self.context.current_modal
            {
                info!("Updating modal data");
                info!("{:?}", ticket.clone());
                info!("{:?}", customer.clone());
                info!("{:?}", task_notes.clone());
                create_task_modal.tur = self.context.tur.clone();
            }
        }

        if let Ok(state) = self.context.app_state_rx.try_recv() {
            debug!("Got a new state: {state:?}");
            self.state = state
        }
    }
}

pub async fn odoo_auth() -> Result<(), Error> {
    let client = Client::builder().build()?;

    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("Content-Type", "application/json".parse()?);

    let data = r#"{
        "jsonrpc": "2.0",
        "params": {
            "db": "pcl_live",
            "login": "logan.lees@pclaptops.com",
            "password": "7!BEZSssMOkwV$6W"
        }
    }"#;

    let json: serde_json::Value = serde_json::from_str(&data)?;

    let request = client
        .request(
            reqwest::Method::POST,
            "https://odoo.master-tech.app/web/session/authenticate",
        )
        .headers(headers)
        .json(&json);

    let response = request.send().await?;

    info!("Response: {:?}", response.text().await?);
    Ok(())
}
