use egui::Style;

mod app;
mod bug_report;
mod charts;
mod checklist_store;
mod checklist_verify;
mod crash_report;
mod db;
mod diagnostics;
mod driver_check;
mod fleet_client;
mod hardware_id;
mod hw_monitor;
mod hw_sampler;
mod mcp;
mod order_panel;
mod pending_results;
mod provisioning;
mod qc_benchmark;
mod oa3_sager;
mod report_view;
mod reporting;
mod schema;
#[cfg(feature = "skia-render")]
mod software_gui;
mod spec_check;
mod stress_panel;
mod telemetry;
mod terminal_mode;
mod update_check;

pub(crate) static LAUNCH_TERMINAL: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(feature = "skia-render")]
fn try_software_gui() -> bool {
    match software_gui::run() {
        Ok(()) => true,
        Err(e) => {
            log::error!("qc-app: software renderer failed ({e:?})");
            false
        }
    }
}

#[cfg(not(feature = "skia-render"))]
fn try_software_gui() -> bool {
    log::error!(
        "qc-app: egui_skia software renderer not compiled into this build (enable the `skia-render` feature)"
    );
    false
}

#[tokio::main]
async fn main() -> eframe::Result<()> {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::System::Threading::GetCurrentProcess;
        use windows::Win32::System::Threading::SetPriorityClass;
        use windows::Win32::System::Threading::ABOVE_NORMAL_PRIORITY_CLASS;
        unsafe {
            let _ = SetPriorityClass(GetCurrentProcess(), ABOVE_NORMAL_PRIORITY_CLASS);
        }
    }

    // Route all `log` output into the in-app Logs tab (mtech-ui egui_logger).
    let _ = mtech_ui::egui_logger::builder()
        .max_level(log::LevelFilter::Debug)
        // Silence third-party crates that flood the in-app Logs tab — most
        // notably evtx, which logs every event-log record as the WHEA/TDR
        // monitors scan the System log.
        .add_blacklist("evtx")
        .add_blacklist("winit")
        .add_blacklist("hyper_util")
        .add_blacklist("wgpu_hal")
        .add_blacklist("wgpu_core")
        .add_blacklist("naga")
        .init();
    crate::crash_report::install_panic_hook();
    log::info!(
        "qc-app: starting v{} (pid={})",
        env!("CARGO_PKG_VERSION"),
        std::process::id()
    );

    // rustls 0.23 demands a process-level CryptoProvider. SurrealDB's WS
    // transport and reqwest both pick it up — install once before any TLS
    // handshake, or both panic on first use.
    if rustls::crypto::ring::default_provider()
        .install_default()
        .is_err()
    {
        log::debug!("qc-app: rustls CryptoProvider already installed");
    }

    // Correct a stale clock (Windows PE boots ~years in the past) before the
    // first TLS handshake, or rustls rejects valid certs as "not valid yet".
    if let Err(e) = database::clock_sync::ensure_system_clock_sane() {
        log::warn!("qc-app: clock sync failed ({e:?}); TLS may fail if the system clock is wrong");
    }

    // Establish the SurrealDB connection + guest signin once at startup so
    // stress-runner can persist `stress_test_run` / metric / event rows.  The
    // guest access has just enough permission to write to the stress test
    // tables (which are `PERMISSIONS FULL` at the table level).  If this
    // fails, the app still runs but stress runs won't be persisted — the
    // worker thread logs a clear "Connection uninitialised" error.
    match database::init_database().await {
        Ok(()) => log::info!("qc-app: SurrealDB connected + guest signin OK"),
        Err(e) => log::error!(
            "qc-app: failed to initialize SurrealDB ({e:?}) — stress runs won't persist"
        ),
    }

    // Hand stress-runner this tokio runtime so its DB writes run on the same
    // runtime that owns the SurrealDB WebSocket connection.  Without this,
    // stress-runner would fall back to its own runtime and the cross-runtime
    // futures might never see WS responses.
    stress_runner::set_runtime_handle(tokio::runtime::Handle::current());

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title(format!("Mastertech QC - {}", database::version_with_build!()))
            .with_inner_size([500.0, 768.0])
            .with_icon(load_icon())
            .with_drag_and_drop(true),
        ..Default::default()
    };

    // Explicit -t/--term launches the standalone ratatui terminal mode directly.
    if std::env::args().any(|a| a == "-t" || a == "--term") {
        if let Err(e) = terminal_mode::run_terminal_mode().await {
            log::error!("qc-app: terminal mode exited with error: {e:?}");
        }
        return Ok(());
    }

    // --cpu/--software forces the egui_skia CPU renderer, skipping the GPU
    // attempt, to test the software path without a no-GPU machine.
    let force_cpu = std::env::args().any(|a| a == "--cpu" || a == "--software");

    let mut gui_ok = false;
    if force_cpu {
        log::info!("qc-app: --cpu set; forcing the egui_skia software renderer");
        gui_ok = try_software_gui();
    } else {
        match eframe::run_native(
            "Mastertech QC",
            options,
            Box::new(|cc| {
                configure_egui_ctx(&cc.egui_ctx);
                Ok(Box::new(app::QcApp::new(cc)))
            }),
        ) {
            Ok(()) => gui_ok = true,
            Err(e) => {
                log::warn!(
                    "qc-app: hardware GL (glow) init failed ({e:?}); falling back to software renderer (egui_skia)"
                );
                gui_ok = try_software_gui();
            }
        }
    }

    // Terminal mode: manual (button set the flag) or last-resort (no GUI could start).
    if LAUNCH_TERMINAL.load(std::sync::atomic::Ordering::Relaxed) || !gui_ok {
        if let Err(e) = terminal_mode::run_terminal_mode().await {
            log::error!("qc-app: terminal mode exited with error: {e:?}");
        }
    }
    Ok(())
}

