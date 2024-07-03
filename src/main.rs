#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use std::{fs::File, sync:: Arc};
use crossbeam::channel::Sender;
use log::{debug, error, info};
use app_state::{AppState, MasterTechApp};
use pages::login_page::HASH;
use ratframe::NewCC;
use simplelog::{WriteLogger, Config, LevelFilter};
use eframe::egui::{style::Style, Color32, Context, FontId, IconData, Stroke, Vec2, ViewportBuilder};
use self_update::cargo_crate_version;
use database::{database::Database, schema::{ComputerData, Store, TaskPayload, TicketData, User, COMPUTER_TABLE, CONNECTED_CLIENT_TABLE}, PreTicketData};
use egui_aesthetix::{themes::CarlDark, Aesthetix};
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
        
        if self.context.connect_to_ws || self.context.disconnect_ws{
            let socket_disconnect = self.context.disconnect_ws.clone();
            info!("Socket_disconnect: {:?}", socket_disconnect);
            // tokio::spawn(async move{
                // let _x = WebSocket::new_websocket_connection(uuid.clone(), socket_disconnect).await;
            // });

            // self.context.output_text += &x;
            self.context.connect_to_ws = false;
            self.context.disconnect_ws = false;
        }

        if self.context.specs_first_run{
            self.context.specs_first_run = false;

            let loaded_data = load_encrypted_user_data(HASH);
            match loaded_data{
                Some(login) => {
                    self.state = AppState::Authenticated(app_state::MainPages::Tasks);
                    let tx = self.context.db_tx.clone();
                    let sysinfo_tx = self.context.computer_specs_tx.clone();
                    spawn(async move {
                        tx.try_send(
                            Database::new(login.username, login.password, None).await
                        ).unwrap();
                        let _ = ComputerData::get_computer_data(sysinfo_tx).await.unwrap_or(());
                    });

                    #[cfg(target_os="windows")]
                    {
                        let mut cps = self.context.current_antivirus.clone();
            
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
                None => { self.state = AppState::NoAuth("No User returned from decryption phase".to_string()); },
            }
        }
        

        if let Ok(computer_data) = self.context.computer_specs_rx.try_recv(){
            self.context.system_info = computer_data;
            for disk in &self.context.system_info.drives{
                self.context.disk_num += 1;
                if let Some(disks_arr) = self.context.disks.as_array_mut() {
                    let disk_json = serde_json::to_value(&disk).unwrap();
                    disks_arr.push(disk_json);
                } else { debug!("Expected self.context.drives to be an Array"); }
            }
            self.context.output_text += format!("{:#?}", &self.context.system_info.seb_info.as_mut()).as_str();
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
                // info!("No auth: {reason}");
                self.login_page(ctx, self.context.db_tx.clone(), self.context.app_state_tx.clone());
            },
            _ => {}
        }

        while let Ok(message) = self.context.rx.try_recv() {
            if let Ok(info) = serde_json::from_str::<database::PreTicketData>(&message) {
                self.context.output_text.clear();
    
                // Handle PreTicketData
                self.context.ticket_info = info;
                debug!("ticket information: {:#?}", self.context.ticket_info);

                if self.context.ticket_info.checkin_rep  == "DMK"{self.context.salesman = self.context.ticket_info.checkin_rep.clone();}
                else if self.context.ticket_info.checkin_rep  == "JDH2"{self.context.salesman = self.context.ticket_info.checkin_rep.clone();}
                self.context.technician = self.context.ticket_info.sales_rep.clone();

                let code = &self.context.ticket_info.cust_code;
                let email = &self.context.ticket_info.customer_email;
                let codes = &self.context.ticket_info.item_codes;
                let store = &self.context.ticket_info.jurisdiction;

                self.context.output_text += &format!("Store: {store:?}\n\nCustomer Code: {code}\nCustomer Email: {email}\n\nItem on order:\n{codes}");
                self.context.spinner = false;
    
            }             
            else if let Ok(info) = serde_json::from_str::<database::GetKeysResponse>(&message) {
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
            #[cfg(target_os="windows")]
            {
                let cps = &mut self.context.current_antivirus;
                let installed_antivirus = ComputerData::get_antivirus()
                .map_err(|e| 
                    *cps += format!("Error checking antivirus: {e}\n").as_str()
                ).unwrap_or_default();
    
    
                for (name, is_installed) in installed_antivirus {
                    match is_installed {
                        Some(true) => {
                            *cps += "\n";
                            *cps += &format!("{name}");
                        },
                        _ => {},
                    }
                }
            }

            self.context.output_text = serde_json::to_string(&data).unwrap();
            
            let sales_rep = data.sales_rep.unwrap_or_default();
            let split_rep = data.split_rep.unwrap_or_default();
            let email = sales_rep.email.split_once("@").clone().unwrap_or(("!! Getting Tech", "")).0.to_string();
            let email_split_rep = split_rep.email.split_once("@").clone().unwrap_or(("!! Getting Salesman", "")).0.to_string();
            self.context.technician = email_split_rep;
            self.context.technician = email;
            // self.context.ticket_info.customer_name = data.customer.name.clone();
            // self.context.ticket_info.customer_phone_1
            let service_details = data.order.associations.order_service;
            let mut checkin_notes = String::new();

            if let Some(service) = service_details{
                if service.len() == 1{
                    let svc = service.get(0).unwrap();
                    checkin_notes = svc.check_in_notes.clone();
                    // svc.intake_notes
                }else{
                    info!("Theres a couple.... {:?}", service);
                }
            }

            let cust = data.customer.clone();

            let ticket = TicketData{
                customer: cust.id.clone(),
                service_number: self.context.so_number.clone(),
                sales_rep: data.order.id_employee_sales_rep.clone(),
                recommendations: self.context.recommendations.clone(),
                tech: self.context.technician.clone(),
                salesman: self.context.salesman.clone(),
                dep: data.order.id_store.clone(),
                ticket_total: data.order.total_paid.clone(),
                doc_alias: data.order.order_type.clone(),
                // hardware_test_results: self.context.,
                // #[cfg(target_os="windows")]
                // current_antivirus: Some(self.context.current_antivirus),
                ..Default::default()
            };
            
            self.context.ticket_payload = Some(ticket);

            // let cust = data.customer.clone();
            // self.context.ticket_info.customer_name
            info!("CUSTOMER DATA {:#?}", data.customer.clone());
            self.context.ticket_info = PreTicketData {
                cust_code: cust.cust_code,
                cust_id: cust.id,
                sales_rep: data.order.id_employee_sales_rep,
                due_date: Some(self.context.date.unwrap_or_default().to_string()),
                doc_alias: data.order.order_type,
                // dep: Store::RIV,
                jurisdiction: data.order.id_store,
                ticket_total: data.order.total_paid,
                customer_name: cust.name,
                customer_phone_1: cust.phone_number,
                customer_phone_2: cust.phone_number_2,
                customer_email: cust.email,
                checkin_notes,
                ..Default::default()
            };
        }

        if let Ok(_connected_clients) = self.context.connected_clients_rx.try_recv(){
            //     info!("Connected clients: {:#?}", connected_clients.clone());
        }

        if let Ok(users) = self.context.store_users_rx.try_recv(){
            self.context.store_users = Some(users);
        }

        if let Ok(tasks) = self.context.initial_tasks_rx.try_recv(){
            self.context.ticket_data = Some(tasks);
        }
        self.viewport_loader(ctx);
    }
}

