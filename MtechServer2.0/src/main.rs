use database::live_data::{handle_live_data, handle_live_notes, update_or_insert, update_or_insert_layout};
use surrealdb::Action;
use utilities::{displays::{chats::ChatView, modals::{create_task_modal::CreateTaskModal, task_modal::TaskModal}}, get_data::get_associated_ticket, ModalType, TaskUiActions};
use displays::ui_tools::{carl_dark::{CarlDark, Aesthetix}, toasts::{Toast, ToastKind, ToastOptions}};
use eframe::egui::{Color32, Context, FontId, Frame, Margin, Rounding, Stroke, Style, Vec2, Window};
use app_state::{AppState, MainPages, MtechServer};
use wasm_bindgen_futures::spawn_local;
use eframe::egui::FontFamily;
use std::sync::Arc;
use log::info;

#[cfg(target_arch="wasm32")]
use app_state::check_authentication;

pub mod tabs;
pub mod app_state;
pub mod utilities;
pub mod webworker;
pub mod pages;
pub mod first_run;

impl eframe::App for MtechServer {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // most important part of the whole app.. setting up our styling
        let arc_style = set_style();
        ctx.set_style(arc_style); // let alt_style = set_alternative_style(); ctx.set_style(alt_style);

        let data_update = self.context.data_update.as_mut().unwrap();
        if let Some(items) = data_update.take() { 
            if !items.is_empty() && self.context.file_system.paths.is_empty() {
                info!("Files: {items:?}");
                self.context.file_system.build_file_system(items); 
            }
        }
        
        // let live_data_update = self.context.live_data_update.as_mut().unwrap();
        // if let Some(items) = live_data_update.take() { info!("live_data_update: {:?}", items); }

        // do some initial setting up
        if self.context.first_run{ self.first_run(); }

