#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use anyhow::Error;
use app_state::{AppState, MasterTechApp};
use database::{
    schema::{
        buckets::list_buckets,
        utilities::{get_store_users, get_tasks},
        ComputerData, GetKeysResponse, HardwareTests, Record, TaskNotePayload, TICKET_TABLE,
    },
    Database, DATABASE, STORAGE_URL,
};
use displays::ui_tools::{
    carl_dark::{Aesthetix, CarlDark},
    toasts::{Toast, ToastKind, ToastOptions},
};

use eframe::egui::{
    style::{HandleShape, NumericColorSpace, Selection, TextCursorStyle, WidgetVisuals, Widgets},
    Color32, Context, CursorIcon, FontFamily, FontId, IconData, Rounding, Shadow, Stroke, Style,
    Vec2, ViewportBuilder, Visuals,
};

use filesystem::system_info::ComputerInfo;
use log::{debug, error, info};
use pages::login_page::HASH;
use std::sync::{atomic::Ordering, Arc, Condvar, Mutex};
use surrealdb::RecordId;
use tabs::{
    github::get_github_releases, logger::logging::builder, tur_sheet::scaffold::AsanaResponse,
};
use tokio::spawn;
use utilities::{
    crypto::pass_hash::load_encrypted_user_data,
    displays::{
        chats::ChatView,
        modals::{create_task_modal::CreateTaskModal, task_modal::TaskModal},
    },
    ModalType, TaskUiActions,
};
// use simplelog::{Config, WriteLogger};

#[cfg(target_os = "windows")]
extern crate winapi;

#[cfg(feature = "term")]
use terminal_mode::run_terminal_mode;

#[cfg(feature = "term")]
mod terminal_mode;

pub mod app_state;
mod filesystem;
pub mod pages;
pub mod tabs;
pub mod utilities;
pub mod viewports;

