use std::ffi::OsStr;

use app_state::{AppState, MainPages, MasterTechApp};
use displays::ui_tools::theme_config::set_custom_style;
use eframe::egui::{Context, IconData, Window};
// use terminal_mode::run_terminal_mode;
use egui_dock::DockState;
use log::{error, info};
use utilities::ai::run_mcp_server_tcp;

#[cfg(target_os = "windows")]
extern crate winapi;

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
        self.receive_prestashop();
        self.context.shared_ctx.receive_task();
        self.context.shared_ctx.receive_ticket();
        self.context.shared_ctx.receive_notes();
        self.context.shared_ctx.receive_notification();
        self.context.shared_ctx.receive_inventory();
        self.context.shared_ctx.receive_client();
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
                self.context.shared_ctx.db_tx.clone(),
                self.context.app_state_tx.clone(),
            ),
            _ => {}
        }
    }

    // fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {}
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

    let res = std::thread::spawn(move || {
        let old_exe = std::env::current_dir().unwrap().join("MasterTech.exe");
        let current_exe = std::env::current_exe();
        let current_exe_name = current_exe.as_ref().unwrap().file_name();
        if current_exe_name == Some(OsStr::new("git-MasterTech.exe")) && old_exe.exists() {
            match std::fs::remove_file(old_exe) {
                Ok(_) => {
                    log::info!("Removed old exe");
                    if let Ok(_) = std::fs::rename(std::env::current_exe().unwrap(), "Mastertech.exe") {
                        log::info!("Renamed exe");
                    }
                },
                Err(e) => log::info!("Error removing old exe: {e:?}"),
            }
        }
    }).join();

    tokio::spawn(async move {
        run_mcp_server_tcp().await?;
        Ok::<(), anyhow::Error>(())
    });
    
    log::info!("Res: {res:?}");
    // console_subscriber::init(); // for tokio console
    let matches = clap::Command::new("Mastertech 4")
        .version(env!("CARGO_PKG_VERSION"))
        .author("Shadowbroker")
        // .about("Accepts a command-line argument and prints it")
        .arg(
            clap::Arg::new("term")
                .short('t')
                .long("term")
                .help("Run MasterTech in Terminal Mode")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            clap::Arg::new("continue")
                .short('c')
                .long("continue")
                .help("Continue running scripts based on where we left off")
                .action(clap::ArgAction::SetTrue),
        )
        .get_matches();

    if matches.get_flag("term") {
        let res = terminal_mode::run_terminal_mode().await;
        log::info!("TERM MODE: {res:?}");
    } else {
        let eframe_app = eframe::run_native(
            format!("Mastertech-{}", env!("CARGO_PKG_VERSION")).as_str(),
            eframe::NativeOptions {
                viewport: eframe::egui::ViewportBuilder::default()
                    .with_inner_size([945.0, 750.0])
                    .with_drag_and_drop(true)
                    .with_icon(load_icon()),
                    // .with_always_on_top(),
                ..Default::default()
            },
            Box::new(|cc| {
                Ok(
                    Box::new(
                        MasterTechApp::new(cc)
                    )
                )
            }),
        );

        if let Err(e) = eframe_app { 
            error!("Error running eframe_native: {e:?} \nswitching to secondary application");
            let res = terminal_mode::run_terminal_mode().await;
            if let Err(e) = res {
                error!("Error running terminal app: {e:?}");
            }
        } else {
            displays::tabs::logger::logging::builder().init().unwrap();
            // let log_level = log::LevelFilter::Info;
            // let log_file = std::fs::File::create("output.log").unwrap();
            // simplelog::WriteLogger::init(
            //     log_level,
            //     simplelog::Config::default(),
            //     log_file
            // ).unwrap();
        }
    }
    
    Ok(())
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

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use bincode::config::standard;
    use displays::remote_viewer::{ratagui::BufferMessage, SerializableBuffer};
    use ratatui::{buffer::Buffer, layout::Rect};

    #[test]
    fn test_encode_buffer_with_timestamp() {

        // Create BufferMessage with SerializableBuffer
        let message = BufferMessage {
            timestamp: SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_millis(),
            frame_count: 1,
            encode_duration: 0, // Placeholder
            buffer: SerializableBuffer::from(Buffer::empty(Rect::new(0, 0, 10, 10))),
        };

        // Encode
        let bincoded = bincode::serde::encode_to_vec(&message, standard()).unwrap();
        let compressed = zstd::encode_all(std::io::Cursor::new(&bincoded), 8).unwrap();

        println!("Compressed data: {:?}", compressed);

        // Decode
        let bincoded = zstd::decode_all(&*compressed).unwrap();
        let (decoded_message, _) = bincode::serde::borrow_decode_from_slice::<BufferMessage, _>(
            &bincoded,
            standard(),
        )
        .unwrap();

        // Convert decoded SerializableBuffer back to Buffer for comparison
        let decoded_buffer = Buffer::from(decoded_message.buffer.clone());

        println!("Decoded message: {:?}", decoded_message);
        assert_eq!(message.buffer, decoded_buffer.into()); // Verify the buffer contents
    }
    
}