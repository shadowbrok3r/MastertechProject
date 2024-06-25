use app_state::{check_authentication, AppState, MainPages, MtechServer};
use egui_toast::{Toast, ToastKind, ToastOptions};
use log::info;
use ratframe::NewCC;
use surrealdb::Action;
use tabs::web_console::websockets::{ClientConnection, ClientDisplay, WebSocketClient};
use utilities::{displays::{chats::ChatView, modals::{create_task_modal::CreateTaskModal, task_modal::TaskModal}}, get_other::{disconnect_client, get_connected_clients, get_store_users}, get_tasks::get_tasks, handle_live_data::{handle_live_create, handle_live_data, handle_live_delete, handle_live_update, listen_data, listen_tasks}, ModalType, TaskUiActions};
use wasm_bindgen_futures::spawn_local;
use wasm_cookies::CookieOptions;
use web_time::Instant;
use std::sync::Arc;
use egui::{Color32, FontId, Stroke, Style, Vec2};
use egui_aesthetix::{themes::CarlDark, Aesthetix};

pub mod tabs;
pub mod app_state;
pub mod utilities;
pub mod webworker;
pub mod pages;

impl eframe::App for MtechServer {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {

        // most important part of the whole app.. setting up our styling
        let arc_style = set_style();
        ctx.set_style(arc_style);

        // i have no god damn idea what this is really doing. it was a 
        // wasm example for using web workers.. i dont even know if its required???
        let data_update = self.context.data_update.as_mut().unwrap();
        if let Some(update) = data_update.take() {
            info!("Received update: {update:?}")
        }

        // For updating our Ratatui chart in the RataGuiBackend terminal
        if self.context.last_tick.elapsed() >= self.context.tick_rate {
            self.context.chart_app.on_tick();
            self.context.last_tick = Instant::now();
        }

        // do some setting up in the initial frame of our update loop for 
        // 1. Getting database connection
        if self.context.first_run{ // || or if refresh button is hit
            self.context.first_run = false;
            // wasm_cookies::set("Cross-Origin-Embedder-Policy","require-corp", &CookieOptions::default().with_same_site(wasm_cookies::SameSite::None));
            // wasm_cookies::set("Cross-Origin-Opener-Policy", "same-origin", &CookieOptions::default().with_same_site(wasm_cookies::SameSite::None));
            match check_authentication(self.context.db_tx.clone()){
                Ok(d) => {
                    self.state = d.0;
                    if let Some(ref _usr) = d.1{
                        // let toast = &mut self.context.toasts;
                        // let auth_toast = Toast{ kind: ToastKind::Success, text: format!("Welcome, {}", usr.name).into(), options: ToastOptions::default().show_progress(true).duration_in_seconds(6.0) };
                        // toast.add(auth_toast);
                        self.context.current_user = d.1;
                    }
                },
                Err(e) => {
                    info!("Error with auth: {e:?}");
                    self.state = AppState::NoAuth(e.to_string());
                    self.context.current_user = None;
                },
            };
        }

