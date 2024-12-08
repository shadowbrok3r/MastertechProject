#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use app_state::{AppState, MainPages, MasterTechApp};
use displays::ui_tools::theme_config::set_custom_style;

use eframe::egui::{
    Context, IconData, ViewportBuilder, Window,
};

use log::{error, info};
use tabs::logger::logging::builder;

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
                // info!("Settings: {:?}", self.context.theme_config);
                let theme = self.context.shared_ctx.theme_config.edit_ui(ui);
                if theme.0 {
                    if let Some(user) = self.context.shared_ctx.current_user.clone().as_mut() {
                        let user_settings = user.user_settings.as_mut().unwrap();
                        user_settings.color_scheme = serde_json::to_value(theme.1.clone()).unwrap();
                        if let Some(storage) = frame.storage_mut() {
                            storage.set_string("user_settings", serde_json::to_string(&user_settings).unwrap_or_default());
                        }
                    }
                    self.context.shared_ctx.theme_config = theme.1;
                    self.context.shared_ctx.modify_theme = false;
                }
            });
        }

        let custom_style = set_custom_style(&self.context.shared_ctx.theme_config);
        ctx.set_style((*custom_style).clone());

        if self.context.first_run {
            self.context.first_run = false;
            self.first_run();
        }

        self.receive_database(ctx);
        self.receive(ctx);
        self.receive_github();
        self.receive_inventory();
        
        self.context.shared_ctx.receive_ui_action();
        self.context.shared_ctx.receive_prestashop();
        self.context.shared_ctx.receive_task();
        self.context.shared_ctx.receive_ticket();
        self.context.shared_ctx.receive_notes();
        self.context.shared_ctx.receive_notification();

        match &self.state {
            app_state::AppState::Authenticated(page) => match page {
                app_state::MainPages::Tasks => self.main_page(ctx),
                app_state::MainPages::Downloads => self.main_page(ctx),
                app_state::MainPages::WebConsole => self.main_page(ctx),
            },
            app_state::AppState::NoAuth(reason) => {
                if reason.to_string().contains("Already connected") {
                    info!("Already connected");
                    if self.context.shared_ctx.current_user.is_some() {
                        self.load_data(ctx);
                    } else {
                        self.context.first_run = true;
                        self.first_run()
                    }
                    self.state = AppState::Authenticated(MainPages::Tasks);
                } else {
                    self.login_page(
                        ctx,
                        self.context.db_tx.clone(),
                        self.context.app_state_tx.clone(),
                    )
                }
            },
            app_state::AppState::Login => self.login_page(
                ctx,
                self.context.db_tx.clone(),
                self.context.app_state_tx.clone(),
            ),
            _ => {}
        }
        
        self.context.shared_ctx.handle_modals(ctx);
        self.context.shared_ctx.toasts.show(ctx);
        self.viewport_loader(ctx);
    }

    // fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
    //     let id = self.context.client_uuid.clone();
    //     if let Some(id) = id {
    //         spawn(async move {
    //             let res: Option<Record> = DATABASE
    //                 .query("UPDATE connected_client SET connected = false WHERE id == $id")
    //                 .bind(("id", id.clone()))
    //                 .await?
    //                 .take(0)?;
    //
    //             match res {
    //                 Some(data) => info!("Disconnected. {data:?}"),
    //                 None => error!("Error Disconnecting Client"),
    //             }
    //             Ok::<(), Error>(())
    //         });
    //     }
    // }
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

