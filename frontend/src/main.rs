use app_state::{check_authentication, AppState, MainPages, MtechServer, NewTicketChannel};
use database::schema::{TaskNotePayload, TaskPayload, TicketPayload, TICKET_TABLE};
use egui::FontFamily;
use log::{debug, info};
use ratframe::NewCC;
use surrealdb::{Action, Response};
use tabs::web_console::websockets::WebSocketClient;
use utilities::{displays::{chats::ChatView, modals::{create_task_modal::CreateTaskModal, task_modal::TaskModal}}, get_other::{get_connected_clients, get_store_users}, get_tasks::get_tasks, handle_live_data::{handle_live_create, handle_live_data, handle_live_delete, handle_live_notes, handle_live_update, listen_data, listen_task_notes, listen_tasks}, ModalType, TaskUiActions};
use wasm_bindgen_futures::spawn_local;
use std::sync::Arc;
use eframe::egui::{Color32, FontId, Stroke, Style, Vec2, Context};
use crate::utilities::ui_tools::{carl_dark::{CarlDark, Aesthetix}, toasts::{Toast, ToastKind, ToastOptions}};

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

        let data_update = self.context.data_update.as_mut().unwrap();
        if let Some(items) = data_update.take() { 
            self.context.file_system.build_file_system(items);
        }

        // For updating our Ratatui chart in the RataGuiBackend terminal
        // if self.context.last_tick.elapsed() >= self.context.tick_rate {
        //     self.context.chart_app.on_tick();
        //     self.context.last_tick = Instant::now();
        // }

        // do some setting up in the initial frame of our update loop for 
        // 1. Getting database connection
        if self.context.first_run{ // || or if refresh button is hit
            self.context.first_run = false;
            match check_authentication(self.context.db_tx.clone()){
                Ok(d) => {
                    self.state = d.0;
                    if let Some(ref _usr) = d.1{
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
            info!("Got db");
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
                    let notes_tx = self.context.notes_tx.clone();
                    if let Some(usr) = self.context.current_user.as_ref(){
                        info!("Getting Initial data");
                        get_tasks(db.clone(), initial_tasks_tx);
                        get_store_users(db.clone(), store_users_tx, usr.store);
                        listen_tasks(db.clone(), live_tasks_tx);
                        listen_data(db.clone(), live_clients_tx);
                        listen_task_notes(db.clone(), notes_tx);

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
                                    // listen_tasks(db.clone(), live_tasks_tx);
                                    // listen_task_notes(db.clone(), notes_tx);

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
            // info!("Got tasks? {tasks:?}");
            self.context.tasks = Some(tasks);
        }

        if let Ok(users) = self.context.store_users_rx.try_recv(){
            info!("Got users");
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
                TaskUiActions::Response(_res) => { }
            }
        }

        if let Ok(ref new_task) = self.context.live_tasks_rx.try_recv(){
            let database = &self.context.database.clone();
            let tx = self.context.new_ticket_tx.clone();
            if let Some(existing_tasks) = &mut self.context.tasks{
                if let Some(service_num) = new_task.1.clone().service_number{
                    if !service_num.is_empty() {
                        let db = database.clone();
                        if let Some(db) = db{
                            let n_task = new_task.clone();
                            spawn_local(async move {
                                let x: Result<Response, surrealdb::Error> = db.database
                                    .query(
                                        format!("SELECT * FROM service_order WHERE service_number == {}", service_num.clone())
                                    )
                                    .await;
                                
                                match x{
                                    Ok(mut data) => {
                                        info!("data: {:?}", data);
                                        let ticket: Option<TicketPayload> = data.take(0).unwrap();


                                        let chnnl = NewTicketChannel {
                                            new_ticket: ticket.unwrap_or_default(),
                                            new_task: n_task,
                                        };
                                        match tx.try_send(chnnl){
                                            Ok(_) => info!("Sent ticket"),
                                            Err(e) => info!("Error sending ticket: {e:?}")
                                        }
                                    },
                                    Err(e) => info!("ERROR: {e:?}"),
                                }
                            });
                        }
                    }
                }else {
                    handle_live_data(new_task.to_owned(), existing_tasks, None).unwrap();
                }
            }
        }

        if let Ok(channel) = self.context.new_ticket_rx.try_recv(){
            if let Some(existing_tasks) = &mut self.context.tasks{
                let live_task = channel.new_task.1;
                let check = existing_tasks.iter().any(|x| x.id == live_task.id);
                info!("existing_tasks.service_num matches new task.service_num: {check}");
                if !check{
                    existing_tasks.push(TaskPayload {
                        id: live_task.id,
                        task_name: live_task.task_name,
                        service_ticket: Some(channel.new_ticket),
                        everest_initials: live_task.everest_initials,
                        task_description: live_task.task_description,
                        assignee: live_task.assignee,
                        service_number: live_task.service_number,
                        due_date: live_task.due_date,
                        priority: live_task.priority,
                        task_note: None,
                        completed: live_task.completed,
                        status: live_task.status,
                        dep: live_task.dep,
                    });
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
            self.context.new_note = true;
            if let ModalType::TaskModal(task_modal) = &mut self.context.current_modal{
                if let Some(task) = task_modal.task.as_mut(){
                    handle_live_notes(payload.clone(), task).unwrap_or(());
                    // info!("Got the new note {:?}");
                    task_modal.chat_view.insert_note(payload.1);
                }
            }
        }

        if let Ok(state) = self.context.app_state_rx.try_recv(){
            debug!("Got a new state: {state:?}");
            self.state = state
        }

        if let Ok(connected_clients) = self.context.connected_clients_rx.try_recv(){
            for client in connected_clients.iter(){
                self.context.clients.insert(client.connection_string.clone(), client.clone());
            }
        }


        
        self.menu_bar(ctx);
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
            AppState::Authenticated(MainPages::Downloads) => {
                self.downloads_page(ctx);
            },
            AppState::Authenticated(_) => {
                self.main_page(ctx);
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
    // use eframe::wgpu::PowerPreference;
    // use eframe::wgpu::{Backends, PowerPreference};
    use log::LevelFilter;
    eframe::WebLogger::init(LevelFilter::Info).ok();
    let mut web_options = eframe::WebOptions::default();
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
    custom_style.spacing.button_padding.y = 2.0;
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
    custom_style.visuals.selection.stroke.color =  Color32::from_rgb(29, 209, 161);
    custom_style.visuals.selection.bg_fill = Color32::from_rgb(120, 10, 120);
    custom_style.visuals.widgets.inactive.bg_fill =  Color32::from_rgb(15,14,18);
    custom_style.visuals.widgets.inactive.fg_stroke =  Stroke::new(1.0, Color32::WHITE);
    custom_style.visuals.widgets.inactive.weak_bg_fill =  Color32::from_rgb(20, 20, 25);
    custom_style.visuals.widgets.inactive.bg_stroke =  Stroke::new(1.0, Color32::from_rgb(80, 80, 80));
    custom_style.visuals.widgets.open.bg_fill =  Color32::from_black_alpha(50);
    custom_style.visuals.widgets.open.weak_bg_fill =  Color32::from_black_alpha(50);
    custom_style.visuals.widgets.active.weak_bg_fill =  Color32::from_rgb(28,28,28);
    custom_style.visuals.widgets.noninteractive.weak_bg_fill = Color32::from_rgb(15,15,19);
    // custom_style.visuals.widgets.hovered.weak_bg_fill =  Color32::TRANSPARENT;
    custom_style.visuals.widgets.hovered.bg_fill =  Color32::from_rgb(12, 12, 12);
    custom_style.visuals.widgets.hovered.bg_stroke =  Stroke::new(1.0, Color32::from_rgb(200, 20, 200));
    let arc_style = Arc::new(custom_style);
    arc_style
}

