use displays::app_state::{AppState, MainPages};
use std::ffi::OsStr;
use log::{error, info};

#[cfg(target_os = "windows")]
extern crate winapi;

mod terminal_mode;
mod software_gui;
pub mod app_state;
mod filesystem;
pub mod pages;
pub mod tabs;
pub mod utilities;
pub mod viewports;
pub mod first_run;
pub mod data;
pub mod transport;
pub mod tcp_listener;
pub mod remote_self_update;

impl eframe::App for app_state::MasterTechApp {
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // One-time: register PluginManager bridge with egui
        if !self.context.plugin_manager_registered {
            self.context.plugin_manager_registered = true;

            let (egui_frame_rx, egui_input_tx) = {
                let mut mgr = self.context.plugin_manager.write().unwrap();
                let capture = displays::plugins::EguiFrameCapture::new();
                let rx = capture.frame_rx.clone();
                let input_tx = capture.input_tx.clone();
                mgr.register(Box::new(capture));
                mgr.set_plugin_enabled("com.mastertech.egui-frame-capture", true);
                mgr.register(Box::new(displays::plugins::EguiRemoteViewer::new()));
                // mgr.register(Box::new(displays::plugins::HelloMastertechPlugin::default()));
                (rx, input_tx)
            };
            self.context.egui_frame_rx = Some(egui_frame_rx);
            self.context.egui_input_tx = Some(egui_input_tx);

            let handle = displays::plugins::PluginManagerHandle(
                self.context.plugin_manager.clone(),
            );
            ctx.add_plugin(handle);

            // Native egui 0.35 inspection server (loopback) so the in-app MCP egui_inspect_* tools
            // can read/drive this app. Gated: debug builds or MTECH_EGUI_INSPECT=1.
            if displays::plugins::egui_inspect::inspection_enabled() {
                ctx.add_plugin(egui_inspection::InspectionPlugin::new(Some("MasterTech".to_owned())));
                match egui_inspection::serve(ctx, displays::plugins::egui_inspect::INSPECT_ADDR) {
                    Ok(()) => log::info!(
                        "egui_inspection serving on {}",
                        displays::plugins::egui_inspect::INSPECT_ADDR
                    ),
                    Err(e) => log::warn!("egui_inspection serve failed: {e}"),
                }
            }

            // Plugin MCP: TCP 9003 (raw stream) + HTTP 9004 /mcp (Cursor / Streamable HTTP)
            let mgr_tcp = self.context.plugin_manager.clone();
            tokio::spawn(async move {
                if let Err(e) = displays::plugins::mcp_bridge::run_plugin_mcp_server(mgr_tcp).await {
                    log::error!("Plugin MCP TCP server error: {e:?}");
                }
            });
            let mgr_http = self.context.plugin_manager.clone();
            tokio::spawn(async move {
                if let Err(e) = displays::plugins::mcp_bridge::run_plugin_mcp_server_http(mgr_http).await {
                    log::error!("Plugin MCP HTTP server error: {e:?}");
                }
            });
        }

        self.receive_logic(ctx, frame);

        // Pump all admin client session transports in the logic phase.
        self.context.shared_ctx.web_console_layout.pump_sessions(ctx);

        self.context.shared_ctx.drain_fleet_updates();

