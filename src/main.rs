#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use std::{fs::File, sync:: Arc};
use crossbeam::channel::Sender;
use egui_toast::{Toast, ToastKind, ToastOptions};
use futures::FutureExt;
use log::{debug, error, info};
use app_state::{AppState, MasterTechApp};
use pages::login_page::HASH;
// use ratframe::NewCC;
use simplelog::{WriteLogger, Config, LevelFilter};
use eframe::egui::{style::Style, Color32, Context, FontId, Stroke, Vec2, ViewportBuilder};
use database::{database::Database, prestashop_schema::ServiceOrder, schema::{ComputerData, ComputerId, HardwareTests, Store, TaskNotePayload, TaskPayload, TicketId, User, TICKET_TABLE}};
use egui_aesthetix::{themes::CarlDark, Aesthetix};
use surrealdb::sql::Thing;
use tabs::tur_sheet::scaffold::AsanaResponse;
use tokio::spawn;
use utilities::crypto::pass_hash::load_encrypted_user_data;

pub mod app_state;
pub mod tabs;
mod filesystem;
mod database;
pub mod pages;
pub mod viewports;
pub mod utilities;

// #[cfg(not(feature = "compat_mode"))]
impl eframe::App for MasterTechApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // most important part of the whole app.. setting up our styling
        let arc_style = set_style();
        ctx.set_style(arc_style);

        if self.context.specs_first_run{
            self.context.specs_first_run = false;

            let loaded_data = load_encrypted_user_data(HASH);
            match loaded_data{
                Some(login) => {
                    self.state = AppState::Authenticated(app_state::MainPages::Tasks);
                    let tx = self.context.db_tx.clone();
                    let sysinfo_tx = self.context.computer_specs_tx.clone();
                    let x = spawn(async move {
                        match tx.try_send(Database::new(login.username, login.password, None).await){
                            Ok(_) => drop(tx),
                            Err(e) => info!("Error sending specs: {e:?}"),
                        }
                        ComputerData::default().get_computer_data(sysinfo_tx).await
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
        

        if let Ok(computer_data) = self.context.computer_specs_rx.try_recv(){
            self.context.computer_data = computer_data;
            for disk in &self.context.computer_data.drives{
                self.context.disk_num += 1;
                if let Some(disks_arr) = self.context.disks.as_array_mut() {
                    let disk_json = serde_json::to_value(&disk).unwrap_or_default();
                    disks_arr.push(disk_json);
                } else { debug!("Expected self.context.drives to be an Array"); }
            }
            self.context.output_text += format!("{:#?}", &self.context.computer_data.seb_info.as_mut()).as_str();
        };

        if let Ok(db) = self.context.db_rx.try_recv(){
            info!("Received DB connection from thread");
            self.context.specs_first_run = true;
            match db{
                Ok(db) => {
                    self.context.current_user = db.clone().user;
                    self.context.database = Some(db.clone());
                    let initial_tasks_tx = self.context.initial_tasks_tx.clone();
                    if let Some(usr) = db.clone().user{
                        get_store_users(db.clone(), self.context.store_users_tx.clone(), usr.store);
                        get_tasks(db.clone(), initial_tasks_tx);
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

        match &self.state{
            app_state::AppState::Authenticated(page) => {
                match page{
                    app_state::MainPages::Tasks => self.main_page(ctx),
                    app_state::MainPages::Downloads => self.main_page(ctx),
                    app_state::MainPages::WebConsole => self.main_page(ctx),
                }
            },
            app_state::AppState::NoAuth(reason) => {
                let toast = &mut self.context.toasts;
    
                let error_toast = Toast{
                    kind: ToastKind::Error,
                    text: reason.into(),
                    options: ToastOptions::default()
                        .show_progress(true)
                        .duration_in_seconds(6.0)
                };
                toast.add(error_toast);
                self.login_page(ctx, self.context.db_tx.clone(), self.context.app_state_tx.clone());
            },
            _ => {}
        }

        while let Ok(message) = self.context.rx.try_recv() {       
            if let Ok(info) = serde_json::from_str::<database::GetKeysResponse>(&message) {
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
            let task = &mut self.context.task_data;
            let task_notes = &mut self.context.task_notes;
            let computer = &mut self.context.computer_data;

            let hdd_test = format!("{:?}", &self.context.hdd_test_cbox);
            let ram_test = format!("{:?}", &self.context.ram_test_cbox);
            let ssd_test = format!("{:?}", &self.context.ssd_test_cbox);
            
            let service_details = data.order.associations.order_service;
            let mut owned_computers: Vec<ComputerId> = Vec::new();
            let mut services: Vec<TicketId> = Vec::new();

            #[cfg(target_os="windows")]
            {
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
            let email = sales_rep.email.split_once("@").clone().unwrap_or(("!! Getting Tech !!", "")).0.to_string();
            let email_split_rep = split_rep.email.split_once("@").clone().unwrap_or(("!! Getting Salesman !!", "")).0.to_string();

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
            customer.computers = Some(owned_computers);
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

            customer.services = Some(services);

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
        
        self.context.toasts.show(ctx);
        self.viewport_loader(ctx);
    }
}

// #[cfg(not(feature = "compat_mode"))]
#[tokio::main]
async fn main() -> eframe::Result<()> {
    // puffin::set_scopes_on(true);
    
    // Configure log level and log file
    let log_level = LevelFilter::Info; 
    let log_file = File::create("output.log").unwrap();

    // Init the logger
    WriteLogger::init( 
        log_level,
        Config::default(),
        log_file
    ).unwrap();

    eframe::run_native(
        format!("Mastertech-{}", env!("CARGO_PKG_VERSION")).as_str(),
        eframe::NativeOptions {
            viewport: ViewportBuilder::default()
                .with_inner_size([945.0, 750.0])
                .with_drag_and_drop(true)
                .with_icon(load_icon()),
            ..Default::default()
        },
        Box::new(|cc| Box::new(MasterTechApp::new(cc))),
    )
}


fn set_style() -> Arc<Style>{
    let theme = CarlDark;
    let mut custom_style: Style = theme.custom_style();
    let mut font = FontId::default();
    custom_style.spacing.button_padding.x = 2.0;
    custom_style.spacing.button_padding.y = 2.0;
    custom_style.spacing.item_spacing = Vec2::new(5.0, 2.0);
    font.size = 12.0;
    custom_style.override_font_id = Some(font);
    custom_style.spacing.combo_height = 60.0; 
    custom_style.spacing.combo_width = 100.0;
    custom_style.interaction.multi_widget_text_select = false;
    custom_style.interaction.selectable_labels = false;
    custom_style.explanation_tooltips = false;
    custom_style.url_in_tooltip = false;
    custom_style.interaction.interact_radius = 15.0;
    custom_style.interaction.resize_grab_radius_side = 15.0;
    custom_style.interaction.resize_grab_radius_corner = 18.0;
    custom_style.visuals.window_shadow.spread = 8.0;
    custom_style.visuals.window_shadow.blur = 10.0;
    custom_style.visuals.selection.stroke.color =  Color32::BLACK;
    custom_style.visuals.selection.bg_fill = Color32::from_rgb(120, 10, 120);
    // custom_style.visuals.widgets.inactive.bg_fill =  Color32::GOLD;
    custom_style.visuals.widgets.inactive.fg_stroke =  Stroke::new(1.0, Color32::WHITE);
    custom_style.visuals.widgets.inactive.weak_bg_fill =  Color32::from_rgb(20, 20, 25);
    custom_style.visuals.widgets.inactive.bg_stroke =  Stroke::new(1.0, Color32::from_rgb(80, 80, 80));
    custom_style.visuals.widgets.open.bg_fill =  Color32::from_black_alpha(50);
    custom_style.visuals.widgets.open.weak_bg_fill =  Color32::from_black_alpha(50);
    custom_style.visuals.widgets.active.weak_bg_fill =  Color32::from_rgb(30,30,30);
    custom_style.visuals.widgets.hovered.weak_bg_fill =  Color32::TRANSPARENT;
    custom_style.visuals.widgets.hovered.bg_fill =  Color32::from_rgb(12, 12, 12);
    custom_style.visuals.widgets.hovered.bg_stroke =  Stroke::new(1.0, Color32::from_rgb(200, 20, 200));
    let arc_style = Arc::new(custom_style);
    arc_style
}

pub fn get_store_users(db: Database, tx: Sender<Vec<User>>, store: Store) {
    spawn(async move {
        db.database.set("store", store).await.unwrap();
        let data: Vec<User> = db.database
            .query("SELECT name, store, everest_initials, id, email FROM user WHERE store == $store")
            .await
            .unwrap()
            .take(0)
            .unwrap();
        
        match tx.try_send(data){
            Ok(_) => info!("Sent Data from querying tasks"),
            Err(e) => error!("Error sending Task Data: {e:?}")
        };
    });
}

pub fn get_tasks(db: Database, tx: Sender<Vec<TaskPayload>>){
    spawn(async move {

        let query = format!("SELECT * FROM task FETCH service_ticket, service_ticket.computer, service_ticket.customer, task_note");

        let query_results: Result<Vec<TaskPayload>, surrealdb::Error> = db
            .database
            .query(query)
            .await
            .unwrap()
            .take(0);

        match query_results{
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

pub(crate) fn load_icon() -> eframe::egui::IconData {
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