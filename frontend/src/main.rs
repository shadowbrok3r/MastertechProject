// #[war(unused_imports)]
use app_state::{check_authentication, AppState, MtechServer};
use database::schema::Store;
use egui_toast::{Toast, ToastKind, ToastOptions, Toasts};
use log::{error, info};
use ratframe::NewCC;
use utilities::{get_other::get_store_users, get_tasks::{get_completed_tasks, get_my_tasks, get_store_tasks}, handle_live_data::{handle_live_data, listen_tasks}};
use web_time::Instant;
use std::sync::Arc;
use egui::{Align2, FontId, Style, Vec2};
use egui_aesthetix::{themes::CarlDark, Aesthetix};
use utilities::Displayable;

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
            log::debug!("Received update: {update:?}")
        }

        // For updating our Ratatui chart in the RataGuiBackend terminal
        if self.context.last_tick.elapsed() >= self.context.tick_rate {
            self.context.chart_app.on_tick();
            self.context.last_tick = Instant::now();
        }

        // do some setting up in the initial frame of our update loop for 
        // 1. Getting database connection
        if self.context.first_run{ // || or if refresh button is hit
            info!("First run after refresh?");
            self.context.first_run = false;
            let (state, user) = check_authentication(self.context.db_tx.clone());
            self.state = state;
            match user{
                Some(usr) => {
                  self.context.current_user = Some(usr);  
                },
                None => {
                    let mut toast = Toasts::new()
                        .anchor(Align2::CENTER_CENTER, (0.0, 0.0));
                    toast.add(Toast{
                        kind: ToastKind::Error,
                        text: "There was a problem with authentication, you may need to login again".into(),
                        options: ToastOptions::default()
                            .show_progress(true)
                            .duration_in_seconds(3.0)
                    });
                },
            }
        }

        // Retrieve our database connection, and 
        // 2. Requesting some task data
        if let Ok(db) = self.context.db_rx.try_recv(){
            let cookie = wasm_cookies::get("jwt");
            let user_cookie = wasm_cookies::get("user");

            if let Some(cookie) = cookie{
                match cookie{
                    Ok(c) => {
                        
                        self.state = AppState::Authenticated;
                        info!("self.state: {:?}", self.state);
                        if let Some(user) = user_cookie{
                            match user{
                                Ok(usr) => {
                                    info!("Got user cookie! {c:?}");
                                    let user = serde_json::from_str(&usr.as_str()).unwrap();
                                    self.context.current_user = Some(user);
                                },
                                Err(e) => error!("Error with user cookie: {e:?}")
                            }
                        }
                    },
                    Err(e) => error!("Error with cookie: {e:?}")
                }
            }
            
            info!("Are we even here?");
            self.context.database = Some(db.clone());

            // get all of our channel Senders from crossbeam to get user/store/completed tasks, 
            // as well as store users and live task notifications
            let tasks_tx = self.context.tasks_tx.clone();
            let my_tasks_tx = self.context.my_tasks_tx.clone();
            let store_tasks_tx = self.context.store_tasks_tx.clone();
            let completed_tasks_tx = self.context.completed_tasks_tx.clone();
            let store_users_tx = self.context.store_users_tx.clone();

            if let Some(usr) = self.context.current_user.as_ref(){
                get_my_tasks(db.clone(), my_tasks_tx, usr.id.clone());
                get_store_tasks(db.clone(), store_tasks_tx, Store::RIV);
                get_completed_tasks(db.clone(), completed_tasks_tx, Store::RIV);
                get_store_users(db.clone(), store_users_tx, Store::RIV);
                listen_tasks(db.clone(), tasks_tx);
            }
        }

        if let Ok(tasks) = self.context.my_tasks_rx.try_recv(){
            self.context.my_tasks = Some(tasks);
        }

        if let Ok(tasks) = self.context.store_tasks_rx.try_recv(){
            self.context.store_tasks = Some(tasks);
        }

        if let Ok(tasks) = self.context.completed_tasks_rx.try_recv(){
            self.context.completed_tasks = Some(tasks);
        }

        if let Ok(users) = self.context.store_users_rx.try_recv(){
            self.context.store_users = Some(users);
        }

        while let Ok(data) = self.context.tasks_rx.try_recv(){
            if let Some(tasks) = &mut self.context.store_tasks{
                handle_live_data(data, tasks).unwrap();
            }
        }

        if self.context.task_layout.create_task_modal{
            let db = self.context.database.clone();
            let task_layout = self.context.task_layout;
            if let Some(ref mut task_opts) = self.context.task_layout.task_opts{
                if let Some(ref mut tasks) = self.context.my_tasks{
                    task_opts.modal.ui(ctx, |ui, stay_open: &mut bool|{
                        *stay_open = true;
                        // task_layout.
                        for task in tasks.iter_mut(){
                            if let Some(ref db) = db{
                                task.task_modal(ui, db.clone());
                            }
                        }
                    });
                }
            }
        }
    }

    /// Called by the frame work to save state before shutdown.
    fn save(&mut self, storage: &mut dyn eframe::Storage) { 
        eframe::set_value(storage, eframe::APP_KEY, self); 
    }
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

