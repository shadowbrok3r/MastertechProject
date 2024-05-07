#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide output_console window on Windows in release
use std::{fs::File, sync::{atomic::Ordering, mpsc::channel, Arc}};
use github::self_updater;
use log::{debug, info};
use scripting::Scripts;
use crate::handle_api::scaffold;
use context::MasterTechApp;
use simplelog::{WriteLogger, Config, LevelFilter};
use eframe::egui::{style::Style, Button, CentralPanel, Color32, Context, FontId, Frame, Grid, IconData, RichText, Stroke, TopBottomPanel, Vec2, ViewportBuilder, ViewportId, Window};
use egui_dock::{DockArea, Style as DockStyle};
use self_update::cargo_crate_version;
use database::{database::{handle_db_data, Database}, schema, ComputerData};
use egui_aesthetix::{themes::CarlDark, Aesthetix};


mod filesystem;
mod handle_api;
mod context;
pub mod github;
mod minidump;
mod database;
mod scripting;

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
                .with_inner_size([945.0, 560.0])
                .with_drag_and_drop(true)
                .with_icon(load_icon()),
            ..Default::default()
        },
        Box::new(|_cc| Box::<MasterTechApp>::default()),
    )
}

impl eframe::App for MasterTechApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        let theme = CarlDark;
        let mut custom_style: Style = theme.custom_style();
        let mut font = FontId::default();
        custom_style.spacing.button_padding.x = 2.0;
        custom_style.spacing.button_padding.y = 2.0;
        custom_style.spacing.item_spacing = Vec2::new(5.0, 2.0);
        font.size = 12.0;
        custom_style.override_font_id = Some(font);
        custom_style.spacing.combo_height = 60.0; 
        custom_style.spacing.combo_width = 135.0;
        let arc_style = Arc::new(custom_style);
        ctx.set_style(arc_style);
        
        if self.context.connect_to_ws{
            let uuid = self.context.client_uuid;
            tokio::spawn(async move{
                let x = ComputerData::initialize_websocket(uuid.clone()).await;
            });

            // self.context.output_text += &x;
            self.context.connect_to_ws = false;
        }

        if self.context.specs_first_run{
            self.context.specs_first_run = false;
            let (tx, rx) = channel();
            let (db_sender, db_receiver) = channel();
            let (db_data_sender, db_data_receiver) = channel::<schema::TaskPayload>();

            tokio::spawn(async move {
                let system_info = ComputerData::get_computer_data().await;
                let database = Database::new().await;
                let _ = handle_db_data(database.clone(), db_data_sender).await.unwrap();

                match tx.send(system_info.unwrap()){
                    Ok(_) => info!("sent computer data"),
                    Err(e) => info!("Error sending computer data: {e:?}"),
                };

                match db_sender.send(database){
                    Ok(_) => info!("Sent db connection across thread"),
                    Err(err) => debug!("Error sending db connection: {err:?}"),
                }

                
                
            });

            if let Ok(db) = db_receiver.recv(){
                info!("Received DB connection from thread");
                self.context.database = Some(db);
            }

            if let Ok(task_payload) = db_data_receiver.recv(){
                info!("Received task_payload from thread: {task_payload:#?}");
            }

            let specs = match rx.recv(){
                Ok(data) => {
                    info!("Received computer data");
                    Ok(data)
                },
                Err(e) => {
                    info!("Error receiving data: {e}");
                    Err(e)
                },
            };

            match specs{
                Ok(computer_data) => {
                    self.context.system_info = computer_data;

                    for disk in &self.context.system_info.drives{
                        
                        self.context.disk_num += 1;
        
                        if let Some(disks_arr) = self.context.disks.as_array_mut() {
                            // Convert `disk` to a serde_json::Value
                            let disk_json = serde_json::to_value(&disk).unwrap();
                    
                            disks_arr.push(disk_json);
                        } else {
                            eprintln!("Expected self.context.drives to be an Array");
                        }
                        
                    }
                    self.context.output_text += format!("{:#?}", self.context.system_info.clone().seb_info.unwrap_or_default()).as_str();
                },
                Err(e) => {
                    self.context.output_text = format!("{}", e.to_string());
                }
            };

            #[cfg(target_os="windows")]
            {
                let mut cps = self.context.current_antivirus.clone();
    
                let installed_antivirus = ComputerData::get_antivirus()
                .map_err(|e| 
                    cps += format!("Error checking antivirus: {e}\n").as_str()
                ).unwrap();
    
    
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
        }
    
        
        let receiver = self.context.rx.as_ref().unwrap();
        
        while let Ok(message) = receiver.try_recv() {
            if let Ok(info) = serde_json::from_str::<database::PreTicketData>(&message) {
    
                self.context.output_text.clear();
    
                // Handle PreTicketData
                self.context.ticket_info = info;
                debug!("ticket information: {:#?}", self.context.ticket_info);

                if self.context.ticket_info.checkin_rep  == "DMK"{self.context.salesman_cbox = scaffold::Salesman::Danny;}
                else if self.context.ticket_info.checkin_rep  == "JDH2"{self.context.salesman_cbox = scaffold::Salesman::Jake}
    
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
            else if let Ok(info) = serde_json::from_str::<crate::handle_api::AsanaResponse>(&message) { 
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
    

        if self.context.show_deferred_viewport.load(Ordering::Relaxed) {
            let show_deferred_viewport = self.context.show_deferred_viewport.clone();

            ctx.show_viewport_deferred(
                ViewportId::from_hash_of("deferred_viewport"),
                ViewportBuilder::default()
                    .with_title("Deferred Viewport")
                    .with_inner_size([200.0, 100.0]),
                move |ctx, _class| {

                    CentralPanel::default().show(ctx, |ui| {
                        let scripts = Scripts::default();
    
                        let script_names = [
                            scripts.wrsa,
                            scripts.sas,
                            scripts.check_driver,
                            scripts.running_tasks,
                        ];

                        Grid::new("scripts").min_col_width(134.0).num_columns(3).min_row_height(10.0).show(
                            ui, |ui| {
                    
                                let mut counter = 0;  // Initialize a counter
                                for value in script_names{
                                    let button = Button::new(
                                        RichText::new(&value))
                                            .stroke(Stroke::new(1.0, Color32::from_rgb(191, 33, 101)));
                    
                    
                                    if ui.add(button).clicked(){
                    
                                    }
                    
                                    counter += 1;  // Increment the counter
                    
                                    if counter % 4 == 0 {
                                        ui.end_row();  // End the row after every 2 buttons
                                    }
                                }
                            }); // Grid
                    });
                    if ctx.input(|i| i.viewport().close_requested()) {
                        // Tell parent to close us.
                        show_deferred_viewport.store(false, Ordering::Relaxed);
                    }
                },
            );
        }

        TopBottomPanel::top("egui_dock::MenuBar").show(ctx, |ui| {
            eframe::egui::menu::bar(ui, |ui| {
                ui.menu_button("View", |ui| {
                    // allow certain tabs to be toggled
                    for tab in &[
                        &"TUR Sheet".to_string(),
                        &"Scripts".to_string(),
                        &"Console".to_string(),
                        &"System Information".to_string(),
                        &"File Browser 📂".to_string(),
                        &"Minidump Analysis".to_string(),
                        &"Profiler".to_string(),
                        &"QC".to_string(),
                        &"Mastertech Website".to_string(),
                    ] {
                        if ui
                            .selectable_label(self.context.open_tabs.contains(*tab), *tab)
                            .clicked()
                        {
                            if let Some(index) = self.tree.find_tab(&tab.to_string()) {
                                self.tree.remove_tab(index);
                                self.context.open_tabs.remove(*tab);
                            } else {
                                self.tree.push_to_focused_leaf(tab.to_string());
                            }
                            ui.close_menu();
                        }
                    }
                });
            })
        });
    
        CentralPanel::default()// When displaying a DockArea in another UI, it looks better
            .frame(Frame::central_panel(&ctx.style()).inner_margin(4.))// to set inner margins to 0.
            .show(ctx, |ui| {
                let mut style = self.context.style.get_or_insert(DockStyle::from_egui(ui.style())).clone();
                style.overlay.selection_color = Color32::from_rgb(92,0,87);
                style.separator.color_hovered = Color32::from_rgba_premultiplied(50,93,80,77);
                style.separator.color_idle = Color32::from_rgba_premultiplied(17,17,33,5);
                style.separator.color_dragged = Color32::from_rgba_premultiplied(189,189,189,130);
                style.buttons.add_tab_align = egui_dock::TabAddAlign::Left;
                style.main_surface_border_rounding.nw = 15.0;
                style.main_surface_border_rounding.ne = 15.0;
                style.buttons.close_tab_color = Color32::from_rgba_premultiplied(118, 0, 129, 58);
    
                DockArea::new(&mut self.tree)
                    .style(style)
                    .show_close_buttons(self.context.show_close_buttons)
                    .show_add_buttons(self.context.show_add_buttons)
                    .show_add_popup(true)
                    .draggable_tabs(self.context.draggable_tabs)
                    .show_tab_name_on_hover(self.context.show_tab_name_on_hover)
                    .show_inside(ui, &mut self.context);
            });
    }
}

#[cfg(feature = "compat_mode")]
#[tokio::main]
async fn main(){
    puffin::set_scopes_on(true); // Remember to call this, or puffin will be disabled!
    // cannot run this logger because the minidump module already uses a logger
    let log_level = LevelFilter::Error; // Configure log level and log file
    let log_file = File::create("output.log").unwrap();
    WriteLogger::init( // Init the logger
        log_level,
        Config::default(),
        log_file
    ).unwrap();

    let mut app = MasterTechApp::default();
    run_software(move |ctx| {
        app.update(&ctx);
    });
}

#[cfg(feature = "compat_mode")]
impl MasterTechApp{
    fn update(&mut self, ctx: &Context){
        catppuccin_egui::set_theme(ctx, MOCHA);
        if self.context.spinner == true{
            eframe::egui::Window::new("Spinner Window")
            .title_bar(false)
            .fixed_size(vec2(10.0,10.0))
            .anchor(Align2::RIGHT_TOP, [2.0, 2.0])
            .show(&ctx, |ui|{
                ui.add(
                    Spinner::new()
                    .color(Color32::LIGHT_RED)
                    .size(20.0)
                );
            });
            
        }
    
        if self.context.specs_first_run == true{
    
            let specs_sender = self.context.sysinfo_request.tx.clone();
            RetrieveSystemInfo::get_system_specs(specs_sender);
            
            #[cfg(target_os="windows")]
            {
                let mut cps = self.context.antivirus_installed.clone();
                let mut new_out_text = String::new();
    
                let installed_antivirus = RetrieveSystemInfo::get_antivirus()
                .map_err(|e| 
                    new_out_text = format!("Error checking antivirus: {e}\n")
                ).unwrap();
    
    
                for (name, is_installed) in installed_antivirus {
                    match is_installed {
                        Some(true) => {
                            new_out_text += &format!("{name} detected");
                            cps += "\n";
                            cps += &format!("{name}");
                        },
                        _ => {},
                    }
                }
            }
        }
    
        self.context.specs_first_run = false;
        let receiver = self.context.rx.as_ref().unwrap();
        
        while let Ok(message) = receiver.try_recv() {
            if let Ok(info) = serde_json::from_str::<scaffold::PreTicketData>(&message) {
                println!("ticket information: {info:#?}");
                self.context.output_text.clear();
                let checkin_rep = info.checkin_rep;
                self.context.ticket_info.checkin_rep = checkin_rep.clone();
                if checkin_rep == "DMK"{self.context.salesman_cbox = scaffold::Salesman::Danny;}
                else if checkin_rep == "JDH2"{self.context.salesman_cbox = scaffold::Salesman::Jake}
    
                // Handle PreTicketData
                self.context.ticket_info.customer_name = info.customer_name;
                self.context.ticket_info.customer_phone_1 = info.customer_phone_1;
                self.context.ticket_info.customer_phone_2 = info.customer_phone_2;
                self.context.ticket_info.checkin_notes = info.checkin_notes;
    
                self.context.ticket_info.cust_code = info.cust_code;
                self.context.ticket_info.doc_alias = info.doc_alias;
                self.context.ticket_info.department = info.department;
                self.context.ticket_info.jurisdiction = info.jurisdiction;
                self.context.ticket_info.invoice_amnt = info.invoice_amnt;
                self.context.ticket_info.customer_email = info.customer_email;
                self.context.ticket_info.last_invoice_number = info.last_invoice_number;
                self.context.ticket_info.last_invoice_amount = info.last_invoice_amount;
                self.context.ticket_info.total_invoice_count = info.total_invoice_count;
                self.context.ticket_info.item_codes = info.item_codes;
    
                let code = self.context.ticket_info.cust_code.clone();
                let email = self.context.ticket_info.customer_email.clone();
                let codes = self.context.ticket_info.item_codes.clone();
    
                self.context.output_text += &format!("Customer Code: {code}\nCustomer Email: {email}\n\nItem on order:\n{codes}");
                self.context.spinner = false;
    
            }             
            else if let Ok(info) = serde_json::from_str::<scaffold::PulledKeys>(&message) {
                if !info.webroot_key.is_empty() || !info.superanti_key.is_empty(){
                    self.context.keys.webroot_key = info.webroot_key;
                    self.context.keys.superanti_key = info.superanti_key;
                }
                self.context.spinner = false;
            }
            else if let Ok(info) = serde_json::from_str::<system_info::ComputerData>(&message) {
                self.context.hostname = info.hostname;
                self.context.cpu = info.cpu;
                self.context.ram = info.ram;
                self.context.gpu = info.gpu;
                for disk in info.drives.drives{
                    
                    self.context.disk_num += 1;
    
                    if let Some(disks_arr) = self.context.drives.as_array_mut() {
                        // Convert `disk` to a serde_json::Value
                        let disk_json = serde_json::to_value(&disk).unwrap();
                
                        disks_arr.push(disk_json);
                    } else {
                        eprintln!("Expected self.context.drives to be an Array");
                    }
                    
                }
                
            }
            else if let Ok(info) = serde_json::from_str::<request::AsanaResponse>(&message) { 
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
    
        TopBottomPanel::top("egui_dock::MenuBar").show(ctx, |ui| {
            eframe::egui::menu::bar(ui, |ui| {
                ui.menu_button("View", |ui| {
                    // allow certain tabs to be toggled
                    for tab in &[
                        &self.context.tur_sheet_tab, 
                        &self.context.scripts_tab, 
                        &self.context.output_console_tab, 
                        &self.context.system_info_tab, 
                        &self.context.file_browser_tab,
                        &"Minidump Analysis".to_string(),
                        &"Profiler".to_string(),
                    ] {
                        if ui
                            .selectable_label(self.context.open_tabs.contains(*tab), *tab)
                            .clicked()
                        {
                            if let Some(index) = self.tree.find_tab(&tab.to_string()) {
                                self.tree.remove_tab(index);
                                self.context.open_tabs.remove(*tab);
                            } else {
                                self.tree.push_to_focused_leaf(tab.to_string());
                            }
                            ui.close_menu();
                        }
                    }
                });
            })
        });
    
        CentralPanel::default()// When displaying a DockArea in another UI, it looks better
            .frame(Frame::central_panel(&ctx.style()).inner_margin(4.))// to set inner margins to 0.
            .show(ctx, |ui| {
                let mut style = self.context.style.get_or_insert(DockStyle::from_egui(ui.style())).clone();
                style.selection_color = Color32::from_rgb(92,0,87);
                style.separator.color_hovered = Color32::from_rgba_premultiplied(50,93,80,77);
                style.separator.color_idle = Color32::from_rgba_premultiplied(17,17,33,5);
                style.separator.color_dragged = Color32::from_rgba_premultiplied(189,189,189,130);
                style.buttons.add_tab_align = egui_dock::TabAddAlign::Left;
                style.tabs.rounding.nw = 15.0;
                style.tabs.rounding.ne = 15.0;
                style.tabs.text_color_active_focused = Color32::from_rgba_premultiplied(0, 254, 158, 255);
                style.tabs.text_color_active_unfocused = Color32::from_rgba_premultiplied(0, 255, 255, 255);
                style.tabs.text_color_unfocused = Color32::from_rgba_premultiplied(230, 230, 230, 100);
                style.buttons.close_tab_color = Color32::from_rgba_premultiplied(118, 0, 129, 58);
    
                DockArea::new(&mut self.tree)
                    .style(style)
                    .show_close_buttons(self.context.show_close_buttons)
                    .show_add_buttons(self.context.show_add_buttons)
                    .show_add_popup(true)
                    .draggable_tabs(self.context.draggable_tabs)
                    .show_tab_name_on_hover(self.context.show_tab_name_on_hover)
                    .show_inside(ui, &mut self.context);
            });
    } 
}

#[cfg(all(feature="winit", feature="compat_mode"))]
fn run_software(mut ui: impl FnMut(&Context) + 'static) {
    use std::num::NonZeroU32;
    use skia_safe::{Surface, surfaces};
    use egui_skia::EguiSkiaWinit;
    use egui_winit::winit::dpi::LogicalSize;
    use egui_winit::winit::event::{Event, WindowEvent};
    use egui_winit::winit::event_loop::{ControlFlow, EventLoop};
    use egui_winit::winit::window::WindowBuilder;

    let ev_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title(format!("Mastertech-{}",cargo_crate_version!()).as_str())
        .with_inner_size(LogicalSize::new(925.0, 740.0))
        .build(&ev_loop)
        .unwrap();

    let context = unsafe { softbuffer::Context::new(&window) }.unwrap();
    let mut softbuffer_surface = unsafe {
        softbuffer::Surface::new(&context, &window)
    }.unwrap();
    let mut egui_skia = EguiSkiaWinit::new(&ev_loop);

    egui_skia
        .egui_winit
        .set_pixels_per_point(window.scale_factor() as f32);

    let size = window.inner_size();
    let size = size.to_logical::<i32>(window.scale_factor());
    let mut surface = surfaces::raster_n32_premul(
        (size.width, size.height)
    ).unwrap();

    ev_loop.run(move |ev, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match ev {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                *control_flow = ControlFlow::Exit;
            }
            Event::WindowEvent {
                event: WindowEvent::Resized(size),
                ..
            } => {
                surface = surfaces::raster_n32_premul(
                    (size.width as i32, size.height as i32)
                ).unwrap();
                window.request_redraw();
            }
            Event::WindowEvent { event, .. } => {
                let response = egui_skia.on_event(&event);
                if response.repaint {
                    window.request_redraw();
                }
            }
            Event::RedrawRequested(window_id) if window_id == window.id() => {
                let canvas = surface.canvas();
                canvas.clear(skia_safe::Color::TRANSPARENT);

                let repaint_after = egui_skia.run(&window, &mut ui);

                *control_flow = if repaint_after.is_zero() {
                    window.request_redraw();
                    ControlFlow::Poll
                } else if let Some(repaint_after_instant) =
                    std::time::Instant::now().checked_add(repaint_after)
                {
                    ControlFlow::WaitUntil(repaint_after_instant)
                } else {
                    ControlFlow::Wait
                };
                
                egui_skia.paint(canvas);
                
                let snapshot = surface.image_snapshot();
                let peek = snapshot.peek_pixels().unwrap();
                let pixels: &[u32] = peek.pixels().unwrap();

                let (width, height) = {
                    let size = window.inner_size();
                    (size.width, size.height)
                };
                softbuffer_surface
                    .resize(
                        NonZeroU32::new(width).unwrap(),
                        NonZeroU32::new(height).unwrap(),
                    )
                    .unwrap();

                let mut buffer = softbuffer_surface.buffer_mut().unwrap();
                buffer.copy_from_slice(pixels);  // Copy Skia pixels to Softbuffer surface
                buffer.present().unwrap();
            }
            _ => {}
        }
    })
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