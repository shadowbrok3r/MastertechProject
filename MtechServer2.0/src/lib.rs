#![allow(dead_code)]
#![allow(unused_must_use)]

pub mod app_state;
pub mod data;
pub mod first_run;
pub mod mtechserver;
pub mod pages;
pub mod tabs;
pub mod webworker;
pub mod deser_worker;

// pub mod worker;
// Re-export MtechServer to make it accessible from the crate root
pub use app_state::MtechServer;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn run() {
    use eframe::wasm_bindgen::JsCast as _;
    // use log::LevelFilter;
    // use displays::tabs::logger::logging::builder;
    use web_sys::HtmlCanvasElement;

    gloo_console::info!("INIT LOGGER");
    egui_logger::builder().init();
    // Redirect `log` message to `console.log` and friends:
    // eframe::WebLogger::init(log::LevelFilter::Debug).ok();

    let web_options = eframe::WebOptions::default();

    gloo_console::info!("Spawn App");
    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window()
            .expect("No window")
            .document()
            .expect("No document");

        let canvas = document
            .get_element_by_id("mtech_canvas")
            .expect("Failed to find the_canvas_id")
            .dyn_into::<HtmlCanvasElement>()
            .expect("the_canvas_id was not a HtmlCanvasElement");

        gloo_console::info!("Starting web runner");
        let start_result = eframe::WebRunner::new()
            .start(
                canvas,
                web_options,
                Box::new(|cc| {
                    gloo_console::info!("Installing image loaders");
                    egui_extras::install_image_loaders(&cc.egui_ctx);
                    gloo_console::info!("Init Main");
                    Ok(Box::new(MtechServer::new(cc)))
                }),
            )
            .await;

        if let Err(e) = start_result {
            gloo_console::info!(format!("Encountered an Error: {e:?}"));
            if let Some(window) = web_sys::window() {
                if let Ok(storage) = window.local_storage() {
                    if let Some(storage) = storage {
                        let clear = storage.clear();
                        gloo_console::info!(format!("Clearing storage: {clear:?}"));
                    }
                }
            }
        }
        // // Remove the loading text and spinner:
        // if let Some(loading_text) = document.get_element_by_id("loading_text") {
        //     match start_result {
        //         Ok(_) => {
        //             loading_text.remove();
        //         }
        //         Err(e) => {
        //             loading_text.set_inner_html(
        //                 "<p> The app has crashed. See the developer console for details. </p>",
        //             );
        //             panic!("Failed to start eframe: {e:?}");
        //         }
        //     }
        // }
    });
}