// #[cfg(not(feature = "compat_mode"))]
#[tokio::main]
async fn main() -> eframe::Result<()> {
    puffin::set_scopes_on(true);
    
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
        format!("Mastertech-{}",cargo_crate_version!()).as_str(),
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

// #[cfg(feature = "compat_mode")]
// #[tokio::main]
// async fn main(){
//     puffin::set_scopes_on(true); // Remember to call this, or puffin will be disabled!
//     // cannot run this logger because the minidump module already uses a logger
//     let log_level = LevelFilter::Error; // Configure log level and log file
//     let log_file = File::create("output.log").unwrap();
//     WriteLogger::init( // Init the logger
//         log_level,
//         Config::default(),
//         log_file
//     ).unwrap();

//     let mut app = MasterTechApp::default();
//     run_software(move |ctx| {
//         app.update(&ctx);
//     });
// }

// #[cfg(feature = "compat_mode")]
// impl MasterTechApp{
//     fn update(&mut self, ctx: &Context){}

// #[cfg(all(feature="winit", feature="compat_mode"))]
// fn run_software(mut ui: impl FnMut(&Context) + 'static) {
//     use std::num::NonZeroU32;
//     use skia_safe::{Surface, surfaces};
//     use egui_skia::EguiSkiaWinit;
//     use egui_winit::winit::dpi::LogicalSize;
//     use egui_winit::winit::event::{Event, WindowEvent};
//     use egui_winit::winit::event_loop::{ControlFlow, EventLoop};
//     use egui_winit::winit::window::WindowBuilder;

//     let ev_loop = EventLoop::new();
//     let window = WindowBuilder::new()
//         .with_title(format!("Mastertech-{}",cargo_crate_version!()).as_str())
//         .with_inner_size(LogicalSize::new(925.0, 740.0))
//         .build(&ev_loop)
//         .unwrap();

//     let context = unsafe { softbuffer::Context::new(&window) }.unwrap();
//     let mut softbuffer_surface = unsafe {
//         softbuffer::Surface::new(&context, &window)
//     }.unwrap();
//     let mut egui_skia = EguiSkiaWinit::new(&ev_loop);

//     egui_skia
//         .egui_winit
//         .set_pixels_per_point(window.scale_factor() as f32);

//     let size = window.inner_size();
//     let size = size.to_logical::<i32>(window.scale_factor());
//     let mut surface = surfaces::raster_n32_premul(
//         (size.width, size.height)
//     ).unwrap();

//     ev_loop.run(move |ev, _, control_flow| {
//         *control_flow = ControlFlow::Wait;

//         match ev {
//             Event::WindowEvent {
//                 event: WindowEvent::CloseRequested,
//                 ..
//             } => {
//                 *control_flow = ControlFlow::Exit;
//             }
//             Event::WindowEvent {
//                 event: WindowEvent::Resized(size),
//                 ..
//             } => {
//                 surface = surfaces::raster_n32_premul(
//                     (size.width as i32, size.height as i32)
//                 ).unwrap();
//                 window.request_redraw();
//             }
//             Event::WindowEvent { event, .. } => {
//                 let response = egui_skia.on_event(&event);
//                 if response.repaint {
//                     window.request_redraw();
//                 }
//             }
//             Event::RedrawRequested(window_id) if window_id == window.id() => {
//                 let canvas = surface.canvas();
//                 canvas.clear(skia_safe::Color::TRANSPARENT);

//                 let repaint_after = egui_skia.run(&window, &mut ui);

//                 *control_flow = if repaint_after.is_zero() {
//                     window.request_redraw();
//                     ControlFlow::Poll
//                 } else if let Some(repaint_after_instant) =
//                     std::time::Instant::now().checked_add(repaint_after)
//                 {
//                     ControlFlow::WaitUntil(repaint_after_instant)
//                 } else {
//                     ControlFlow::Wait
//                 };
                
//                 egui_skia.paint(&mut canvas);
                
//                 let snapshot = surface.image_snapshot();
//                 let peek = snapshot.peek_pixels().unwrap();
//                 let pixels: &[u32] = peek.pixels().unwrap();

//                 let (width, height) = {
//                     let size = window.inner_size();
//                     (size.width, size.height)
//                 };
//                 softbuffer_surface
//                     .resize(
//                         NonZeroU32::new(width).unwrap(),
//                         NonZeroU32::new(height).unwrap(),
//                     )
//                     .unwrap();

//                 let mut buffer = softbuffer_surface.buffer_mut().unwrap();
//                 buffer.copy_from_slice(pixels);  // Copy Skia pixels to Softbuffer surface
//                 buffer.present().unwrap();
//             }
//             _ => {}
//         }
//     })
// }


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