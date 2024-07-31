use database::{schema::TaskPayload, STORAGE_URL};
use mtechserver::{live_worker::LiveInput, webworker::Input};
use utilities::{displays::{chats::ChatView, modals::{create_task_modal::CreateTaskModal, task_modal::TaskModal}}, get_data::{get_associated_ticket, get_connected_clients, get_customer_data, get_notifications, get_store_users, get_tasks}, handle_live_data::{handle_live_create, handle_live_data, handle_live_delete, handle_live_notes, handle_live_update, listen_data, listen_task_notes, listen_tasks, update_or_insert, update_or_insert_layout, update_or_insert_notes}, ModalType, TaskUiActions};
use crate::utilities::ui_tools::{carl_dark::{CarlDark, Aesthetix}, toasts::{Toast, ToastKind, ToastOptions}};
use app_state::{check_authentication, AppState, MainPages, MtechServer};
use eframe::egui::{Color32, FontId, Stroke, Style, Vec2, Context};
use wasm_bindgen_futures::spawn_local;
use eframe::egui::FontFamily;
use log::{debug, info};
use surrealdb::Action;
use std::sync::Arc;

pub mod tabs;
pub mod app_state;
pub mod utilities;
pub mod webworker;
pub mod pages;

impl eframe::App for MtechServer {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // most important part of the whole app.. setting up our styling
        let arc_style = set_style();
        ctx.set_style(arc_style);
        // let alt_style = set_alternative_style();
        // ctx.set_style(alt_style);

        let data_update = self.context.data_update.as_mut().unwrap();
        if let Some(items) = data_update.take() { self.context.file_system.build_file_system(items); }
        
        let live_data_update = self.context.live_data_update.as_mut().unwrap();
        if let Some(items) = live_data_update.take() { info!("live_data_update: {:?}", items); }