        // Handle "Already connected" state recovery in logic phase
        if let AppState::NoAuth(reason) = &self.context.shared_ctx.state {
            if reason.contains("Already connected") {
                info!("Already connected");
                let usr = self.context.shared_ctx.current_user.clone();
                if let Some(user) = usr {
                    self.context.shared_ctx.load_data(ctx, &user);
                    let _ = self.context.shared_ctx.app_state_tx.try_send(AppState::Authenticated(MainPages::Tasks));
                } else {
                    self.context.shared_ctx.first_run = true;
                    self.first_run(ctx);
                    log::error!("1");
                    self.context.shared_ctx.state = AppState::NoAuth("No user detected".to_string());
                }
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        ui.options_mut(|options| {
            options.max_passes = std::num::NonZeroUsize::new(2).unwrap();
        });

        self.receive_ui(ui.ctx(), frame);
        self.menu_bar(ui);

        match &self.context.shared_ctx.state {
            AppState::Authenticated(page) => match page {
                MainPages::Tasks => self.main_page(ui),
                MainPages::UserPreferences => self
                    .context
                    .shared_ctx
                    .account_settings_page(ui, self.context.shared_ctx.app_state_tx.clone()),
                _ => {}
            },
            AppState::CreateAccount => self.context.shared_ctx.signup_page(
                ui,
                self.context.shared_ctx.db_tx.clone(),
                self.context.shared_ctx.app_state_tx.clone(),
            ),
            AppState::NoAuth(reason) => {
                if !reason.contains("Already connected") {
                    self.context.shared_ctx.login_page(
                        ui,
                        self.context.shared_ctx.db_tx.clone(),
                        self.context.shared_ctx.app_state_tx.clone(),
                    )
                }
            }
        }

        // Render Mastertech plugin UIs from the user-frame callback (here),
        // NOT from inside `egui::Plugin::on_end_pass`. egui holds an internal
        // mutex on `PluginHandle` for the duration of `on_end_pass`; any
        // interactive widget a plugin creates from there re-enters that
        // mutex via `Context::create_widget` → `on_widget_under_pointer`
        // and triggers the 10s `epaint::mutex` deadlock panic. Calling here
        // means no egui plugin lock is held while plugins render.
        let plugin_handle = displays::plugins::PluginManagerHandle(
            self.context.plugin_manager.clone(),
        );
        plugin_handle.render_plugin_uis(ui);
    }

    fn persist_egui_memory(&self) -> bool { true }

    /// Called by eframe right before the window is destroyed. Fire the global
    /// shutdown signal so every long-running tokio loop (TCP admin listener on
    /// :9101, MCP servers on :9001/:9003/:9004) breaks out of its `accept`
    /// before the runtime starts dropping. Without this the runtime drop can
    /// hang on Windows IOCP waits and keep the launching terminal alive.
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        log::info!("MasterTechApp::on_exit -> signaling global shutdown to background tasks");
        displays::signal_shutdown();
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let ticket_data = serde_json::to_string(&self.context.ticket_data).unwrap_or_default();
        // let computer_data = serde_json::to_string(&self.context.computer_data).unwrap_or_default();
        let task_data = serde_json::to_string(&self.context.task_data).unwrap_or_default();
        let customer_data = serde_json::to_string(&self.context.customer_data).unwrap_or_default();
        let seb_info = serde_json::to_string(&self.context.seb_info).unwrap_or_default();
        storage.set_string("ticket_data", ticket_data);
        // storage.set_string("computer_data", computer_data);
        storage.set_string("task_data", task_data);
        storage.set_string("customer_data", customer_data);
        storage.set_string("seb_info", seb_info);
        storage.flush();
    }
}

impl app_state::MasterTechApp {
    // Mirrors the eframe `logic` call with a storage-less stub frame.
    pub fn logic_inner(&mut self, ctx: &egui::Context) {
        let mut frame = eframe::Frame::_new_kittest();
        <Self as eframe::App>::logic(self, ctx, &mut frame);
    }

    // Renders the eframe App UI onto the software-backend root Ui.
    pub fn ui_inner(&mut self, ui: &mut egui::Ui) {
        let mut frame = eframe::Frame::_new_kittest();
        <Self as eframe::App>::ui(self, ui, &mut frame);
    }
}

fn env_logger_with_dependency_filters() -> env_logger::Builder {
    let mut builder = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,evtx=warn"),
    );
    builder.filter_module("evtx", log::LevelFilter::Warn);
    builder
}

fn output_log_path() -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("output.log")))
        .unwrap_or_else(|| std::path::PathBuf::from("output.log"))
}

#[cfg(target_os = "windows")]
fn attach_parent_console() {
    use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(not(target_os = "windows"))]
fn attach_parent_console() {}

fn stderr_logger() -> Box<dyn log::Log + 'static> {
    Box::new(
        env_logger_with_dependency_filters()
            .target(env_logger::Target::Stderr)
            .build(),
    )
}

fn file_logger() -> Box<dyn log::Log + 'static> {
    let log_path = output_log_path();
    Box::new(simplelog::WriteLogger::new(
        log::LevelFilter::Trace,
        simplelog::Config::default(),
        std::fs::File::create(&log_path)
            .unwrap_or_else(|e| panic!("create {}: {e}", log_path.display())),
    ))
}