impl eframe::App for MasterTechApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // most important part of the whole app.. setting up our styling
        let arc_style = set_darker_style();
        ctx.set_style(arc_style); // let alt_style = set_alternative_style(); ctx.set_style(alt_style);

        if self.context.specs_first_run {
            self.context.specs_first_run = false;
            // let x = std::env::current_exe().unwrap();
            // std::fs::rename( x, "Mastertech1").unwrap();
            let tx = self.context.db_tx.clone();
            let pair = Arc::new((Mutex::new(ComputerData::default()), Condvar::new()));
            let pair_clone = Arc::clone(&pair);
            let github_tx = self.context.github_releases_channel.0.clone();
            let client = self.context.client.clone();
            spawn(async move {
                match ComputerData::default().get_computer_data().await {
                    // sysinfo_tx
                    Ok(data) => {
                        let (lock, cvar) = &*pair_clone;
                        let mut comp_data = lock.lock().unwrap();
                        *comp_data = data;
                        info!("Computer Data: {comp_data:?}");
                        cvar.notify_one();
                    }
                    Err(e) => error!("Error getting specs: {e:?}"),
                }
                match get_github_releases(github_tx, client).await {
                    Ok(_) => info!("Got github releases"),
                    Err(e) => error!("Error getting github releases: {e:?}"),
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
            for disk in &self.context.computer_data.drives {
                self.context.disk_num += 1;
                if let Some(disks_arr) = self.context.disks.as_array_mut() {
                    let disk_json = serde_json::to_value(&disk).unwrap_or_default();
                    disks_arr.push(disk_json);
                } else {
                    debug!("Expected self.context.drives to be an Array");
                }
            }
            if let Some(seb_inf) = &self.context.computer_data.seb_info {
                self.context.output_text += &format!("{:#?}", &seb_inf);
            }

            let loaded_data = load_encrypted_user_data(HASH);
            match loaded_data {
                Some(login) => {
                    self.state = AppState::Authenticated(app_state::MainPages::Tasks);

                    spawn(async move {
                        let db = Database::new(login.username, login.password, None).await;
                        info!("DB: {db:?}");
                        match tx.try_send(db) {
                            Ok(_) => {
                                info!("Sent DB connection");
                                drop(tx)
                            }
                            Err(e) => error!("Error sending specs: {e:?}"),
                        }
                    });

                    // match x.poll_unpin(cx)
                    #[cfg(target_os = "windows")]
                    {
                        let cps = &mut self.context.current_antivirus.clone();
                        let installed_antivirus = ComputerData::get_antivirus()
                            .map_err(|e| {
                                *cps += format!("Error checking antivirus: {e}\n").as_str()
                            })
                            .unwrap_or(Vec::new());

                        for (name, is_installed) in installed_antivirus {
                            match is_installed {
                                Some(true) => {
                                    *cps += "\n";
                                    *cps += &format!("{name}");
                                }
                                _ => {}
                            }
                        }
                    }
                }
                None => {
                    let toast = &mut self.context.toasts;

                    let error_toast = Toast {
                        kind: ToastKind::Error,
                        text: "Could not get login from encoded data".into(),
                        options: ToastOptions::default()
                            .show_progress(true)
                            .duration_in_seconds(6.0),
                    };
                    toast.add(error_toast);
                    self.state =
                        AppState::NoAuth("No User returned from decryption phase".to_string());
                }
            }
        }

        if let Ok(db) = self.context.db_rx.try_recv() {
            info!("Received DB connection from thread");
            self.context.specs_first_run = true;
            match db {
                Ok(db) => {
                    self.context.current_user = db.user.clone();
                    if let Some(usr) = db.user {
                        if let (Some(access_key), Some(secret_key)) =
                            (usr.minio_access_key.clone(), usr.minio_secret_key.clone())
                        {
                            self.context.toolbox.access_key = access_key.clone();
                            self.context.toolbox.secret_key = secret_key.clone();
                            self.context.toolbox.set_user(usr.clone());
                            let minio_tx = self.context.minio_files.0.clone();
                            let name = usr.email.clone();
                            let parsed = name
                                .split_once('@')
                                .unwrap_or_default()
                                .0
                                .to_string()
                                .clone();

                            info!("Getting Minio files");
                            spawn(async move {
                                let list_bucket_res = list_buckets(
                                    STORAGE_URL.to_string(),
                                    access_key,
                                    secret_key,
                                    parsed,
                                )
                                .await;

                                match list_bucket_res {
                                    Ok(files) => {
                                        info!("Got files: {files:?}");
                                        minio_tx.try_send(files).unwrap()
                                    }
                                    Err(e) => error!("Error getting minio files: {e:?}"),
                                }
                            });
                        }
                        let initial_tasks_tx = self.context.initial_tasks_tx.clone();
                        let tx = self.context.store_users_tx.clone();
                        spawn(async move {
                            get_store_users(tx, usr.store).await?;
                            get_tasks(initial_tasks_tx).await?;
                            Ok::<(), Error>(())
                        });
                        self.context.connect(ctx.clone());
                        self.context.show_ws_viewport.store(true, Ordering::Relaxed);
                    }
                }
                Err(e) => {
                    error!("Error with auth: {e:?}");
                    self.state = AppState::NoAuth(e.to_string());
                    self.context.current_user = None;
                }
            }
        }

        if let Ok(releases) = self.context.github_releases_channel.1.try_recv() {
            debug!("Releases: {releases:?}");
            self.context.github_releases = releases;
        }

        while let Ok(message) = self.context.rx.try_recv() {
            if let Ok(info) = serde_json::from_str::<GetKeysResponse>(&message) {
                if !info.webroot_key.is_empty() || !info.superanti_key.is_empty() {
                    self.context.keys = info;
                }
                self.context.spinner = false;
            } else if let Ok(info) = serde_json::from_str::<AsanaResponse>(&message) {
                if let Some(e) = info.status {
                    self.context.output_text = format!("Status Code: {e:#?}");
                };
                self.context.output_text = format!("{:#?}", info.gid);
            } else {
                self.context.output_text = format!("{}", message);
                self.context.spinner = false;
            }
        }
        // TODO
        // Fix this, egui_file doesnt support 0.29 egui yet
        // if let Some(dialog) = &mut self.context.open_file_dialog {
        //     if dialog.show(&ctx).selected() {
        //         if let Some(file) = dialog.path() {
        //             self.context.opened_file = Some(file.to_path_buf());
        //         }
        //     }
        // }

        if let Ok(data) = self.context.prestashop_api_rx.try_recv() {
            let customer = &mut self.context.customer_data;
            let ticket = &mut self.context.ticket_data;
            // let _task = &mut self.context.task_data;
            let task_notes = &mut self.context.task_notes;
            let computer = &mut self.context.computer_data;

            let hdd_test = format!("{:?}", &self.context.hdd_test_cbox);
            let ram_test = format!("{:?}", &self.context.ram_test_cbox);
            let ssd_test = format!("{:?}", &self.context.ssd_test_cbox);

            let service_details = data.order.associations.order_service;
            let mut owned_computers: Vec<RecordId> = Vec::new();
            let mut services: Vec<RecordId> = Vec::new();

            #[cfg(target_os = "windows")]
            {
                let cps = &mut self.context.current_antivirus;
                let installed_antivirus = ComputerData::get_antivirus()
                    .map_err(|e| *cps += format!("Error checking antivirus: {e}\n").as_str())
                    .unwrap_or(Vec::new());
                let x: Vec<String> = installed_antivirus
                    .iter()
                    .map(|cps| {
                        if let Some(true) = cps.1 {
                            cps.0.clone()
                        } else {
                            "Not installed".to_string()
                        }
                    })
                    .collect::<Vec<String>>();
                ticket.current_antivirus = Some(x);
            }

            let sales_rep = data.sales_rep.unwrap_or_default();
            let split_rep = data.split_rep.unwrap_or_default();
            let email = sales_rep
                .email
                .split_once("@")
                .clone()
                .unwrap_or(("", ""))
                .0
                .to_string();
            let email_split_rep = split_rep
                .email
                .split_once("@")
                .clone()
                .unwrap_or(("", ""))
                .0
                .to_string();

            for msg in data.customer_messages {
                task_notes.push(TaskNotePayload {
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
            ticket.hardware_test_results = HardwareTests {
                hdd_test,
                ssd_test,
                ram_test,
            };
            ticket.doc_alias = data.order.order_type_name.unwrap_or(String::new());

            ticket.id = Some(RecordId::from((
                TICKET_TABLE.to_string(),
                ticket.service_number.clone(),
            )));
            if let Some(computer_id) = computer.id.clone() {
                owned_computers.push(computer_id);
            }
            // customer.computers = Some(owned_computers);
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

            self.context.output_text +=
                &serde_json::to_string_pretty(&ticket).unwrap_or("".to_string());
            self.context.output_text +=
                &serde_json::to_string_pretty(&customer).unwrap_or("".to_string());
            self.context.output_text +=
                &serde_json::to_string_pretty(&computer).unwrap_or("".to_string());
        }

        if let Ok(users) = self.context.store_users_rx.try_recv() {
            self.context.store_users = Some(users);
        }

        if let Ok(tasks) = self.context.initial_tasks_rx.try_recv() {
            self.context.task_payload = Some(tasks);
        }

        if let Ok(action) = self.context.ui_actions_rx.try_recv() {
            match action {
                TaskUiActions::OpenTaskModal(task) => {
                    let task_modal = if !task.task_note.is_empty() {
                        let chat_modal = ChatView::new(
                            task.task_note.clone(),
                            self.context.current_user.as_ref().unwrap().clone(),
                            task.id.clone().unwrap(),
                        );
                        TaskModal::new(chat_modal, task.clone())
                    } else {
                        TaskModal::new(ChatView::default(), task.clone())
                    };
                    self.context.current_modal = ModalType::TaskModal(task_modal);
                    self.context.task_modal_handler.open();
                }
                TaskUiActions::CreateTaskModal => {
                    let create_modal =
                        CreateTaskModal::new("Create Task", self.context.store_users.clone());
                    self.context.current_modal = ModalType::CreateTaskModal(create_modal);
                    self.context.create_task_modal_handler.open();
                }
                TaskUiActions::Response(_res) => {}
                TaskUiActions::OpenChatModal(pld) => {
                    info!("Got Chat action");
                    if let Some(current_user) = self.context.current_user.as_ref() {
                        let chat_modal =
                            ChatView::new(pld.1.to_owned(), current_user.clone(), pld.0.clone());
                        self.context.current_modal = ModalType::ChatView(chat_modal);
                        self.context.chat_modal_handler.open();
                    } // self.context.chat = ModalType::ChatView(pld);
                }
                _ => (),
            }
        }

        if let Ok(keys) = self.context.cps_keys_rx.try_recv() {
            if keys.webroot_key.contains("Error") {
                let toast = &mut self.context.toasts;
                self.context.output_text =
                    "Error fetching Keys. Is SW\\/PCLCPS\\/O on ticket?".to_string();
                let error_toast = Toast {
                    kind: ToastKind::Error,
                    text: "Error fetching Keys. Is SW\\/PCLCPS\\/O on ticket?".into(),
                    options: ToastOptions::default()
                        .show_progress(true)
                        .duration_in_seconds(6.0),
                };
                toast.add(error_toast);
            }
            self.context.keys = keys;
        }

        if let Ok(files) = self.context.minio_files.1.try_recv() {
            self.context.toolbox.build_file_system(files);
        }

        if let Ok(state) = self.context.app_state_rx.try_recv() {
            info!("Got a new state: {state:?}");
            self.state = state
        }

        while let Ok(copied_items) = self.context.copied_items_rx.try_recv() {
            // info!("GOT COPIED ITEMS: {copied_items:?}");
            self.context.output_text += &format!("{copied_items}\n");
            // if self.context.output_text.is_empty() {}
        }

        match &self.state {
            app_state::AppState::Authenticated(page) => match page {
                app_state::MainPages::Tasks => self.main_page(ctx),
                app_state::MainPages::Downloads => self.main_page(ctx),
                app_state::MainPages::WebConsole => self.main_page(ctx),
            },
            app_state::AppState::NoAuth(_reason) => self.main_page(ctx),
            app_state::AppState::Login => self.login_page(
                ctx,
                self.context.db_tx.clone(),
                self.context.app_state_tx.clone(),
            ),
            _ => {}
        }

        self.context.handle_modals(ctx);
        self.context.toasts.show(ctx);
        self.viewport_loader(ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        let id = self.context.client_uuid.clone();
        if let Some(id) = id {
            spawn(async move {
                let res: Option<Record> = DATABASE
                    .query("UPDATE connected_client SET connected = false WHERE id == $id")
                    .bind(("id", id.clone()))
                    .await?
                    .take(0)?;

                match res {
                    Some(data) => info!("Disconnected. {data:?}"),
                    None => error!("Error Disconnecting Client"),
                }
                Ok::<(), Error>(())
            });
        }
    }
}

#[tokio::main]
async fn main() -> eframe::Result<()> {
    #[cfg(target_os = "windows")]
    {
        use winapi::um::processthreadsapi::GetCurrentProcess;
        use winapi::um::processthreadsapi::SetPriorityClass;
        use winapi::um::winbase::ABOVE_NORMAL_PRIORITY_CLASS;

        unsafe {
            SetPriorityClass(GetCurrentProcess(), ABOVE_NORMAL_PRIORITY_CLASS);
        }
    }

    // console_subscriber::init();
    // Init the logger
    // Configure log level and log file
    builder().init().unwrap();

    // let log_level = LevelFilter::Info;
    // let log_file = File::create("output.log").unwrap();
    // WriteLogger::init(
    //     log_level,
    //     Config::default(),
    //     log_file
    // ).unwrap();

    #[cfg(feature = "gui")]
    let eframe_app = eframe::run_native(
        format!("Mastertech-{}", env!("CARGO_PKG_VERSION")).as_str(),
        eframe::NativeOptions {
            viewport: ViewportBuilder::default()
                .with_inner_size([945.0, 750.0])
                .with_drag_and_drop(true)
                .with_icon(load_icon()),
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(MasterTechApp::new(cc)))),
    );

    if let Err(e) = eframe_app {
        error!("Error running eframe_native: {e:?} \nswitching to secondary application");
        #[cfg(feature = "term")]
        {
            let res = run_terminal_mode();
            if let Err(e) = res {
                error!("Error running terminal app: {e:?}");
            }
        }
    }

    Ok(())
}

// #[cfg(feature = "term")]
// #[tokio::main]
// async fn main() -> eframe::Result<()> {
//     let res = run_terminal_mode();
//     if let Err(e) = res {
//         error!("Error running terminal app: {e:?}");
//     }
//     Ok(())
// }

fn set_darker_style() -> Arc<Style> {
    // Define colors based on "Tokyo Night Dark" theme
    let background_color = Color32::from_rgb(10, 10, 13); // Editor background
    let foreground_color = Color32::from_rgb(169, 177, 214); // Editor foreground
    let widget_bg_color = Color32::from_rgb(20, 20, 22); // Background for inactive widgets
    let hovered_bg_color = Color32::from_rgb(35, 35, 40); // Background for hovered widgets
    let active_bg_color = Color32::from_rgb(28, 28, 28); // Background for active widgets
    let border_color = Color32::from_rgb(16, 16, 23); // Border color for windows and panels
    let text_color = Color32::from_rgb(199, 202, 245); // Default text color
    let error_color = Color32::from_rgb(227, 104, 176); // Error text color
    let warn_color = Color32::from_rgb(155, 104, 227); // Warning text color
    let link_color = Color32::from_rgb(155, 104, 227); // Hyperlink color

    let theme = CarlDark; // Assuming a theme object or struct
    let mut custom_style: Style = theme.custom_style();

    // Font settings
    let mut font = FontId::default();
    font.size = 10.5;
    font.family = FontFamily::Proportional;

    // Assign custom font
    custom_style.override_font_id = Some(font);

    // Adjust spacing and interactions
    custom_style.spacing.button_padding = Vec2::new(3.0, 3.0);
    custom_style.spacing.item_spacing = Vec2::new(2.0, 1.0);
    custom_style.spacing.combo_height = 55.0;
    custom_style.spacing.combo_width = 100.0;
    custom_style.interaction.selectable_labels = true;
    custom_style.interaction.interact_radius = 10.0;

    // Define visuals with updated values
    custom_style.visuals = Visuals {
        dark_mode: true,                       // Set for dark mode
        override_text_color: Some(text_color), // Global text color override
        widgets: Widgets {
            noninteractive: WidgetVisuals {
                bg_fill: widget_bg_color,
                weak_bg_fill: widget_bg_color,
                bg_stroke: Stroke::new(1.0, Color32::from_rgb(50, 50, 60)),
                rounding: Rounding::same(4.0),
                fg_stroke: Stroke::new(1.0, foreground_color),
                expansion: 0.0,
            },
            inactive: WidgetVisuals {
                bg_fill: widget_bg_color,
                weak_bg_fill: Color32::from_rgb(18, 18, 20),
                bg_stroke: Stroke::new(1.0, Color32::from_rgb(80, 80, 80)),
                rounding: Rounding::same(4.0),
                fg_stroke: Stroke::new(1.0, text_color),
                expansion: 0.0,
            },
            hovered: WidgetVisuals {
                bg_fill: hovered_bg_color,
                weak_bg_fill: Color32::from_rgb(40, 40, 45),
                bg_stroke: Stroke::new(0.5, Color32::from_rgba_premultiplied(120, 20, 120, 100)),
                rounding: Rounding::same(4.0),
                fg_stroke: Stroke::new(1.0, link_color), // Highlight text in link color
                expansion: 0.1,
            },
            active: WidgetVisuals {
                bg_fill: active_bg_color,
                weak_bg_fill: Color32::from_rgb(28, 28, 28),
                bg_stroke: Stroke::new(1.0, Color32::from_rgb(90, 90, 100)),
                rounding: Rounding::same(4.0),
                fg_stroke: Stroke::new(1.0, foreground_color), // Active widget text
                expansion: 0.1,
            },
            open: WidgetVisuals {
                bg_fill: Color32::from_rgb(30, 30, 35),
                weak_bg_fill: Color32::from_rgb(35, 35, 40),
                bg_stroke: Stroke::new(1.0, Color32::from_rgb(100, 100, 110)),
                rounding: Rounding::same(4.0),
                fg_stroke: Stroke::new(1.0, foreground_color), // Open widget text
                expansion: 0.1,
            },
        },
        selection: Selection {
            bg_fill: Color32::from_rgba_premultiplied(90, 55, 88, 90), // Selection background
            stroke: Stroke::new(1.0, Color32::from_rgba_premultiplied(81, 92, 126, 50)), // Selection border
        },
        hyperlink_color: link_color,                   // Hyperlink color
        faint_bg_color: Color32::from_rgb(20, 20, 25), // Subtle background elements
        extreme_bg_color: Color32::from_rgb(15, 15, 20), // Very dark background for contrast
        code_bg_color: Color32::from_rgb(20, 20, 27),  // Background for code blocks
        warn_fg_color: warn_color,                     // Warning text color
        error_fg_color: error_color,                   // Error text color
        window_rounding: Rounding::same(4.0),
        window_shadow: Shadow::default(),
        window_fill: background_color,
        window_stroke: Stroke::new(1.0, border_color), // Window border
        window_highlight_topmost: true,
        menu_rounding: Rounding::same(4.0),
        panel_fill: background_color,
        popup_shadow: Shadow::default(),
        resize_corner_size: 10.0,
        text_cursor: TextCursorStyle::default(),
        clip_rect_margin: 5.0,
        button_frame: true,
        collapsing_header_frame: true,
        indent_has_left_vline: true,
        striped: true,
        slider_trailing_fill: true,
        handle_shape: HandleShape::Circle,
        interact_cursor: Some(CursorIcon::PointingHand),
        image_loading_spinners: true,
        numeric_color_space: NumericColorSpace::Linear, // How numeric values are displayed
    };

    Arc::new(custom_style)
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
