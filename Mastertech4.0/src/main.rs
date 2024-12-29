#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use app_state::{AppState, MainPages, MasterTechApp};
use displays::ui_tools::theme_config::set_custom_style;

use eframe::egui::{
    Context, IconData, ViewportBuilder, Window,
};

use egui_dock::DockState;
use log::{error, info};
// use tabs::logger::logging::builder;

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
pub mod first_run;
pub mod data;

impl eframe::App for MasterTechApp {
    fn update(&mut self, ctx: &Context, frame: &mut eframe::Frame) {
        // most important part of the whole app.. setting up our styling
        if self.context.shared_ctx.modify_theme {
            Window::new("Theme Mods").max_height(600.).title_bar(true).show(ctx, |ui| {
                let theme = self.context.shared_ctx.theme_config.edit_ui(ui, self.context.shared_ctx.settings_sender.clone());
                if theme.0 {
                    if let Some(user) = self.context.shared_ctx.current_user.clone().as_mut() {
                        user.user_settings.color_scheme = serde_json::to_value(theme.1.clone()).unwrap();
                        if let Some(storage) = frame.storage_mut() {
                            storage.set_string("user_settings", serde_json::to_string(&user.user_settings).unwrap_or_default());
                        }
                    }
                    self.context.shared_ctx.theme_config = theme.1;
                    self.context.shared_ctx.modify_theme = false;
                }
            });
        }

        let custom_style = set_custom_style(&self.context.shared_ctx.theme_config);
        ctx.set_style((*custom_style).clone());

        if self.context.first_run { self.first_run(); }

        // Get User settings from local storage
        if let Some(user) = &self.context.shared_ctx.current_user {
            if self.context.get_settings {
                self.context.get_settings = false;
                match serde_json::from_value::<DockState<String>>(user.user_settings.ui_layout.mastertech.clone()){
                    Ok(tree) => self.tree = tree,
                    Err(e) => info!("Could not get UI layout from user: {e:?}"),
                }
            } 
        }
        
        self.context.shared_ctx.receive_ui_action();
        self.context.shared_ctx.receive_prestashop();
        self.context.shared_ctx.receive_task();
        self.context.shared_ctx.receive_ticket();
        self.context.shared_ctx.receive_notes();
        self.context.shared_ctx.receive_notification();
        self.context.shared_ctx.receive_inventory();
        self.context.shared_ctx.handle_modals(ctx);
        self.context.shared_ctx.toasts.show(ctx);
        self.context.shared_ctx.receive(frame, ctx);
        self.context.shared_ctx.handle_viewports(ctx);
        self.receive_database(ctx);
        self.receive(ctx);
        self.receive_github();
        self.viewport_loader(ctx);
        self.menu_bar(ctx);

        match &self.state {
            app_state::AppState::Authenticated(page) => match page {
                app_state::MainPages::Tasks => self.main_page(ctx),
                _ => {}
            },
            app_state::AppState::NoAuth(reason) => {
                if reason.to_string().contains("Already connected") {
                    info!("Already connected");
                    if self.context.shared_ctx.current_user.is_some() {
                        info!("Am i even loading data?");
                        self.load_data(ctx);
                        let _ = self.context.app_state_tx.try_send(AppState::Authenticated(MainPages::Tasks));
                    } else {
                        self.context.first_run = true;
                        self.first_run();
                        let _ = self.context.app_state_tx.try_send(AppState::Authenticated(MainPages::Tasks));
                    }
                } else {
                    let _ = self.context.app_state_tx.try_send(AppState::Login);
                }
            },
            app_state::AppState::Login => self.login_page(
                ctx,
                self.context.db_tx.clone(),
                self.context.app_state_tx.clone(),
            ),
            _ => {}
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
    // builder().init().unwrap();

    let log_level = log::LevelFilter::Info;
    let log_file = std::fs::File::create("output.log").unwrap();
    simplelog::WriteLogger::init(
        log_level,
        simplelog::Config::default(),
        log_file
    ).unwrap();

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
