use database::{live_data::{handle_live_create, handle_live_delete, handle_live_update, listen_data, listen_task_notes, listen_tasks}, schema::utilities::{get_connected_clients, get_notifications, get_store_users, get_tasks}};
use displays::ui_tools::toasts::{Toast, ToastKind, ToastOptions};
use eframe::egui::{Color32, RichText};
use crate::utilities::get_data::get_customer_data;
use crate::app_state::{AppState, MtechServer};
use wasm_bindgen_futures::spawn_local;
use database::STORAGE_URL;
use surrealdb::Action;
use log::debug;
use log::info;
use mtechserver::webworker::Input;

// #[cfg(target_arch="wasm32")]
use {
    crate::app_state::check_authentication,
    mtechserver::live_worker::LiveInput,
};

impl MtechServer {
    pub fn first_run(&mut self) {
        self.context.first_run = false;
        // #[cfg(target_arch="wasm32")]
        match check_authentication(self.context.db_tx.clone()){
            Ok(d) => {
                info!("1");
                self.state = d.0;
                if let Some(ref usr) = d.1{
                    self.context.current_user = Some(usr.clone());
                    self.context.file_system.set_user(usr.clone());
                    let bridge_op = &self.context.bridge;
                    // let live_bridge = &self.context.live_bridge;
                    // info!("live bridge?");
                    // if let Some(live_bridge) = live_bridge{
                    //     info!("Have live bridge");
                    //     live_bridge.send(LiveInput { url: "fuck if i know".to_string() });
                    // }
                    if let (
                        Some(access_key), 
                        Some(secret_key), 
                        Some(bridge)
                    ) = (
                        usr.minio_access_key.clone(), 
                        usr.minio_secret_key.clone(), 
                        bridge_op
                    ) {
                        self.context.file_system.access_key = access_key.clone();
                        self.context.file_system.secret_key = secret_key.clone();
                        let name = usr.email.clone();
                        let parsed = name.split_once('@').unwrap().0.to_string().clone();
                        bridge.send(Input {
                            url: STORAGE_URL.to_string(),
                            access_key,
                            secret_key,
                            name: parsed
                        });
                    }
                }
            },
            Err(e) => {
                info!("2");
                info!("Error with auth: {e:?}");
                self.state = AppState::NoAuth(e.to_string());
                self.context.current_user = None;
            },
        };
    }

    pub fn load_data(&mut self) {
        // get all of our channel Senders from crossbeam to get user/store/completed tasks, 
        // as well as store users and live task notifications
        let live_tasks_tx = self.context.live_tasks_tx.clone();
        let live_clients_tx = self.context.live_clients_tx.clone();
        let initial_tasks_tx = self.context.initial_tasks_tx.clone();
        let store_users_tx = self.context.store_users_tx.clone();
        let tx = self.context.connected_clients_tx.clone();
        let notes_tx = self.context.notes_tx.clone();
        let notification_tx = self.context.notification_tx.clone();
        let live_output = self.context.live_output_tx.clone();

        if let Some(usr) = self.context.current_user.as_ref(){
            info!("Getting Initial data");
            let user = usr.clone();
            let name = usr.name.clone();

            let bridge_op = &self.context.bridge;

            if let (
                Some(access_key), 
                Some(secret_key), 
                Some(bridge)
            ) = (
                usr.minio_access_key.clone(), 
                usr.minio_secret_key.clone(), 
                bridge_op
            ) {
                self.context.file_system.access_key = access_key.clone();
                self.context.file_system.secret_key = secret_key.clone();
                let name = usr.email.clone();
                let parsed = name.split_once('@').unwrap().0.to_string().clone();
                bridge.send(Input {
                    url: STORAGE_URL.to_string(),
                    access_key,
                    secret_key,
                    name: parsed
                });
            }

            spawn_local(async move {
                let listen_task_notes = listen_task_notes(notes_tx).await;
                info!("listen_task_notes: {listen_task_notes:?}");
            });

            spawn_local(async move {
                let listen_tasks = listen_tasks(live_tasks_tx).await;
                info!("listen_tasks: {listen_tasks:?}");
            });

            spawn_local(async move {
                let listen_data = listen_data(live_clients_tx).await;
                info!("listen_data: {listen_data:?}");
            });
            
            // spawn_local(async move { let listen_data = listen_notifications(notification_tx.clone()).await; info!("listen_notifications: {listen_notifications:?}"); });

            spawn_local(async move {
                let get_tasks = get_tasks(initial_tasks_tx).await;
                let get_store_users = get_store_users(store_users_tx, user.clone().store).await;
                let get_connected_clients = get_connected_clients(tx, user.clone()).await;
                let get_notifications = get_notifications(notification_tx, user.clone().id.0).await;
                let get_custs = get_customer_data(live_output).await;
                info!("get_notifications: {get_notifications:?}");
                info!("get_connected_clients: {get_connected_clients:?}");
                info!("get_tasks: {get_tasks:?}");
                info!("get_store_users: {get_store_users:?}");
                info!("get_custs: {get_custs:?}");
            });

            // let live_bridge = &self.context.live_bridge;
            // info!("live bridge?");
            // if let Some(live_bridge) = live_bridge{
            //     info!("Have live bridge");
            //     live_bridge.send(LiveInput { url: "fuck if i know".to_string() });
            // }
            let toast = &mut self.context.toasts;
            let auth_toast = Toast{
                kind: ToastKind::Success,
                text: format!("Logged in successfully\nWelcome, {}", name).into(),
                options: ToastOptions::default().show_progress(true).duration_in_seconds(6.0)
            };
            toast.add(auth_toast);
        }else{
            info!("4");
            #[cfg(target_arch="wasm32")]
            match check_authentication(self.context.db_tx.clone()){
                Ok(d) => {
                    self.state = d.0;
                    if let Some(ref usr) = d.1{
                        let bridge_op = &self.context.bridge;

                        if let (
                            Some(access_key), 
                            Some(secret_key), 
                            Some(bridge)
                        ) = (
                            usr.minio_access_key.clone(), 
                            usr.minio_secret_key.clone(), 
                            bridge_op
                        ) {
                            self.context.file_system.access_key = access_key.clone();
                            self.context.file_system.secret_key = secret_key.clone();
                            let name = usr.email.clone();
                            let parsed = name.split_once('@').unwrap().0.to_string().clone();
                            bridge.send(Input {
                                url: STORAGE_URL.to_string(),
                                access_key,
                                secret_key,
                                name: parsed
                            });
                        }
                        self.context.current_user = Some(usr.clone());
                        self.context.file_system.set_user(usr.clone());
                        let user = usr.clone();
                        spawn_local(async move {
                            let listen_task_notes = listen_task_notes(notes_tx).await;
                            info!("listen_task_notes: {listen_task_notes:?}");
                        });
            
                        spawn_local(async move {
                            let listen_tasks = listen_tasks(live_tasks_tx).await;
                            info!("listen_tasks: {listen_tasks:?}");
                        });
            
                        spawn_local(async move {
                            let listen_data = listen_data(live_clients_tx).await;
                            info!("listen_data: {listen_data:?}");
                        });
                        
                        // spawn_local(async move { let listen_data = listen_notifications(notification_tx.clone()).await; info!("listen_notifications: {listen_notifications:?}"); });
            
                        spawn_local(async move {
                            let get_tasks = get_tasks(initial_tasks_tx).await;
                            let get_store_users = get_store_users(store_users_tx, user.clone().store).await;
                            let get_connected_clients = get_connected_clients(tx, user.clone()).await;
                            let get_notifications = get_notifications(notification_tx, user.clone().id.0).await;
                            let get_custs = get_customer_data(live_output).await;
                            info!("get_notifications: {get_notifications:?}");
                            info!("get_connected_clients: {get_connected_clients:?}");
                            info!("get_tasks: {get_tasks:?}");
                            info!("get_store_users: {get_store_users:?}");
                            info!("get_custs: {get_custs:?}");
                        });
                        let toast = &mut self.context.toasts;
                        let auth_toast = Toast{
                            kind: ToastKind::Success,
                            text: format!("Welcome, {}", usr.name).into(),
                            options: ToastOptions::default().show_progress(true).duration_in_seconds(6.0)
                        };
                        toast.add(auth_toast);
                    }
                },
                Err(e) => {
                    info!("Error with auth: {e:?}");
                    self.state = AppState::NoAuth(e.to_string());
                    self.context.current_user = None;
                },
            };
        }
    }