fn start_tui_logger_event_pump() {
    use std::sync::Once;
    static START: Once = Once::new();
    START.call_once(|| {
        std::thread::Builder::new()
            .name("tui-logger::move_events".into())
            .spawn(|| {
                let duration = std::time::Duration::from_millis(10);
                loop {
                    std::thread::park_timeout(duration);
                    tui_logger::move_events();
                }
            })
            .expect("tui-logger mover thread");
    });
    tui_logger::set_default_level(log::LevelFilter::Info);
}

fn tui_drain_logger() -> Box<dyn log::Log + 'static> {
    let drain = tui_logger::Drain::new();
    Box::new(
        env_logger_with_dependency_filters()
            .format(move |_buf, record| {
                terminal_mode::data::log_capture::capture_record(record);
                Ok(drain.log(record))
            })
            .build(),
    )
}

fn init_terminal_mode_logging(log_to_file: bool) {
    start_tui_logger_event_pump();
    if log_to_file {
        attach_parent_console();
        multi_log::MultiLogger::init(
            vec![tui_drain_logger(), stderr_logger(), file_logger()],
            log::Level::Info,
        )
        .expect("Error initializing multi_logger");
    } else {
        let drain = tui_logger::Drain::new();
        env_logger_with_dependency_filters()
            .format(move |_buf, record| {
                terminal_mode::data::log_capture::capture_record(record);
                Ok(drain.log(record))
            })
            .try_init()
            .expect("Error initializing terminal mode logger");
    }
}