        // do some setting up in the initial frame of our update loop for 
        // 1. Getting database connection
        if self.context.first_run{ // || or if refresh button is hit
            self.context.first_run = false;

            match check_authentication(self.context.db_tx.clone()){
                Ok(d) => {
                    info!("1");
                    self.state = d.0;
                    if let Some(ref usr) = d.1{
                        self.context.current_user = Some(usr.clone());
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
                            bridge.send(Input {
                                url: STORAGE_URL.to_string(),
                                access_key,
                                secret_key,
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

        // Retrieve our database connection, and 2. Requesting some task data
        if let Ok(db) = self.context.db_rx.try_recv(){
            match db{
                Ok(_db) => {
                    info!("3");
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
                            bridge.send(Input {
                                url: STORAGE_URL.to_string(),
                                access_key,
                                secret_key,
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
                        

                        // spawn_local(async move {
                        //     let listen_data = listen_notifications(notification_tx.clone()).await;
                        //     info!("listen_notifications: {listen_notifications:?}");
                        // });

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

                        let live_bridge = &self.context.live_bridge;
                        info!("live bridge?");
                        if let Some(live_bridge) = live_bridge{
                            info!("Have live bridge");
                            live_bridge.send(LiveInput { url: "fuck if i know".to_string() });
                        }

                        let toast = &mut self.context.toasts;
                        let auth_toast = Toast{
                            kind: ToastKind::Success,
                            text: format!("Logged in successfully\nWelcome, {}", name).into(),
                            options: ToastOptions::default().show_progress(true).duration_in_seconds(6.0)
                        };
                        toast.add(auth_toast);
                    }else{
                        info!("4");
                        match check_authentication(self.context.db_tx.clone()){

                            Ok(d) => {
                                self.state = d.0;
                                if let Some(ref usr) = d.1{
                                    self.context.current_user = Some(usr.clone());
                                    let user = usr.clone();
                                    spawn_local(async move {
                                        info!("5");
                                        let _ = get_tasks(initial_tasks_tx).await;
                                        let _ = get_store_users(store_users_tx, user.store).await;
                                        let _ = listen_task_notes(notes_tx).await.unwrap();
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
                },
                Err(e) => {
                    info!("6");
                    if e.to_string().contains("Already connected"){
                        info!("7");
                        self.state = AppState::Authenticated(MainPages::Tasks); 
                        let toast = &mut self.context.toasts;
                        let auth_toast = Toast{
                            kind: ToastKind::Success,
                            text: format!("Already Connected").into(),
                            options: ToastOptions::default().show_progress(true).duration_in_seconds(6.0)
                        };
                        toast.add(auth_toast);
                    } else {
                        info!("8");
                        info!("{e:?}");
                        wasm_cookies::delete("jwt");
                        wasm_cookies::delete("user");
                        // eframe::web::storage::local_storage_get(key)
                        let toast = &mut self.context.toasts;
                        let auth_toast = Toast{
                            kind: ToastKind::Error,
                            text: format!("{e:?} \nYou may need to login again").into(),
                            options: ToastOptions::default().show_progress(true).duration_in_seconds(6.0)
                        };
                        toast.add(auth_toast);
                        self.state = AppState::NoAuth("Needs login".to_string());
                    }
                }
            }
        }
        
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

        if let Ok(action) = self.context.ui_actions_rx.try_recv(){
            match action{
                TaskUiActions::OpenTaskModal(task) => {
                    let task_modal = if let Some(notes) = &task.task_note{
                        let chat_modal = ChatView::new(notes.clone(), self.context.current_user.as_ref().unwrap().clone(), task.id.clone().unwrap());
                        TaskModal::new(chat_modal, task.clone())
                    }else{ TaskModal::new(ChatView::new(Vec::new(), self.context.current_user.as_ref().unwrap().clone(), task.id.clone().unwrap()), task.clone()) };
                    self.context.current_modal = ModalType::TaskModal(task_modal);
                    self.context.task_modal_handler.open();
                },
                TaskUiActions::CreateTaskModal => {
                    let create_modal = CreateTaskModal::new("Create Task", self.context.store_users.clone());
                    self.context.current_modal = ModalType::CreateTaskModal(create_modal);
                    self.context.create_task_modal_handler.open();
                },
                TaskUiActions::Response(_res) => { }
                TaskUiActions::OpenChatModal(pld) => {
                    info!("Got Chat action");
                    if let Some(current_user) = self.context.current_user.as_ref() {
                        let chat_modal = ChatView::new(pld.1.to_owned(), current_user.clone(), pld.0.clone());
                        self.context.current_modal = ModalType::ChatView(chat_modal);
                        self.context.chat_modal_handler.open();
                    }// self.context.chat = ModalType::ChatView(pld);
                }, _ => (),
            }
        }

        if let Ok(new_task) = self.context.live_tasks_rx.try_recv(){
            info!("New Task Update");
            let tx = self.context.new_ticket_tx.clone();
            if let Some(service_num) = new_task.clone().1.service_number{
                if !service_num.is_empty() {
                    let new_task = new_task.clone();
                    spawn_local(async move {
                        match get_associated_ticket(tx, new_task.clone()).await{
                            Ok(_) => {},// info!("Got associated ticket"),
                            Err(e) => info!("Error getting associated ticket: {e:?}")
                        }
                    });
                }
            }else { 
                info!("Inserting Task");
                self.context.rerun_filtering_completed = true;
                self.context.rerun_filtering_my_tasks = true;
                self.context.rerun_filtering_store_tasks = true;
                handle_live_data(new_task.to_owned(), &mut self.context.tasks, None).unwrap(); 
            }
        }

        if let Ok(channel) = self.context.new_ticket_rx.try_recv(){
            info!("New Ticket Update ");
            for (_, layout) in self.context.task_layouts.iter_mut() {
                for (_, tasks) in layout.task_map.iter_mut(){ // .zip(tasks) 
                    for task in tasks.iter_mut(){
                        if task.id.clone().unwrap().0.id == channel.new_task.1.id.clone().unwrap().0.id{
                            debug!("\nReplacing {:?}\n with \n{:?}\n", task.task_name.clone(), channel.new_task.1.task_name.clone());
                            match update_or_insert_layout(
                                &mut self.context.tasks, 
                                channel.new_task.1.clone(), 
                            Some(channel.new_ticket.clone()), 
                            task
                            ){
                                Ok(_) => {
                                    self.context.rerun_filtering_my_tasks = true;
                                    self.context.rerun_filtering_store_tasks = true;
                                    self.context.rerun_filtering_completed = true;
                                    info!("Updated existing task");
                                },
                                Err(e) => info!("Error updating existing task: {e:?}"),
                            }
                        } else {
                            match update_or_insert(&mut self.context.tasks, channel.new_task.1.clone(), Some(channel.new_ticket.clone())){
                                Ok(_) => {},// info!("Updated existing task"),
                                Err(e) => info!("Error updating existing task: {e:?}"),
                            }
                        }
                    }
                }
            }
        }

        if let Ok((action, new_client)) = self.context.live_clients_rx.try_recv(){
            match action{
                Action::Create => handle_live_create(&mut self.context.clients, new_client.clone()).unwrap_or(()),
                Action::Update => handle_live_update(&mut self.context.clients, new_client.clone()).unwrap_or(()),
                Action::Delete => handle_live_delete(&mut self.context.clients, new_client.clone()).unwrap_or(()),
                _ => (),
            };
        }

        if let Ok(payload) = self.context.notes_rx.try_recv(){
            info!("New note");
            self.context.new_note = true;
            if let ModalType::TaskModal(task_modal) = &mut self.context.current_modal{
                handle_live_notes(payload.clone(), &mut task_modal.task).unwrap_or(());
                info!("Inserting note into modal");
                task_modal.chat_view.insert_note(payload.1);
            } else if let ModalType::ChatView(chat_view) = &mut self.context.current_modal{
                let task = self.context.tasks.iter_mut().find(|task| task.id == chat_view.task_id );
                if let Some(task) = task{
                    handle_live_notes(payload.clone(), task).unwrap_or(());
                    info!("Inserting note into modal");
                    chat_view.insert_note(payload.1);
                }
            }
        }

        if let Ok(state) = self.context.app_state_rx.try_recv(){
            debug!("Got a new state: {state:?}");
            self.state = state
        }

        if let Ok(connected_clients) = self.context.connected_clients_rx.try_recv(){
            for client in connected_clients.iter(){
                if self.context.clients.get(&client.connection_string).is_none() {
                    self.context.clients.insert(client.connection_string.clone(), client.clone());
                }
            }
        }

        self.menu_bar(ctx);
        self.context.handle_modals(ctx);
        self.context.toasts.show(ctx);
        // Always checking authentication.
        match &self.state{
            AppState::Authenticated(MainPages::Tasks) => self.main_page(ctx),
            AppState::NoAuth(_reason) => self.login_page(ctx, self.context.db_tx.clone(), self.context.app_state_tx.clone()),
            AppState::Authenticated(MainPages::Downloads) => self.downloads_page(ctx),
            AppState::Authenticated(_) => self.main_page(ctx),
            AppState::CreateAccount => self.signup_page(ctx, self.context.db_tx.clone(), self.context.app_state_tx.clone())
        }
    }

    fn persist_egui_memory(&self) -> bool { true }
    fn save(&mut self, _storage: &mut dyn eframe::Storage) { }
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) { }
}

// When compiling to web using trunk:
#[cfg(target_arch = "wasm32")]
fn main() {
    // use eframe::wgpu::PowerPreference;
    // use eframe::wgpu::{Backends, PowerPreference};
    use log::LevelFilter;
    eframe::WebLogger::init(LevelFilter::Info).ok();
    let web_options = eframe::WebOptions::default();
    // web_options.wgpu_options.power_preference = PowerPreference::HighPerformance;
    // web_options.wgpu_options.supported_backends = Backends::METAL;
    // web_options.wgpu_options.supported_backends = eframe::wgpu::Instance::

    wasm_bindgen_futures::spawn_local(async {
        eframe::WebRunner::new()
            .start(
                "mtech_canvas", // hardcode it
                web_options,
                Box::new(|cc| Ok(Box::new(MtechServer::new(cc)))),
            )
            .await
            .expect("failed to start eframe");
    });
}

// When compiling natively:
#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([400.0, 300.0])
            .with_min_inner_size([300.0, 220.0])
            .with_icon(
                // NOTE: Adding an icon is optional
                eframe::icon_data::from_png_bytes(&include_bytes!("../assets/mtechlogo.png")[..])
                    .expect("Failed to load icon"),
            ),
        ..Default::default()
    };
    eframe::run_native(
        "MtechServer",
        native_options,
        Box::new(|cc| Ok(Box::new(MtechServer::new(cc)))),
    )
}

fn set_style() -> Arc<Style>{
    let theme = CarlDark;
    let mut custom_style: Style = theme.custom_style();
    let mut font = FontId::default();
    font.size = 10.5;
    font.family = FontFamily::Proportional;
    
    custom_style.override_font_id = Some(font);
    custom_style.spacing.button_padding.x = 3.0;
    custom_style.spacing.button_padding.y = 3.0;
    custom_style.spacing.item_spacing = Vec2::new(2.0, 1.0);
    custom_style.spacing.combo_height = 55.0; 
    custom_style.spacing.combo_width = 100.0;
    custom_style.interaction.multi_widget_text_select = false;
    custom_style.interaction.selectable_labels = true;
    custom_style.explanation_tooltips = false;
    custom_style.url_in_tooltip = true;
    custom_style.interaction.interact_radius = 10.0;
    custom_style.interaction.resize_grab_radius_side = 10.0;
    custom_style.interaction.resize_grab_radius_corner = 10.0;
    custom_style.visuals.window_shadow.spread = 8.0;
    custom_style.visuals.window_shadow.blur = 10.0;
    // custom_style.visuals.panel_fill = Color32::from_rgb(16,16,17);
    // custom_style.visuals.window_fill = Color32::from_rgb(16,16,17);
    custom_style.visuals.selection.stroke.color =  Color32::from_rgba_premultiplied(199, 20, 150, 100);
    custom_style.visuals.selection.bg_fill = Color32::from_rgba_premultiplied(40,40,40,20);
    custom_style.visuals.widgets.inactive.bg_fill =  Color32::from_rgb(17,17,19);
    custom_style.visuals.widgets.inactive.fg_stroke =  Stroke::new(1.0, Color32::WHITE);
    custom_style.visuals.widgets.inactive.weak_bg_fill =  Color32::from_rgb(20, 20, 25);
    custom_style.visuals.widgets.inactive.bg_stroke =  Stroke::new(1.0, Color32::from_rgb(80, 80, 80));
    custom_style.visuals.widgets.open.bg_fill =  Color32::LIGHT_BLUE;
    custom_style.visuals.widgets.open.weak_bg_fill =  Color32::LIGHT_BLUE;
    custom_style.visuals.widgets.active.weak_bg_fill =  Color32::from_rgb(28,28,28);
    custom_style.visuals.widgets.active.bg_fill =  Color32::LIGHT_GREEN;
    custom_style.visuals.widgets.noninteractive.weak_bg_fill = Color32::from_rgb(15,15,19);
    // custom_style.visuals.
    // custom_style.visuals.widgets.hovered.weak_bg_fill =  Color32::TRANSPARENT;
    // custom_style.visuals.widgets.hovered.bg_fill =  Color32::from_rgb(12, 12, 12);
    custom_style.visuals.widgets.hovered.bg_stroke =  Stroke::new(0.5, Color32::from_rgba_premultiplied(120, 20, 120, 100));
    let arc_style = Arc::new(custom_style);
    arc_style
}

fn _set_alternative_style() -> Arc<Style> {
    let theme = CarlDark;
    let mut custom_style: Style = theme.custom_style();
    let mut font = FontId::default();
    font.size = 10.5;
    font.family = FontFamily::Proportional;
    
    custom_style.override_font_id = Some(font);
    custom_style.spacing.button_padding.x = 3.0;
    custom_style.spacing.button_padding.y = 3.0;
    custom_style.spacing.item_spacing = Vec2::new(2.0, 1.0);
    custom_style.spacing.combo_height = 55.0; 
    custom_style.spacing.combo_width = 100.0;
    custom_style.interaction.multi_widget_text_select = false;
    custom_style.interaction.selectable_labels = false;
    custom_style.explanation_tooltips = false;
    custom_style.url_in_tooltip = true;
    custom_style.interaction.interact_radius = 10.0;
    custom_style.interaction.resize_grab_radius_side = 10.0;
    custom_style.interaction.resize_grab_radius_corner = 10.0;
    custom_style.visuals.window_shadow.spread = 8.0;
    custom_style.visuals.window_shadow.blur = 10.0;
    
    // Update color scheme based on the extracted colors
    custom_style.visuals.selection.stroke.color =  Color32::from_rgb(199, 20, 150); // Kept the same for contrast
    custom_style.visuals.selection.bg_fill = Color32::from_rgb(40, 40, 40); // Kept the same for contrast
    custom_style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(13, 16, 23);
    custom_style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    custom_style.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(12, 15, 22);
    custom_style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(21, 24, 31));
    custom_style.visuals.widgets.open.bg_fill = Color32::from_rgb(18, 21, 28);
    custom_style.visuals.widgets.open.weak_bg_fill = Color32::from_rgb(18, 21, 28);
    custom_style.visuals.widgets.active.weak_bg_fill = Color32::from_rgb(18, 21, 28);
    custom_style.visuals.widgets.active.bg_fill = Color32::from_rgb(20, 23, 29);
    custom_style.visuals.widgets.noninteractive.weak_bg_fill = Color32::from_rgb(12, 15, 22);
    custom_style.visuals.widgets.hovered.bg_stroke = Stroke::new(0.5, Color32::from_rgb(199, 20, 150)); // Kept the same for contrast
    
    let arc_style = Arc::new(custom_style);
    arc_style
}
