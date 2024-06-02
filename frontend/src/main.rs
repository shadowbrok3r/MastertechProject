// #[war(unused_imports)]
use app_state::{check_authentication, AppState, MtechServer};
use egui_toast::{Toast, ToastKind, ToastOptions};
use log::info;
use ratframe::NewCC;
use utilities::{ModalType, get_other::get_store_users, get_tasks::get_tasks, handle_live_data::{handle_live_data, listen_tasks}};
use web_time::Instant;
use std::sync::Arc;
use egui::{FontId, Style, Vec2};
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

        // Always checking authentication.
        match self.state{
            //if auth'd, user shall be allowed
            app_state::AppState::Authenticated => self.main_page(ctx),
            // if no auth, appstate will be login_page
            app_state::AppState::NoAuth => self.login_page(ctx, self.context.db_tx.clone()),
        }

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
    
            match check_authentication(self.context.db_tx.clone()){
                Ok(d) => {
                    self.state = d.0;
                    self.context.current_user = d.1;
                },
                Err(e) => {
                    info!("Error with auth: {e:?}");
                    self.state = AppState::NoAuth;
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
                    let tasks_tx = self.context.tasks_tx.clone();
                    let my_tasks_tx = self.context.my_tasks_tx.clone();
                    // let store_tasks_tx = self.context.store_tasks_tx.clone();
                    // let completed_tasks_tx = self.context.completed_tasks_tx.clone();
                    let store_users_tx = self.context.store_users_tx.clone();

                    if let Some(usr) = self.context.current_user.as_ref(){
                        get_tasks(db.clone(), my_tasks_tx);
                        get_store_users(db.clone(), store_users_tx, usr.store);
                        listen_tasks(db.clone(), tasks_tx);
                        self.state = AppState::Authenticated;
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
                    self.state = AppState::NoAuth;
                }
            }
        }
        
        if let Ok(tasks) = self.context.my_tasks_rx.try_recv(){
            info!("Task payloads: {tasks:?}");
            self.context.my_tasks = Some(tasks);
        }


        if let Ok(users) = self.context.store_users_rx.try_recv(){
            self.context.store_users = Some(users);
        }

        if self.context.task_layouts.values().all(|task_layout| task_layout.show_modal){

            for task_layout in &mut self.context.task_layouts{
                match task_layout.1.modal{
                    ModalType::CreateTaskModal(_) => {
                        let open = &mut task_layout.1.show_modal;
                        self.context.current_modal = task_layout.1.modal.clone();
                        if *open{
                            self.context.create_task_modal_handler.open();
                            *open = false;
                        }
                    },
                    ModalType::TaskModal(_) => {
                        let open = &mut task_layout.1.show_modal;
                        self.context.current_modal = task_layout.1.modal.clone();
                        if *open{
                            self.context.task_modal_handler.open();
                            *open = false;
                        }
                    },
                    _ => (),
                }
            }
        }
        

        while let Ok(ref data) = self.context.tasks_rx.try_recv(){
            for task_layout in self.context.task_layouts.values_mut(){
                handle_live_data(data.to_owned(), &mut task_layout.tasks).unwrap();
            }
        }

        self.context.handle_modals(ctx);

        self.context.toasts.show(ctx);
    }

    /// Called by the frame work to save state before shutdown.
    fn save(&mut self, storage: &mut dyn eframe::Storage) { 
        eframe::set_value(storage, eframe::APP_KEY, self); 
    }
}

// fn parse_ticket_payload(json_data: &Value) -> anyhow::Result<TicketPayload, anyhow::Error> {
    
//     // Extract the main service ticket part
//     let service_ticket = json_data.get("service_ticket").unwrap(); // : TicketData
//     let ticket_payload: TicketPayload = from_value(service_ticket.clone())?;
//     Ok(ticket_payload)
// }


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

// When compiling to web using trunk:
#[cfg(target_arch = "wasm32")]
fn main() {
    use log::LevelFilter;
    eframe::WebLogger::init(LevelFilter::Debug).ok();
    let web_options = eframe::WebOptions::default();

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
    custom_style.spacing.combo_width = 135.0;
    custom_style.interaction.multi_widget_text_select = false;
    custom_style.interaction.selectable_labels = false;
    custom_style.explanation_tooltips = false;
    custom_style.url_in_tooltip = false;
    let arc_style = Arc::new(custom_style);
    arc_style
}

