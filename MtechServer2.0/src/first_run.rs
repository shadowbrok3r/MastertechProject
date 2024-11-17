use std::collections::HashMap;

use crate::{
    app_state::{AppState, MtechServer},
    pages::downloads_page::get_github_releases,
    tabs::{ai_playground::ChatThread, stock::{get_extra_stock_info, get_stock}},
};
use database::{
    live_data::listen_data,
    schema::{
        utilities::{get_connected_clients, get_notifications, get_store_users, get_tasks},
        CONNECTED_CLIENT_TABLE, NOTIFICATION_TABLE, TASK_NOTE_TABLE, TASK_TABLE,
    },
    DATABASE,
};
use database::schema::Store;
use displays::ui_tools::toasts::{Toast, ToastKind, ToastOptions};
use eframe::Frame;
use egui_dock::DockState;
use log::info;
use log::{debug, error};
// use mtechserver::webworker::Input;
use wasm_bindgen_futures::spawn_local;

// #[cfg(target_arch="wasm32")]
use {
    crate::app_state::check_authentication,
    // mtechserver::live_worker::LiveInput,
};

impl MtechServer {
    pub fn first_run(&mut self, frame: &mut Frame) {
        self.context.first_run = false;
        let github_releases_tx = self.context.github_releases_channel.0.clone();
        spawn_local(async move {
            let get_releases = get_github_releases(github_releases_tx).await;
            info!("get_releases: {get_releases:?}");
        });

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

            // Get existing chats a user has
            // with ChatGPT
            if let Some(chat_history) = storage.get_string("chat_history") {
                let chat_threads: HashMap<String, ChatThread> = serde_json::from_str(&chat_history).unwrap_or_default();
                self.context.ai_playground.set_threads(chat_threads);
            }

            if let Some(version) = storage.get_string("version") {
                if env!("CARGO_PKG_VERSION") != version {
                    gloo_console::info!(format!("Mismatched Cargo Version. Doing update from {:?} to -> {:?}", version, env!("CARGO_PKG_VERSION")));
                    #[cfg(target_arch = "wasm32")]
                    {
                        wasm_cookies::delete("user");
                        wasm_cookies::delete("jwt");
                    }
                    spawn_local(async move {
                        let invalidation = DATABASE.invalidate().await;
                        info!("invalidated connection: {:?}", invalidation);
                    });

                    if let Some(window) = web_sys::window() {
                        let reload = window.location().reload();
                        info!("Reloading winodw: {reload:?}");
                        if let Ok(storage) = window.local_storage() {
                            if let Some(storage) = storage {
                                let clear = storage.clear();
                                info!("Clearing storage: {clear:?}");
                            }
                        }
                    } else {
                        info!("No window");
                    }
                    let logout_msg = "Logged out".to_string();
                    self.state = AppState::NoAuth(logout_msg.clone());
                    match self
                        .context
                        .app_state_tx
                        .try_send(AppState::NoAuth(logout_msg))
                    {
                        Ok(_) => info!("Logged out"),
                        Err(e) => error!("Error: {e:?}"),
                    }
                }
            }
            else {
                storage.set_string(
                    "version",
                    env!("CARGO_PKG_VERSION").to_string()
                );
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
        let stock_tx = self.context.stock_channel.0.clone();
        let ex_stock_tx = self.context.extra_stock_channel.0.clone();
        let live_notif_tx = self.context.live_notification_tx.clone();
        let notif_tx = self.context.notification_tx.clone();

        if let Some(usr) = self.context.current_user.as_ref() {
            info!("Getting Initial data");
            let user = usr.clone();
            let name = usr.name.clone();

            // if self.context.file_system.paths.is_empty() {
            //     let bridge_op = &self.context.bridge;
            //
            //     if let (Some(access_key), Some(secret_key), Some(bridge)) = (
            //         usr.minio_access_key.clone(),
            //         usr.minio_secret_key.clone(),
            //         bridge_op,
            //     ) {
            //         self.context.file_system.access_key = access_key.clone();
            //         self.context.file_system.secret_key = secret_key.clone();
            //         let name = usr.email.clone();
            //         let parsed = name.split_once('@').unwrap().0.to_string().clone();
            //         info!("Retrieving minio files");
            //         bridge.send(Input {
            //             url: STORAGE_URL.to_string(),
            //             access_key,
            //             secret_key,
            //             name: parsed,
            //         });
            //     }
            // }

            spawn_local(async move {
                let listen_data = listen_data(notes_tx, TASK_NOTE_TABLE).await;
                info!("listen_task_notes: {listen_data:?}");
            });

            spawn_local(async move {
                let listen_data = listen_data(live_tasks_tx, TASK_TABLE).await;
                info!("listen_tasks: {listen_data:?}");
            });

            spawn_local(async move {
                let listen_data = listen_data(live_clients_tx, CONNECTED_CLIENT_TABLE).await;
                info!("listen_clients: {listen_data:?}");
            });

            spawn_local(async move {
                let listen_data = listen_data(live_notif_tx.clone(), NOTIFICATION_TABLE).await;
                info!("listen_notifications: {listen_data:?}");
            });

            if self.context.tasks.is_empty() || self.context.store_users.is_empty() {
                let store_selection = match usr.clone().store {
                    Store::RIV => 76,
                    Store::LTN => 73,
                    Store::MUR => 74,
                    Store::AF => 72,
                    Store::WJ => 78,
                    Store::ORE => 75,
                    Store::SAN => 77,
                };

                spawn_local(async move {
                    let get_tasks = get_tasks(initial_tasks_tx).await;
                    let get_store_users = get_store_users(store_users_tx, user.clone().store).await;
                    let get_connected_clients = get_connected_clients(tx, user.clone()).await;
                    let stock = get_stock(stock_tx.clone(), store_selection).await;
                    let stock_quantities = get_extra_stock_info(ex_stock_tx).await;
                    let notifications = get_notifications(notif_tx.clone(), user.clone().id).await;
                    info!("Get Notifications: {notifications:?}");
                    info!("Extra Stock {stock_quantities:?}");
                    info!("Stock call: {stock:?} for Store: {:?}", store_selection);
                    info!("get_connected_clients: {get_connected_clients:?}");
                    info!("get_store_users: {get_store_users:?}");
                    info!("get_tasks: {get_tasks:?}");
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

        // if let Ok(live_output) = self.context.live_output_rx.try_recv() {
        //     info!("Customers: {live_output:?}");
        //     self.context.data_output = live_output;
        // }

        if let Ok(releases) = self.context.github_releases_channel.1.try_recv() {
            debug!("Releases: {releases:?}");
            self.context.github_releases = releases;
        }

        if let Ok(state) = self.context.app_state_rx.try_recv() {
            debug!("Got a new state: {state:?}");
            self.state = state
        }

        if let Ok(thread_obj) = self.context.ai_thread_channel.1.try_recv() {
            let mut thread_map = HashMap::new();
            self.context.save_chats = true;
            thread_map.insert(thread_obj.id.clone(), ChatThread {
                id: thread_obj.id.clone(),
                messages: Vec::new(),
                images: Vec::new(),
                input: String::new(),
            });
            self.context.ai_playground.selected_thread = thread_obj.id;
            self.context.ai_playground.set_threads(thread_map);
        }
    }
}