async fn run_gui(log_to_file: bool, force_cpu: bool) -> eframe::Result<()> {
    let egui_logger = Box::new(
        displays::ui_tools::egui_logger::builder()
            .add_blacklist("evtx::evtx_chunk")
            .add_blacklist("evtx::evtx_parser")
            .build(),
    );
    start_tui_logger_event_pump();
    let mut loggers: Vec<Box<dyn log::Log + 'static>> =
        vec![egui_logger, tui_drain_logger()];
    if log_to_file {
        attach_parent_console();
        loggers.push(stderr_logger());
        loggers.push(file_logger());
        eprintln!("Mastertech logging to {}", output_log_path().display());
    }
    multi_log::MultiLogger::init(loggers, log::Level::Info)
        .expect("Error initializing multi_logger");

    tokio::spawn(async move {
        utilities::ai::run_mcp_server_tcp().await?;
        Ok::<(), anyhow::Error>(())
    });
    // GPU (glow) first, then the egui_skia software renderer, then terminal mode.
    let mut gui_ok = false;
    if force_cpu {
        log::info!("--cpu/--software set; forcing the egui_skia software renderer");
        match software_gui::run() {
            Ok(()) => gui_ok = true,
            Err(e) => error!("software renderer failed: {e:?}"),
        }
    } else {
        let eframe_app = eframe::run_native(
            format!("Mastertech-{}", database::version_with_build!()).as_str(),
            eframe::NativeOptions {
                viewport: eframe::egui::ViewportBuilder::default()
                    .with_inner_size([1000.0, 750.0])
                    .with_drag_and_drop(true)
                    .with_icon(load_icon()),
                ..Default::default()
            },
            Box::new(|cc| {
                egui_extras::install_image_loaders(&cc.egui_ctx);
                Ok(Box::new(app_state::MasterTechApp::new(cc)))
            }),
        );
        match eframe_app {
            Ok(()) => gui_ok = true,
            Err(e) => {
                error!("eframe glow init failed: {e:?}; trying egui_skia software renderer");
                match software_gui::run() {
                    Ok(()) => gui_ok = true,
                    Err(e2) => error!("software renderer failed: {e2:?}"),
                }
            }
        }
    }

    if gui_ok {
        displays::signal_shutdown();
        tokio::time::sleep(std::time::Duration::from_millis(750)).await;
        log::info!("main -> GUI closed; forcing process exit to release the launching terminal");
        std::process::exit(0);
    } else {
        error!("no GUI could start; switching to terminal mode");
        if let Err(e) = terminal_mode::run_terminal_mode().await {
            error!("Error running terminal app: {e:?}");
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> eframe::Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();

    // Correct a stale clock (Windows PE boots ~years in the past) before any TLS
    // handshake, or rustls rejects valid certs as "not valid yet".
    if let Err(e) = database::clock_sync::ensure_system_clock_sane() {
        log::warn!("clock sync failed ({e:?}); TLS may fail if the system clock is wrong");
    }

    #[cfg(target_os = "windows")]
    {
        use windows::Win32::System::Threading::GetCurrentProcess;
        use windows::Win32::System::Threading::SetPriorityClass;
        use windows::Win32::System::Threading::ABOVE_NORMAL_PRIORITY_CLASS;
        unsafe {
            let _ = SetPriorityClass(GetCurrentProcess(), ABOVE_NORMAL_PRIORITY_CLASS);
        }
    }

    let matches = clap::Command::new("Mastertech")
        .version(env!("CARGO_PKG_VERSION"))
        .author("Shadowbroker")
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
                .help("Also write logs to output.log beside the exe, and mirror to cmd when launched from a console")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            clap::Arg::new("continue")
                .short('c')
                .long("continue")
                .help("Continue running scripts based on where we left off")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            clap::Arg::new("mcp-stdio")
                .long("mcp-stdio")
                .help("Run only the plugin MCP server over stdin/stdout (for Claude Desktop). Skips the GUI entirely; logs go to stderr.")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            clap::Arg::new("cpu")
                .long("cpu")
                .visible_alias("software")
                .help("Force the egui_skia software (CPU) renderer, skipping the GPU attempt")
                .action(clap::ArgAction::SetTrue),
        )
        .get_matches();

    let log_to_file = matches.get_flag("log");
    if log_to_file {
        attach_parent_console();
        eprintln!("Mastertech logging to {}", output_log_path().display());
    }

    match check_old_exe() {
        Ok(_) => log::info!("check_old_exe ran ok"),
        Err(e) => {
            if log_to_file {
                eprintln!("check_old_exe failed: {e:?}");
            }
            log::error!("check_old_exe Err: {e:?}");
        }
    }

    // ── --mcp-stdio: headless single-session MCP for Claude Desktop ────────────
    //
    // Must come before every other branch because:
    //   * Claude Desktop spawns us as a child process and talks JSON-RPC on
    //     stdio. Anything that writes to *stdout* corrupts the framing — so we
    //     pin the logger to stderr and skip the multi_log/egui_logger setup.
    //   * eframe / the GUI mode auto-spawns the TCP :9003 and HTTP :9004 plugin
    //     MCP servers from inside `logic()`. Re-spawning them here would cause
    //     a `bind: address in use` if a GUI instance is already running, and
    //     they're useless to Claude Desktop anyway.
    //   * No `process::exit(0)` race with eframe's drop path.
    if matches.get_flag("mcp-stdio") {
        let _ = env_logger_with_dependency_filters()
        .target(env_logger::Target::Stderr)
        .try_init();

        log::info!("Mastertech --mcp-stdio: starting plugin MCP server on stdio (no GUI)");

        let (plugin_dispatcher, _plugin_cmd_rx) =
            displays::plugins::DefaultEventDispatcher::new();
        let plugin_manager = {
            let mut mgr = displays::plugins::PluginManager::new();
            mgr.set_dispatcher(plugin_dispatcher);
            std::sync::Arc::new(std::sync::RwLock::new(mgr))
        };

        if let Err(e) =
            displays::plugins::run_plugin_mcp_server_stdio(plugin_manager).await
        {
            log::error!("Plugin MCP stdio server error: {e:?}");
            std::process::exit(1);
        }
        // Peer closed cleanly — exit so we release stdin/stdout handles and
        // the parent (Claude Desktop) can clean up its child-process record.
        std::process::exit(0);
    }

    if matches.get_flag("term") {
        init_terminal_mode_logging(log_to_file);
        let res = terminal_mode::run_terminal_mode().await;
        log::info!("TERM MODE: {res:?}");
    } else {
        run_gui(log_to_file, matches.get_flag("cpu")).await?;
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

pub(crate) fn load_icon() -> eframe::egui::IconData {
    let (icon_rgba, _icon_width, _icon_height) = {
        let icon = include_bytes!("assets/masterlogoV3.ico");
        let image = image::load_from_memory(icon)
            .expect("Failed to open icon path")
            .into_rgba8();
        let (width, height) = image.dimensions();
        let rgba = image.into_raw();
        (rgba, width, height)
    };

    eframe::egui::IconData {
        rgba: icon_rgba,
        width: 256,
        height: 256,
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
    
    use database::{PlatformSpawner, Spawner, schema::odoo::inventory::search_open_orders_for_product};

    #[test]
    fn test_inventory_calls() {
        PlatformSpawner::spawn(async move {
            let results = search_open_orders_for_product("GPU/RTX4060TI16", "7").await;
            log::info!("Results: {results:?}");
        });
    }
}