    pub fn receive(&mut self) {
        if let Ok(tasks) = self.context.initial_tasks_rx.try_recv(){
            self.context.tasks = tasks;
        }

        if let Ok(users) = self.context.store_users_rx.try_recv(){
            self.context.store_users = Some(users);
        }

        if let Ok(notifications) = self.context.notification_rx.try_recv(){
            self.context.notifications = notifications;
        }

        if let Ok(live_output) = self.context.live_output_rx.try_recv() {
            info!("Customers: {live_output:?}");
            self.context.data_output = live_output;
        }

        if let Ok((action, new_client)) = self.context.live_clients_rx.try_recv(){
            info!("new_client: {action:?} // {new_client:?}");
            
            if let (Some(usr), Some(current_user)) = (&new_client.assigned_user, &self.context.current_user){
                if usr == &current_user.id{
                    let toast = &mut self.context.toasts;
                    let txt = match action {
                        Action::Create => RichText::new(
                            format!("Client has connected: {}", &new_client.connection_string)
                            ).color(Color32::LIGHT_GREEN),
                        // Action::Update => RichText::new(
                        //     format!("Client update: {:#?}", &new_client.clone())
                        // ).color(Color32::LIGHT_BLUE),
                        Action::Delete => RichText::new(
                            format!("Client has disconnected: {}", &new_client.connection_string)
                        ).color(Color32::LIGHT_RED),
                        _ => RichText::new(
                            format!("Client has connected: {}", &new_client.connection_string)
                            ).color(Color32::LIGHT_GREEN),
                    };
                    let toast_opts = ToastOptions::default().show_progress(true).duration_in_seconds(5.0);
        
                    let client_connected_toast = Toast{ kind: ToastKind::Success, text: txt.into(), options: toast_opts };
        
                    toast.add(client_connected_toast);
                }
            }


            match action{
                Action::Create => handle_live_create(&mut self.context.clients, new_client.clone()).unwrap_or(()),
                Action::Update => handle_live_update(&mut self.context.clients, new_client.clone()).unwrap_or(()),
                Action::Delete => handle_live_delete(&mut self.context.clients, new_client.clone()).unwrap_or(()),
                _ => (),
            };
        }

        if let Ok(connected_clients) = self.context.connected_clients_rx.try_recv(){
            self.context.clients = connected_clients.clone();
            for client in connected_clients {
                self.context.undock_client.insert(client.connection_string, false);
            }
        }

        if let Ok(state) = self.context.app_state_rx.try_recv(){
            debug!("Got a new state: {state:?}");
            self.state = state
        }
    }
}