/// Shared egui context setup: phosphor fonts, forced dark theme, global style.
pub(crate) fn configure_egui_ctx(ctx: &egui::Context) {
    // Phosphor icon glyphs merged into the default fonts. add_to_fonts only
    // registers phosphor under Proportional; the style forces Monospace
    // everywhere, so register it there too or icons tofu.
    let mut fonts = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    if let Some(keys) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
        if !keys.iter().any(|k| k == "phosphor") {
            keys.insert(1.min(keys.len()), "phosphor".into());
        }
    }
    ctx.set_fonts(fonts);
    ctx.options_mut(|opt| opt.theme_preference = egui::ThemePreference::Dark);
    match serde_json::from_str::<Style>(STYLE) {
        Ok(theme) => {
            let style = std::sync::Arc::new(theme);
            ctx.set_global_style(style);
        }
        Err(e) => log::error!("Error setting theme: {e:?}"),
    };
}

const STYLE: &str = r#"{"override_text_style":null,"override_font_id":null,"override_text_valign":"Center","text_styles":{"Small":{"size":9.0,"family":"Proportional"},"Body":{"size":13.0,"family":"Proportional"},"Monospace":{"size":13.0,"family":"Monospace"},"Button":{"size":13.0,"family":"Proportional"},"Heading":{"size":18.0,"family":"Proportional"}},"drag_value_text_style":"Button","wrap":null,"wrap_mode":null,"spacing":{"item_spacing":{"x":3.0,"y":3.0},"window_margin":{"left":2,"right":2,"top":2,"bottom":2},"button_padding":{"x":4.0,"y":1.0},"menu_margin":{"left":5,"right":5,"top":5,"bottom":5},"indent":18.0,"interact_size":{"x":40.0,"y":18.0},"slider_width":100.0,"slider_rail_height":8.0,"combo_width":100.0,"text_edit_width":280.0,"icon_width":14.0,"icon_width_inner":8.0,"icon_spacing":4.0,"default_area_size":{"x":600.0,"y":400.0},"tooltip_width":500.0,"menu_width":400.0,"menu_spacing":2.0,"indent_ends_with_horizontal_line":false,"combo_height":200.0,"scroll":{"floating":true,"content_margin":{"left":0,"right":0,"top":0,"bottom":0},"bar_width":10.0,"handle_min_length":12.0,"bar_inner_margin":4.0,"bar_outer_margin":0.0,"floating_width":2.0,"floating_allocated_width":0.0,"foreground_color":true,"dormant_background_opacity":0.0,"active_background_opacity":0.4,"interact_background_opacity":0.7,"dormant_handle_opacity":0.0,"active_handle_opacity":0.6,"interact_handle_opacity":1.0,"fade":{"strength":0.0,"size":0.0}}},"interaction":{"interact_radius":5.0,"resize_grab_radius_side":3.0,"resize_grab_radius_corner":10.0,"show_tooltips_only_when_still":true,"tooltip_delay":0.5,"tooltip_grace_time":0.2,"selectable_labels":true,"multi_widget_text_select":true},"visuals":{"dark_mode":true,"text_options":{"max_texture_side":2048,"alpha_from_coverage":"TwoCoverageMinusCoverageSq","font_hinting":true},"override_text_color":[232,232,232,255],"weak_text_alpha":0.6,"weak_text_color":null,"widgets":{"noninteractive":{"bg_fill":[6,6,6,255],"weak_bg_fill":[6,6,6,255],"bg_stroke":{"width":1.0,"color":[17,17,21,87]},"corner_radius":{"nw":2,"ne":2,"sw":2,"se":2},"fg_stroke":{"width":1.0,"color":[232,232,232,255]},"expansion":0.0},"inactive":{"bg_fill":[12,12,12,255],"weak_bg_fill":[0,0,0,255],"bg_stroke":{"width":0.6,"color":[50,52,77,129]},"corner_radius":{"nw":2,"ne":2,"sw":2,"se":2},"fg_stroke":{"width":1.0,"color":[232,232,232,255]},"expansion":0.0},"hovered":{"bg_fill":[7,7,7,255],"weak_bg_fill":[36,34,53,255],"bg_stroke":{"width":0.5,"color":[116,109,187,218]},"corner_radius":{"nw":3,"ne":3,"sw":3,"se":3},"fg_stroke":{"width":1.5,"color":[232,232,232,255]},"expansion":0.0},"active":{"bg_fill":[0,0,0,255],"weak_bg_fill":[118,26,60,118],"bg_stroke":{"width":1.0,"color":[11,11,11,255]},"corner_radius":{"nw":2,"ne":2,"sw":2,"se":2},"fg_stroke":{"width":2.0,"color":[232,232,232,255]},"expansion":0.0},"open":{"bg_fill":[3,3,3,255],"weak_bg_fill":[3,3,3,255],"bg_stroke":{"width":1.0,"color":[48,47,64,221]},"corner_radius":{"nw":2,"ne":2,"sw":2,"se":2},"fg_stroke":{"width":1.0,"color":[232,232,232,255]},"expansion":0.0}},"selection":{"bg_fill":[108,60,118,118],"stroke":{"width":1.0,"color":[76,77,103,247]}},"hyperlink_color":[84,71,226,255],"faint_bg_color":[16,16,16,255],"extreme_bg_color":[13,13,18,255],"text_edit_bg_color":null,"code_bg_color":[6,6,6,255],"warn_fg_color":[76,219,255,255],"error_fg_color":[255,73,137,255],"window_corner_radius":{"nw":4,"ne":4,"sw":4,"se":4},"window_shadow":{"offset":[0,0],"blur":5,"spread":7,"color":[2,2,2,164]},"window_fill":[0,0,0,255],"window_stroke":{"width":1.0,"color":[24,24,34,73]},"window_highlight_topmost":true,"menu_corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"panel_fill":[0,0,0,255],"popup_shadow":{"offset":[0,0],"blur":5,"spread":5,"color":[0,0,0,96]},"resize_corner_size":12.0,"text_cursor":{"stroke":{"width":2.0,"color":[189,221,255,255]},"preview":false,"blink":true,"on_duration":0.5,"off_duration":0.5},"button_frame":true,"collapsing_header_frame":true,"indent_has_left_vline":true,"striped":true,"slider_trailing_fill":true,"handle_shape":{"Rect":{"aspect_ratio":0.75}},"interact_cursor":null,"image_loading_spinners":true,"numeric_color_space":"GammaByte","disabled_alpha":0.5},"animation_time":0.1,"explanation_tooltips":false,"url_in_tooltip":true,"always_scroll_the_only_direction":false,"scroll_animation":{"points_per_second":5000.0,"duration":{"min":0.1,"max":0.3}},"compact_menu_style":true}"#;


fn load_icon() -> eframe::egui::IconData {
    let (icon_rgba, _icon_width, _icon_height) = {
        let icon = include_bytes!("assets/QcApp.ico");
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