        // Retrieve our database connection, and 
        // 2. Requesting some task data
        if let Ok(db) = self.context.db_rx.try_recv(){
            match db{
                Ok(db) => {
                    self.context.database = Some(db.clone());
                    // get all of our channel Senders from crossbeam to get user/store/completed tasks, 
                    // as well as store users and live task notifications
                    let live_tasks_tx = self.context.live_tasks_tx.clone();
                    let live_clients_tx = self.context.live_clients_tx.clone();
                    let initial_tasks_tx = self.context.initial_tasks_tx.clone();
                    let store_users_tx = self.context.store_users_tx.clone();
                    let tx = self.context.connected_clients_tx.clone();

                    if let Some(usr) = self.context.current_user.as_ref(){
                        get_tasks(db.clone(), initial_tasks_tx);
                        get_store_users(db.clone(), store_users_tx, usr.store);
                        listen_tasks(db.clone(), live_tasks_tx);
                        listen_data(db.clone(), live_clients_tx);
                        let user = usr.clone();
                        spawn_local(async move {
                            get_connected_clients(db, tx, user).await.unwrap();
                        });
                        let toast = &mut self.context.toasts;
    
                        let auth_toast = Toast{
                            kind: ToastKind::Success,
                            text: format!("Logged in successfully\nWelcome, {}", usr.name).into(),
                            options: ToastOptions::default()
                                .show_progress(true)
                                .duration_in_seconds(6.0)
                        };
                        toast.add(auth_toast);
                    }else{
                        match check_authentication(self.context.db_tx.clone()){
                            Ok(d) => {
                                self.state = d.0;
                                if let Some(ref usr) = d.1{
                                    self.context.current_user = Some(usr.clone());
                                    get_tasks(db.clone(), initial_tasks_tx);
                                    get_store_users(db.clone(), store_users_tx, usr.store);
                                    listen_tasks(db.clone(), live_tasks_tx);
            
                                    let toast = &mut self.context.toasts;
                
                                    let auth_toast = Toast{
                                        kind: ToastKind::Success,
                                        text: format!("Welcome, {}", usr.name).into(),
                                        options: ToastOptions::default()
                                            .show_progress(true)
                                            .duration_in_seconds(6.0)
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
                    info!("{e:?}");
                    let toast = &mut self.context.toasts;
    
                    let auth_toast = Toast{
                        kind: ToastKind::Error,
                        text: format!("{e:?} \nYou may need to login again").into(),
                        options: ToastOptions::default()
                            .show_progress(true)
                            .duration_in_seconds(6.0)
                    };
                    toast.add(auth_toast);
                    self.state = AppState::NoAuth("Needs login".to_string());
                }
            }
        }
        
        if let Ok(tasks) = self.context.initial_tasks_rx.try_recv(){
            self.context.tasks = Some(tasks);
        }

        if let Ok(users) = self.context.store_users_rx.try_recv(){
            self.context.store_users = Some(users);
        }

        if let Ok(action) = self.context.ui_actions_rx.try_recv(){
            match action{
                TaskUiActions::OpenTaskModal(task) => {
                    let mut task_modal = if let Some(notes) = &task.task_note{
                        let chat_modal = ChatView::new(notes.clone(), self.context.current_user.as_ref().unwrap().clone(), task.id.clone().unwrap());
                        TaskModal::new(chat_modal)
                    }else{
                        TaskModal::default()
                    };
                    task_modal.database = Some(self.context.database.as_ref().unwrap().to_owned());
                    task_modal.task = Some(task);
                    self.context.current_modal = ModalType::TaskModal(task_modal);
                    self.context.task_modal_handler.open();
                },
                TaskUiActions::CreateTaskModal => {
                    let create_modal = CreateTaskModal::new("Create Task", self.context.database.clone(), self.context.store_users.clone());
                    self.context.current_modal = ModalType::CreateTaskModal(create_modal);
                    self.context.create_task_modal_handler.open();
                },
                TaskUiActions::Response(_res) => {

                    
                }
            }
        }

        while let Ok(ref new_task) = self.context.live_tasks_rx.try_recv(){
            if let Some(existing_tasks) = &mut self.context.tasks{
                handle_live_data(new_task.to_owned(), existing_tasks).unwrap();
            }
        }

        while let Ok(ref new_data) = self.context.live_clients_rx.try_recv(){
            match new_data.0{
                Action::Create => handle_live_create(&mut self.context.clients, new_data.1.clone()).unwrap_or(()),
                Action::Update => handle_live_update(&mut self.context.clients, new_data.1.clone()).unwrap_or(()),
                Action::Delete => handle_live_delete(&mut self.context.clients, new_data.1.clone()).unwrap_or(()),
                _ => (),
            };
        }

        if let Ok(state) = self.context.app_state_rx.try_recv(){
            info!("Got a new state: {state:?}");
            self.state = state
        }

        if let Ok(connected_clients) = self.context.connected_clients_rx.try_recv(){
            for client in connected_clients.iter(){
                self.context.clients.insert(client.connection_string.clone(), client.clone());
            }
        }

        if let Ok(connection) = self.context.client_connection_rx.try_recv(){
            match connection{
                ClientConnection::ClientUrl(url) => {
                    // let wakeup = move || ctx.request_repaint();
                    match ewebsock::connect(&url, Default::default()) {
                        Ok((ws_sender, ws_receiver)) => {
                            let ws_client = WebSocketClient::new(ws_sender, ws_receiver);
                            self.context.client_layout = Some(ClientDisplay::new_client(self.context.clients.clone(), ws_client));
                        }
                        Err(error) => {
                            log::error!("Failed to connect to {:?}: {}", &url, error);
                            // self.error = error;
                        }
                    };
                },                
                
                ClientConnection::Disconnect(url) => {
                    spawn_local(async move {
                        // disconnect_client(db, tx, user).await.unwrap();
                    });
                    // let wakeup = move || ctx.request_repaint();
                    // match ewebsock::connect(&url, Default::default()) {
                    //     Ok((ws_sender, ws_receiver)) => {
                    //         ws_sender.close()
                    //         let ws_client = WebSocketClient::new(ws_sender, ws_receiver);
                    //         self.context.client_layout = Some(ClientDisplay::new_client(self.context.clients.clone(), ws_client));
                    //     }
                    //     Err(error) => {
                    //         log::error!("Failed to connect to {:?}: {}", &url, error);
                    //         // self.error = error;
                    //     }
                    // };
                },
            }

        }
        // Always checking authentication.
        match &self.state{
            //if auth'd, user shall be allowed
            AppState::Authenticated(MainPages::Tasks) => {
                // info!("Main page state");
                self.main_page(ctx);
            },
            // if no auth, appstate will be login_page
            AppState::NoAuth(_reason) => {
                self.login_page(ctx, self.context.db_tx.clone(), self.context.app_state_tx.clone());

                // info!("Login page state");
            },
            AppState::Authenticated(_) => {
                self.main_page(ctx);
                // info!("Authed state");
            },
            AppState::CreateAccount => {
                // info!("Create Account state");
                self.signup_page(ctx, self.context.db_tx.clone(), self.context.app_state_tx.clone());
            }
        }

        self.context.handle_modals(ctx);

        self.context.toasts.show(ctx);
    }

    // Called by the frame work to save state before shutdown.
    // fn save(&mut self, storage: &mut dyn eframe::Storage) { 
    //     eframe::set_value(storage, eframe::APP_KEY, self); 
    // }
}

// When compiling to web using trunk:
#[cfg(target_arch = "wasm32")]
fn main() {
    use eframe::wgpu::PowerPreference;
    // use eframe::wgpu::{Backends, PowerPreference};
    use log::LevelFilter;
    eframe::WebLogger::init(LevelFilter::Info).ok();
    let mut web_options = eframe::WebOptions::default();
    web_options.wgpu_options.power_preference = PowerPreference::HighPerformance;
    // web_options.wgpu_options.supported_backends = Backends::METAL;
    // web_options.wgpu_options.supported_backends = eframe::wgpu::Instance::

    wasm_bindgen_futures::spawn_local(async {
        eframe::WebRunner::new()
            .start(
                "mtech_canvas", // hardcode it
                web_options,
                Box::new(|cc| Box::new(MtechServer::new(cc))),
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
        Box::new(|cc| Box::new(MtechServer::new(cc))),
    )
}

fn set_style() -> Arc<Style>{
    let theme = CarlDark;
    let mut custom_style: Style = theme.custom_style();
    let mut font = FontId::default();
    font.size = 12.0;
    custom_style.override_font_id = Some(font);
    custom_style.spacing.button_padding.x = 2.0;
    custom_style.spacing.button_padding.y = 2.0;
    custom_style.spacing.item_spacing = Vec2::new(5.0, 2.0);
    custom_style.spacing.combo_height = 60.0; 
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
    custom_style.visuals.selection.stroke.color =  Color32::from_rgb(29, 209, 161);
    custom_style.visuals.selection.bg_fill = Color32::from_rgb(120, 10, 120);
    custom_style.visuals.widgets.inactive.bg_fill =  Color32::from_rgb(15,14,18);
    custom_style.visuals.widgets.inactive.fg_stroke =  Stroke::new(1.0, Color32::WHITE);
    custom_style.visuals.widgets.inactive.weak_bg_fill =  Color32::from_rgb(20, 20, 25);
    custom_style.visuals.widgets.inactive.bg_stroke =  Stroke::new(1.0, Color32::from_rgb(80, 80, 80));
    custom_style.visuals.widgets.open.bg_fill =  Color32::from_black_alpha(50);
    custom_style.visuals.widgets.open.weak_bg_fill =  Color32::from_black_alpha(50);
    custom_style.visuals.widgets.active.weak_bg_fill =  Color32::from_rgb(30,30,30);
    // custom_style.visuals.widgets.hovered.weak_bg_fill =  Color32::TRANSPARENT;
    custom_style.visuals.widgets.hovered.bg_fill =  Color32::from_rgb(12, 12, 12);
    custom_style.visuals.widgets.hovered.bg_stroke =  Stroke::new(1.0, Color32::from_rgb(200, 20, 200));
    let arc_style = Arc::new(custom_style);
    arc_style
}

