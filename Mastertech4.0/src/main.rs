#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use utilities::{crypto::pass_hash::load_encrypted_user_data, displays::{chats::ChatView, modals::{create_task_modal::CreateTaskModal, task_modal::TaskModal}}, ModalType, TaskUiActions};
use database::{schema::{ComputerData, ComputerId, GetKeysResponse, HardwareTests, Record, Store, TaskNotePayload, TaskPayload, TicketId, User, TICKET_TABLE}, Database, DATABASE};
use eframe::egui::{style::Style, Color32, Context, FontFamily, FontId, IconData, Stroke, Vec2, ViewportBuilder};
use displays::ui_tools::{toasts::{Toast, ToastKind, ToastOptions}, carl_dark::{Aesthetix, CarlDark}};
use std::{fs::File, sync::{Arc, Condvar, Mutex}};
use tabs::tur_sheet::scaffold::AsanaResponse;
use log::{debug, error, info, LevelFilter};
use filesystem::system_info::ComputerInfo;
use app_state::{AppState, MasterTechApp};
use simplelog::{Config, WriteLogger};
use crossbeam::channel::Sender;
use pages::login_page::HASH;
use surrealdb::sql::Thing;
use tokio::spawn;

pub mod app_state;
pub mod tabs;
pub mod pages;
pub mod viewports;
pub mod utilities;
mod filesystem;