        // Retrieve our database connection, and 2. Requesting some task data
        if let Ok(db) = self.context.db_rx.try_recv(){
            match db{
                Ok(_db) => {
                    info!("3");
                    self.load_data();
                },
                Err(e) => {
                    info!("6");
                    if e.to_string().contains("Already connected"){
                        info!("7");
                        self.load_data();
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
                        #[cfg(target_arch="wasm32")]{
                            wasm_cookies::delete("jwt");
                            wasm_cookies::delete("user");
                        }
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
    
        if let Ok(action) = self.context.ui_actions_rx.try_recv(){
            match action{
                TaskUiActions::OpenTaskModal(task) => {
                    if let (Some(id), Some(usr)) = (task.id.clone(), self.context.current_user.clone()) {
                        let task_modal = if !task.task_note.is_empty(){
                            let chat_modal = ChatView::new(
                                task.task_note.clone(), 
                                usr, 
                                id,
                                self.context.store_users.clone()
                            );
                            TaskModal::new(chat_modal, task.clone())
                        }else{ 

                            TaskModal::new(ChatView::new(
                                task.task_note.clone(), 
                                usr, 
                                id,
                                self.context.store_users.clone()
                            ), task.clone()) 
                        };
                        self.context.current_modal = ModalType::TaskModal(task_modal);
                        self.context.task_modal_handler.open();
                    }
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
                        let chat_modal = ChatView::new(pld.1.to_owned(), current_user.clone(), pld.0.clone(), self.context.store_users.clone());
                        self.context.current_modal = ModalType::ChatView(chat_modal);
                        self.context.chat_modal_handler.open();
                    }// self.context.chat = ModalType::ChatView(pld);
                }, _ => (),
            }
        }

        if let Ok(new_task) = self.context.live_tasks_rx.try_recv(){
            info!("New Task Update: {:?}", new_task.0);
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
                info!("Inserting Task: {:?}", new_task.0);
                self.context.rerun_filtering_completed = true;
                self.context.rerun_filtering_my_tasks = true;
                self.context.rerun_filtering_store_tasks = true;
                if let Err(e) = handle_live_data(
                    new_task.to_owned(), 
                    &mut self.context.tasks, 
                    None
                ) {
                    info!("Error handling live data: {e:?}");
                }
            }
        }

        if let Ok(channel) = self.context.new_ticket_rx.try_recv() {
            info!("New Ticket Update");
        
            let new_task_id = channel.new_task.1.id.clone().unwrap().0.id;
        
            for layout in self.context.task_layouts.values_mut() {
                for tasks in layout.task_map.values_mut() {
                    for task in tasks.iter_mut() {
                        if task.id.as_ref().unwrap().0.id == new_task_id {
                            info!("\nReplacing {:?}\n with \n{:?}\n", task.task_name.clone(), channel.new_task.1.task_name.clone());
        
                            if let Err(e) = update_or_insert_layout(
                                &mut self.context.tasks,
                                channel.new_task.1.clone(),
                                Some(channel.new_ticket.clone()),
                                task
                            ) {
                                info!("Error updating existing task: {e:?}");
                            } else {
                                self.context.rerun_filtering_my_tasks = true;
                                self.context.rerun_filtering_store_tasks = true;
                                self.context.rerun_filtering_completed = true;
                                info!("Updated existing task");
                            }
                            break;
                        }
                    }
                }
            }
        
            // If no matching task was found in the layouts, add the task to the global context
            if !self.context.tasks.iter().any(|task| task.id.as_ref().unwrap().0.id == new_task_id) {
                if let Err(e) = update_or_insert(
                    &mut self.context.tasks,
                    channel.new_task.1.clone(),
                    Some(channel.new_ticket.clone())
                ) {
                    info!("Error updating existing task: {e:?}");
                } else {
                    self.context.rerun_filtering_my_tasks = true;
                    self.context.rerun_filtering_store_tasks = true;
                    self.context.rerun_filtering_completed = true;
                    info!("Inserted new task");
                }
            }
        }

        // if let Ok(channel) = self.context.new_ticket_rx.try_recv(){
        //     info!("New Ticket Update ");
        //     for (_, layout) in self.context.task_layouts.iter_mut() {
        //         for (_, tasks) in layout.task_map.iter_mut(){ 
        //             for task in tasks.iter_mut(){
        //                 if task.id.clone().unwrap().0.id == channel.new_task.1.id.clone().unwrap().0.id {
        //                     info!("\nReplacing {:?}\n with \n{:?}\n", task.task_name.clone(), channel.new_task.1.task_name.clone());
        //                     match update_or_insert_layout(
        //                         &mut self.context.tasks, 
        //                         channel.new_task.1.clone(), 
        //                     Some(channel.new_ticket.clone()), 
        //                     task
        //                     ){
        //                         Ok(_) => {
        //                             self.context.rerun_filtering_my_tasks = true;
        //                             self.context.rerun_filtering_store_tasks = true;
        //                             self.context.rerun_filtering_completed = true;
        //                             info!("Updated existing task");
        //                         },
        //                         Err(e) => info!("Error updating existing task: {e:?}"),
        //                     }
        //                 } else {
        //                     match update_or_insert(&mut self.context.tasks, channel.new_task.1.clone(), Some(channel.new_ticket.clone())){
        //                         Ok(_) => {
        //                             self.context.rerun_filtering_my_tasks = true;
        //                             self.context.rerun_filtering_store_tasks = true;
        //                             self.context.rerun_filtering_completed = true;
        //                             info!("Updated existing task")
        //                         },
        //                         Err(e) => info!("Error updating existing task: {e:?}"),
        //                     }
        //                 }
        //             }
        //         }
        //     }
        // }

        if let Ok(mut payload) = self.context.notes_rx.try_recv(){
            info!("{:?}", payload);
            self.context.new_note = true;
            if let ModalType::TaskModal(task_modal) = &mut self.context.current_modal{
                handle_live_notes(payload.clone(), &mut task_modal.task).unwrap_or(());
                
                if let Action::Delete = payload.0 {
                    task_modal.chat_view.delete_note(&payload.1);
                } else {
                    task_modal.chat_view.insert_note(&mut payload.1);
                }
            } else if let ModalType::ChatView(chat_view) = &mut self.context.current_modal {
                let task = self.context.tasks.iter_mut().find(|task| task.id == chat_view.task_id );
                if let Some(task) = task{
                    handle_live_notes(payload.clone(), task).unwrap_or(());
                
                    if let Action::Delete = payload.0 {
                        chat_view.delete_note(&payload.1);
                    } else {
                        chat_view.insert_note(&mut payload.1);
                    }
                }
            }
            if let Action::Create = payload.0 {
                if let (Some(id), Some(user)) = (&payload.1.clone().task_id, &self.context.current_user) {
                    if let Some(task) = self.context.tasks.iter()
                        .find(|task| 
                            task.id == Some(id.clone()) && task.assignee == user.id
                    ) {
                        // This should work with ID and not initials
                        if payload.1.everest_initials != user.everest_initials {
                            let toast = &mut self.context.toasts;
                            let new_msg_toast = Toast{
                                kind: ToastKind::Success,
                                text: format!("New Message for {}", task.task_name).into(),
                                options: ToastOptions::default().show_progress(true).duration_in_seconds(6.0)
                            };
                            toast.add(new_msg_toast);
                        }
                    }
                }
            }
        }
        
        if self.context.wants_to_undock {
            for client in self.context.clients.clone() {
                let undock = if let Some(undock) = self.context.undock_client.get(&client.connection_string){
                    undock
                } else { &false };

                if *undock {
                    let color = if client.connected{ Color32::LIGHT_BLUE } else { Color32::LIGHT_RED };
        
                    let column_frame = Frame::default().fill(Color32::from_rgb(12, 12, 14))
                        .inner_margin(Margin::same(4.0)).outer_margin(Margin::symmetric(5.0, 3.0))
                        .rounding(Rounding::same(10.0)).stroke(Stroke::new(1.0, color));

                    Window::new(&client.connection_string).frame(column_frame)
                        .max_size(Vec2::new(700., 400.))
                        .show(ctx, |ui| 
                    {
                        ui.vertical_centered_justified(|ui| {
                            ui.horizontal(|ui| self.context.headers(ui, client.clone()));
                            if let Some(ws_client) = self.context.ws_clients.get_mut(&client.connection_string) {
                                ws_client.show(ui);
                            }
                        });
                    });
                }
            }
        }

        self.receive();
        self.menu_bar(ctx);
        self.context.handle_modals(ctx);
        self.context.toasts.show(ctx);
        
        match &self.state{ // Always checking authentication 
            AppState::Authenticated(MainPages::Tasks) => self.main_page(ctx),
            AppState::Authenticated(MainPages::Downloads) => self.downloads_page(ctx),
            AppState::Authenticated(MainPages::AccountSettings) => self.account_settings_page(ctx, self.context.app_state_tx.clone()),
            AppState::Authenticated(_) => self.main_page(ctx),
            AppState::CreateAccount => self.signup_page(ctx, self.context.db_tx.clone(), self.context.app_state_tx.clone()),
            AppState::NoAuth(reason) => {
                if reason.to_string().contains("Already connected") {
                    info!("Already connected");
                    if self.context.current_user.is_some() {
                        self.load_data();
                    } else { 
                        self.context.first_run = true;
                        self.first_run() 
                    }
                    self.state = AppState::Authenticated(MainPages::Tasks);
                } else {
                    self.login_page(ctx, self.context.db_tx.clone(), self.context.app_state_tx.clone())
                }
            },
        }
    }

    fn persist_egui_memory(&self) -> bool { true }

    fn save(&mut self, _storage: &mut dyn eframe::Storage) { }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) { 
        if let Some(window) = web_sys::window(){
            if let Ok(storage) = window.local_storage(){
                if let Some(storage) = storage{
                    let clear = storage.clear();
                    info!("Clearing storage: {clear:?}");
                }
            }
        }
    }
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
        viewport: eframe::egui::ViewportBuilder::default()
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
    // custom_style.visuals.widgets.open.bg_fill =  Color32::LIGHT_BLUE;
    // custom_style.visuals.widgets.open.weak_bg_fill =  Color32::LIGHT_BLUE;
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
