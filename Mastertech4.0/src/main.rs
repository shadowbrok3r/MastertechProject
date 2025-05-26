use displays::{app_state::{AppState, MainPages}, ui_tools::theme_config::set_custom_style};
use app_state::MasterTechApp;
use eframe::egui::{Context, IconData, Window};
use std::ffi::OsStr;
// use terminal_mode::run_terminal_mode;
use egui_dock::DockState;
use log::{error, info};

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
        ctx.options_mut(|options| {
            options.max_passes = std::num::NonZeroUsize::new(2).unwrap();
        });

        // most important part of the whole app.. setting up our styling

        let theme_res = Window::new("Theme Configuration")
        .open(&mut self.context.shared_ctx.modify_theme)
        .max_height(600.)
        .min_width(700.)
        .title_bar(true)
        .show(ctx, |ui|
            self.context.shared_ctx.theme_config.edit_ui(ui, self.context.shared_ctx.settings_sender.clone())
        );
        
        if let Some(window_res) = theme_res {
            if let Some(r) = window_res.inner {
                if r.0 {
                    if let Some(user) = self.context.shared_ctx.current_user.clone().as_mut() {
                        user.set_color_scheme(serde_json::to_value(r.1.clone()).unwrap());
                        if let Some(storage) = frame.storage_mut() {
                            storage.set_string("user_settings", serde_json::to_string(&user.get_user_settings()).unwrap_or_default());
                        }
                    }
                    self.context.shared_ctx.theme_config = r.1;
                    self.context.shared_ctx.modify_theme = false;
                }
            }
        }
        

        let custom_style = set_custom_style(&self.context.shared_ctx.theme_config);
        ctx.set_style((*custom_style).clone());

        if self.context.first_run { self.first_run(); }

        // Get User settings from local storage
        if let Some(user) = &self.context.shared_ctx.current_user {
            if self.context.get_settings {
                self.context.get_settings = false;
                match serde_json::from_value::<DockState<String>>(user.get_user_settings().get_ui_layout_mastertech()){
                    Ok(tree) => self.tree = tree,
                    Err(e) => log::error!("Could not get UI layout from user: {e:?}"),
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

        match &self.context.shared_ctx.state {
            AppState::Authenticated(page) => match page {
                MainPages::Tasks => self.main_page(ctx),
                MainPages::UserPreferences => self
                    .context
                    .shared_ctx
                    .account_settings_page(ctx, self.context.shared_ctx.app_state_tx.clone()),
                _ => {}
            },
            AppState::NoAuth(reason) => {
                if reason.to_string().contains("Already connected") {
                    info!("Already connected");
                    if self.context.shared_ctx.current_user.is_some() {
                        if !self.context.shared_ctx.load_data(ctx) {
                            self.context.first_run = true;
                            self.first_run();
                            self.context.shared_ctx.state = AppState::NoAuth("No user detected".to_string());
                        } else {
                            self.context.first_run = true;
                            self.first_run();
                        }
                        let _ = self.context.shared_ctx.app_state_tx.try_send(AppState::Authenticated(MainPages::Tasks));
                    } else {
                        self.context.first_run = true;
                        self.first_run();
                        let _ = self.context.shared_ctx.app_state_tx.try_send(AppState::Authenticated(MainPages::Tasks));
                    }
                } else {
                    self.context.shared_ctx.login_page(
                        ctx,
                        self.context.shared_ctx.db_tx.clone(),
                        self.context.shared_ctx.app_state_tx.clone(),
                    )
                }
            },
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

    match check_old_exe() {
        Ok(_) => log::info!("check_old_exe ran ok"),
        Err(e) => log::error!("check_old_exe Err: {e:?}"),
    }


    // tokio::spawn(async move {
    //     utilities::ai::run_mcp_server_tcp().await?;
    //     Ok::<(), anyhow::Error>(())
    // });
    
    // let _ = crate::utilities::scripts::InstalledProgram::get_installed_programs();

    // console_subscriber::init(); // for tokio console
    let matches = clap::Command::new("Mastertech")
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
            clap::Arg::new("log")
                .short('l')
                .long("log")
                .help("output log to file (output.log)")
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
        simplelog::WriteLogger::init(
            log::LevelFilter::Info,
            simplelog::Config::default(),
            std::fs::File::create("tui-output.log").unwrap()
        ).unwrap();
        let res = terminal_mode::run_terminal_mode().await;
        log::info!("TERM MODE: {res:?}");
    } else if matches.get_flag("log") {
        simplelog::WriteLogger::init(
            log::LevelFilter::Trace,
            simplelog::Config::default(),
            std::fs::File::create("output.log").unwrap()
        ).unwrap();
    } else {
        let init = displays::tabs::logger::logging::builder().init();
        log::info!("Init logger: {init:?}");
        // simplelog::WriteLogger::init(
        //     log::LevelFilter::Info,
        //     simplelog::Config::default(),
        //     std::fs::File::create("output.log").unwrap()
        // ).unwrap();
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
            // displays::tabs::logger::logging::builder().init()
            // Set max_log_level to Trace
            let init = tui_logger::init_logger(log::LevelFilter::Info);
            log::info!("Init logger: {init:?}");
            // // Set default level for unknown targets to Trace
            // tui_logger::set_default_level(log::LevelFilter::Info);
            // simplelog::WriteLogger::init(
            //     log::LevelFilter::Info,
            //     simplelog::Config::default(),
            //     std::fs::File::create("tui-output.log").unwrap()
            // ).unwrap();
            error!("Error running eframe_native: {e:?} \nswitching to secondary application");
            let res = terminal_mode::run_terminal_mode().await;
            if let Err(e) = res {
                error!("Error running terminal app: {e:?}");
            }
        } else {
            let _x = displays::tabs::logger::logging::builder().init();
        }
    }
    
    Ok(())
}

fn check_old_exe() -> anyhow::Result<(), anyhow::Error> {
    let old_exe = std::env::current_dir()?;
    // for dir in old_exe.read_dir()? {
    //     let entry = dir?;
    //     let file_name = entry.file_name().into_string().unwrap_or_default();
    //     let file = entry.path();
    //     if file_name.contains("__selfdelete__") {
    //         std::fs::remove_file(file)?;
    //     }
    // }

    if std::env::current_exe()?.file_name() == Some(OsStr::new("git-MasterTech.exe")) && old_exe.join("MasterTech.exe").exists() {
        match std::fs::remove_file(old_exe) {
            Ok(_) => {
                log::info!("Removed old exe");
                std::fs::rename(std::env::current_exe()?, "Mastertech.exe")?;
            },
            Err(e) => log::error!("Error removing old exe: {e:?}"),
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