impl eframe::App for MasterTechApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // most important part of the whole app.. setting up our styling
        let arc_style = set_style();
        ctx.set_style(arc_style);

        if self.context.specs_first_run{
            self.context.specs_first_run = false;
            let tx = self.context.db_tx.clone();
            let pair = Arc::new((Mutex::new(ComputerData::default()), Condvar::new()));
            let pair_clone = Arc::clone(&pair);
            
            spawn(async move {
                match ComputerData::default().get_computer_data().await{ // sysinfo_tx
                    Ok(data) => {
                        let (lock, cvar) = &*pair_clone;
                        let mut comp_data = lock.lock().unwrap();
                        *comp_data = data;
                        info!("Computer Data: {comp_data:?}");
                        cvar.notify_one();
                    },
                    Err(e) => info!("Error getting specs: {e:?}"),
                }
            });

            // Wait for the spawned task to complete and notify the condition variable
            let (lock, cvar) = &*pair;
            let mut comp_data = lock.lock().unwrap();
            while comp_data.cpu.is_empty() {
                comp_data = cvar.wait(comp_data).unwrap();
            }
            // Access the shared data after notification
            self.context.computer_data = comp_data.clone();
            for disk in &self.context.computer_data.drives{
                self.context.disk_num += 1;
                if let Some(disks_arr) = self.context.disks.as_array_mut() {
                    let disk_json = serde_json::to_value(&disk).unwrap_or_default();
                    disks_arr.push(disk_json);
                } else { debug!("Expected self.context.drives to be an Array"); }
            }
            self.context.output_text += format!("{:#?}", &self.context.computer_data.seb_info.as_mut()).as_str();

            let loaded_data = load_encrypted_user_data(HASH);
            match loaded_data{
                Some(login) => {
                    self.state = AppState::Authenticated(app_state::MainPages::Tasks);
                
                    spawn(async move {
                        let db = Database::new(login.username, login.password, None).await;
                        info!("DB: {db:?}");
                        match tx.try_send(db){
                            Ok(_) => {
                                info!("Sent DB connection");
                                drop(tx)
                            },
                            Err(e) => info!("Error sending specs: {e:?}"),
                        }
                    });

                    // match x.poll_unpin(cx)
                    #[cfg(target_os="windows")]
                    {
                        let mut cps = self.context.current_antivirus.clone();
                        // let sysinfo = self.context.computer_data;
                        // let installed_antivirus = sysinfo.get_antivirus()
                        let installed_antivirus = ComputerData::get_antivirus()
                        .map_err(|e| 
                            cps += format!("Error checking antivirus: {e}\n").as_str()
                        ).unwrap_or(Vec::new());
            
            
                        for (name, is_installed) in installed_antivirus {
                            match is_installed {
                                Some(true) => {
                                    cps += "\n";
                                    cps += &format!("{name}");
                                },
                                _ => {},
                            }
                        }
                    }
                },
                None => { 
                    let toast = &mut self.context.toasts;
    
                    let error_toast = Toast{
                        kind: ToastKind::Error,
                        text: "Could not get login from encoded data".into(),
                        options: ToastOptions::default()
                            .show_progress(true)
                            .duration_in_seconds(6.0)
                    };
                    toast.add(error_toast);
                    self.state = AppState::NoAuth("No User returned from decryption phase".to_string()); 
                },
            }
        }

        if let Ok(db) = self.context.db_rx.try_recv(){
            info!("Received DB connection from thread");
            self.context.specs_first_run = true;
            match db{
                Ok(db) => {
                    self.context.current_user = db.user.clone();
                    let initial_tasks_tx = self.context.initial_tasks_tx.clone();
                    if let Some(usr) = db.user{
                        get_store_users(self.context.store_users_tx.clone(), usr.store);
                        get_tasks(initial_tasks_tx);
                    }
                },
                Err(e) => {
                    info!("Error with auth: {e:?}");
                    self.state = AppState::NoAuth(e.to_string());
                    self.context.current_user = None;
                },
            }
        }

        if let Ok(state) = self.context.app_state_rx.try_recv(){
            info!("Got a new state: {state:?}");
            self.state = state
        }

        while let Ok(message) = self.context.rx.try_recv() {       
            if let Ok(info) = serde_json::from_str::<GetKeysResponse>(&message) {
                if !info.webroot_key.is_empty() || !info.superanti_key.is_empty(){
                    self.context.keys = info;
                }
                self.context.spinner = false;
            }
            else if let Ok(info) = serde_json::from_str::<AsanaResponse>(&message) { 
                if let Some(e) = info.status{
                    self.context.output_text = format!("Status Code: {e:#?}");
                };
                self.context.output_text = format!("{:#?}", info.gid);
            }
            else{
                self.context.output_text = format!("{}", message);
                self.context.spinner = false;
            }
        }
        
        if let Some(dialog) = &mut self.context.open_file_dialog {
            if dialog.show(&ctx).selected() {
                if let Some(file) = dialog.path() {
                    self.context.opened_file = Some(file.to_path_buf());
                }
            }
        }
    
        if let Ok(data) = self.context.prestashop_api_rx.try_recv(){
            let customer = &mut self.context.customer_data;
            let ticket = &mut self.context.ticket_data;
            // let _task = &mut self.context.task_data;
            let task_notes = &mut self.context.task_notes;
            let computer = &mut self.context.computer_data;

            let hdd_test = format!("{:?}", &self.context.hdd_test_cbox);
            let ram_test = format!("{:?}", &self.context.ram_test_cbox);
            let ssd_test = format!("{:?}", &self.context.ssd_test_cbox);
            
            let service_details = data.order.associations.order_service;
            let mut owned_computers: Vec<ComputerId> = Vec::new();
            let mut services: Vec<TicketId> = Vec::new();

            #[cfg(target_os="windows")] {
                let cps = &mut self.context.current_antivirus;
                let mut cps_v = Vec::new();
                let installed_antivirus = ComputerData::get_antivirus()
                .map_err(|e| 
                    *cps += format!("Error checking antivirus: {e}\n").as_str()
                ).unwrap_or_default();
    
    
                for (name, _is_installed) in installed_antivirus {
                    cps_v.push(name);
                }
                ticket.current_antivirus = Some(cps_v);
            }

            let sales_rep = data.sales_rep.unwrap_or_default();
            let split_rep = data.split_rep.unwrap_or_default();
            let email = sales_rep.email.split_once("@").clone().unwrap_or(("", "")).0.to_string();
            let email_split_rep = split_rep.email.split_once("@").clone().unwrap_or(("", "")).0.to_string();

            for msg in data.customer_messages{
                task_notes.push(TaskNotePayload{
                    everest_initials: msg.id_employee,
                    note: msg.message,
                    ..Default::default()
                })
            }

            customer.id = data.customer.id;
            customer.cust_code = data.customer.cust_code;
            customer.email = data.customer.email;
            customer.name = data.customer.name.clone();
            customer.phone_number = data.customer.phone_number;
            computer.customer = customer.id.clone();
            ticket.salesman = email_split_rep;
            ticket.tech = email;
            ticket.customer = customer.id.clone();
            ticket.computer = computer.id.clone();
            ticket.hardware_test_results = HardwareTests{ hdd_test, ssd_test, ram_test };
            ticket.doc_alias = data.order.order_type_name.unwrap_or(String::new());

            ticket.id = Some(TicketId(Thing::from((TICKET_TABLE.to_string(), ticket.service_number.clone()))));
            if let Some(computer_id) = computer.id.clone() {
                owned_computers.push(computer_id);
            }
            // customer.computers = Some(owned_computers);
            if let Some(ticket_id) = &ticket.id {
                services.push(ticket_id.clone());
            }

            if let Some(service) = service_details{
                if service.len() == 1{
                    let svc = service.get(0);
                    if let Some(service) = svc {
                        ticket.checkin_notes = service.check_in_notes.clone();
                    }
                }else{
                    info!("Theres a couple.... {:?}", service);
                }
            }

            self.context.output_text += &serde_json::to_string_pretty(&ticket).unwrap_or("".to_string());
            self.context.output_text += &serde_json::to_string_pretty(&customer).unwrap_or("".to_string());
            self.context.output_text += &serde_json::to_string_pretty(&computer).unwrap_or("".to_string());
        }

        if let Ok(_connected_clients) = self.context.connected_clients_rx.try_recv(){
            //     info!("Connected clients: {:#?}", connected_clients.clone());
        }

        if let Ok(users) = self.context.store_users_rx.try_recv(){
            self.context.store_users = Some(users);
        }

        if let Ok(tasks) = self.context.initial_tasks_rx.try_recv(){
            self.context.task_payload = Some(tasks);
        }
        
        if let Ok(action) = self.context.ui_actions_rx.try_recv(){
            match action{
                TaskUiActions::OpenTaskModal(task) => {
                    let task_modal = if let Some(notes) = &task.task_note{
                        let chat_modal = ChatView::new(notes.clone(), self.context.current_user.as_ref().unwrap().clone(), task.id.clone().unwrap());
                        TaskModal::new(chat_modal, task.clone())
                    }else{ TaskModal::new(ChatView::default(), task.clone()) };
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

        if let Ok(keys) = self.context.cps_keys_rx.try_recv(){
            if keys.webroot_key.contains("Error"){
                let toast = &mut self.context.toasts;
                self.context.output_text = "Error fetching Keys. Is SW\\/PCLCPS\\/O on ticket?".to_string();
                let error_toast = Toast{
                    kind: ToastKind::Error,
                    text: "Error fetching Keys. Is SW\\/PCLCPS\\/O on ticket?".into(),
                    options: ToastOptions::default()
                        .show_progress(true)
                        .duration_in_seconds(6.0)
                };
                toast.add(error_toast);
            }
            self.context.keys = keys;
        }

        match &self.state{
            app_state::AppState::Authenticated(page) => {
                match page{
                    app_state::MainPages::Tasks => self.main_page(ctx),
                    app_state::MainPages::Downloads => self.main_page(ctx),
                    app_state::MainPages::WebConsole => self.main_page(ctx),
                }
            },
            app_state::AppState::NoAuth(_reason) => self.main_page(ctx),
                // self.login_page(ctx, self.context.db_tx.clone(), self.context.app_state_tx.clone()),
            app_state::AppState::Login => self.login_page(ctx, self.context.db_tx.clone(), self.context.app_state_tx.clone()),
            _ => {}
        }

        self.context.handle_modals(ctx);
        self.context.toasts.show(ctx);
        self.viewport_loader(ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        let id = self.context.client_uuid.clone();
        if let Some(id) = id{
            spawn(async move {
                let res: Result<Option<Record>, surrealdb::Error> = DATABASE
                    .query("UPDATE connected_client SET connected = false WHERE id == $id")
                    .bind(("id", id.clone()))
                    .await
                    .unwrap().take(0);

                match res{
                    Ok(data) => info!("Disconnected. {data:?}"),
                    Err(e) => info!("Error Creating Client: {e:?}"),
                }
            });
        }
    }
}

// #[cfg(not(feature = "compat_mode"))]
#[tokio::main]
async fn main() -> eframe::Result<()> {
    // console_subscriber::init();
    // Init the logger
    // Configure log level and log file
    // builder().init().unwrap(); 
    let log_level = LevelFilter::Info; 
    let log_file = File::create("output.log").unwrap();
    WriteLogger::init( 
        log_level,
        Config::default(),
        log_file
    ).unwrap();

    eframe::run_native(
        format!("Mastertech-{}", env!("CARGO_PKG_VERSION")).as_str(),
        eframe::NativeOptions {
            viewport: ViewportBuilder::default().with_inner_size([945.0, 750.0])
                .with_drag_and_drop(true).with_icon(load_icon()),
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(MasterTechApp::new(cc)))),
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
    custom_style.interaction.selectable_labels = false;
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

pub fn get_store_users(tx: Sender<Vec<User>>, store: Store) {
    spawn(async move {
        DATABASE.set("store", store).await.unwrap();
        let data: Vec<User> = DATABASE.query("SELECT name, store, everest_initials, id, email FROM user WHERE store == $store")
            .await
            .unwrap()
            .take(0)
            .unwrap();
        
        match tx.try_send(data) {
            Ok(_) => info!("Sent Data from querying tasks"),
            Err(e) => error!("Error sending Task Data: {e:?}")
        };
    });
}

pub fn get_tasks(tx: Sender<Vec<TaskPayload>>){
    spawn(async move {

        let query = format!("SELECT * FROM task FETCH service_ticket, service_ticket.computer, service_ticket.customer, task_note");

        let query_results: Result<Vec<TaskPayload>, surrealdb::Error> = DATABASE
            .query(query)
            .await
            .unwrap()
            .take(0);

        match query_results {
            Ok(data) => {
                match tx.try_send(data){
                    Ok(_) => drop(tx),
                    Err(e) => error!("Error sending Task Data: {e:?}")
                }
            },
            Err(e) => error!("Error unwrapping data: {e:?}"),
        }
    });
}

pub(crate) fn load_icon() -> IconData {
	let (icon_rgba, icon_width, icon_height) = {
		let icon = include_bytes!("assets/masterlogoV2.ico");
		let image = image::load_from_memory(icon)
			.expect("Failed to open icon path")
			.into_rgba8();
		let (width, height) = image.dimensions();
		let rgba = image.into_raw();
		(rgba, width, height)
	};
	
	eframe::egui::IconData {
		rgba: icon_rgba,
		width: icon_width,
		height: icon_height,
	}
}