// fn set_darker_style() -> Arc<Style> {
//     // Define colors based on "Tokyo Night Dark" theme
//     let background_color = Color32::from_rgb(10, 10, 13); // Editor background
//     let foreground_color = Color32::from_rgb(169, 177, 214); // Editor foreground
//     let widget_bg_color = Color32::from_rgb(20, 20, 22); // Background for inactive widgets
//     let hovered_bg_color = Color32::from_rgb(35, 35, 40); // Background for hovered widgets
//     let active_bg_color = Color32::from_rgb(28, 28, 28); // Background for active widgets
//     let border_color = Color32::from_rgb(16, 16, 23); // Border color for windows and panels
//     let text_color = Color32::from_rgb(199, 202, 245); // Default text color
//     let error_color = Color32::from_rgb(227, 104, 176); // Error text color
//     let warn_color = Color32::from_rgb(155, 104, 227); // Warning text color
//     let link_color = Color32::from_rgb(155, 104, 227); // Hyperlink color
//     let theme = CarlDark; // Assuming a theme object or struct
//     let mut custom_style: Style = theme.custom_style();
//     // Font settings
//     let mut font = FontId::default();
//     font.size = 10.5;
//     font.family = FontFamily::Proportional;
//     // Assign custom font
//     custom_style.override_font_id = Some(font);
//     // Adjust spacing and interactions
//     custom_style.spacing.button_padding = Vec2::new(3.0, 3.0);
//     custom_style.spacing.item_spacing = Vec2::new(2.0, 1.0);
//     custom_style.spacing.combo_height = 55.0;
//     custom_style.spacing.combo_width = 100.0;
//     custom_style.interaction.selectable_labels = true;
//     custom_style.interaction.interact_radius = 10.0;
//     // Define visuals with updated values
//     custom_style.visuals = Visuals {
//         dark_mode: true,                       // Set for dark mode
//         override_text_color: Some(text_color), // Global text color override
//         widgets: Widgets {
//             noninteractive: WidgetVisuals {
//                 bg_fill: widget_bg_color,
//                 weak_bg_fill: widget_bg_color,
//                 bg_stroke: Stroke::new(1.0, Color32::from_rgb(50, 50, 60)),
//                 rounding: Rounding::same(4.0),
//                 fg_stroke: Stroke::new(1.0, foreground_color),
//                 expansion: 0.0,
//             },
//             inactive: WidgetVisuals {
//                 bg_fill: widget_bg_color,
//                 weak_bg_fill: Color32::from_rgb(18, 18, 20),
//                 bg_stroke: Stroke::new(1.0, Color32::from_rgb(80, 80, 80)),
//                 rounding: Rounding::same(4.0),
//                 fg_stroke: Stroke::new(1.0, text_color),
//                 expansion: 0.0,
//             },
//             hovered: WidgetVisuals {
//                 bg_fill: hovered_bg_color,
//                 weak_bg_fill: Color32::from_rgb(40, 40, 45),
//                 bg_stroke: Stroke::new(0.5, Color32::from_rgba_premultiplied(120, 20, 120, 100)),
//                 rounding: Rounding::same(4.0),
//                 fg_stroke: Stroke::new(1.0, link_color), // Highlight text in link color
//                 expansion: 0.1,
//             },
//             active: WidgetVisuals {
//                 bg_fill: active_bg_color,
//                 weak_bg_fill: Color32::from_rgb(28, 28, 28),
//                 bg_stroke: Stroke::new(1.0, Color32::from_rgb(90, 90, 100)),
//                 rounding: Rounding::same(4.0),
//                 fg_stroke: Stroke::new(1.0, foreground_color), // Active widget text
//                 expansion: 0.1,
//             },
//             open: WidgetVisuals {
//                 bg_fill: Color32::from_rgb(30, 30, 35),
//                 weak_bg_fill: Color32::from_rgb(35, 35, 40),
//                 bg_stroke: Stroke::new(1.0, Color32::from_rgb(100, 100, 110)),
//                 rounding: Rounding::same(4.0),
//                 fg_stroke: Stroke::new(1.0, foreground_color), // Open widget text
//                 expansion: 0.1,
//             },
//         },
//         selection: Selection {
//             bg_fill: Color32::from_rgba_premultiplied(90, 55, 88, 90), // Selection background
//             stroke: Stroke::new(1.0, Color32::from_rgba_premultiplied(81, 92, 126, 50)), // Selection border
//         },
//         hyperlink_color: link_color,                   // Hyperlink color
//         faint_bg_color: Color32::from_rgb(20, 20, 25), // Subtle background elements
//         extreme_bg_color: Color32::from_rgb(15, 15, 20), // Very dark background for contrast
//         code_bg_color: Color32::from_rgb(20, 20, 27),  // Background for code blocks
//         warn_fg_color: warn_color,                     // Warning text color
//         error_fg_color: error_color,                   // Error text color
//         window_rounding: Rounding::same(4.0),
//         window_shadow: Shadow::default(),
//         window_fill: background_color,
//         window_stroke: Stroke::new(1.0, border_color), // Window border
//         window_highlight_topmost: true,
//         menu_rounding: Rounding::same(4.0),
//         panel_fill: background_color,
//         popup_shadow: Shadow::default(),
//         resize_corner_size: 10.0,
//         text_cursor: TextCursorStyle::default(),
//         clip_rect_margin: 5.0,
//         button_frame: true,
//         collapsing_header_frame: true,
//         indent_has_left_vline: true,
//         striped: true,
//         slider_trailing_fill: true,
//         handle_shape: HandleShape::Circle,
//         interact_cursor: Some(CursorIcon::PointingHand),
//         image_loading_spinners: true,
//         numeric_color_space: NumericColorSpace::Linear, // How numeric values are displayed
//     };
//     Arc::new(custom_style)
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
