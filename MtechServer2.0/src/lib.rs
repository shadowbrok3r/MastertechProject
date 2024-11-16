pub mod app_state;
pub mod data;
pub mod first_run;
pub mod mtechserver;
pub mod pages;
pub mod tabs;
pub mod utilities;
pub mod webworker;
pub mod worker;
// Re-export MtechServer to make it accessible from the crate root
pub use app_state::MtechServer;

use wasm_bindgen::prelude::*;
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn run() {
    use eframe::wasm_bindgen::JsCast as _;
    use log::LevelFilter;
    use tabs::logger::logging::builder;
    use wasm_bindgen::prelude::*;
    use web_sys::HtmlCanvasElement;

    builder().init().unwrap();
    // Redirect `log` message to `console.log` and friends:
    // eframe::WebLogger::init(log::LevelFilter::Debug).ok();

    let web_options = eframe::WebOptions::default();

    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window()
            .expect("No window")
            .document()
            .expect("No document");

        let canvas = document
            .get_element_by_id("mtech_canvas")
            .expect("Failed to find the_canvas_id")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("the_canvas_id was not a HtmlCanvasElement");

        let start_result = eframe::WebRunner::new()
            .start(
                canvas,
                web_options,
                Box::new(|cc| {
                    egui_extras::install_image_loaders(&cc.egui_ctx);
                    Ok(Box::new(MtechServer::new(cc)))
                }),
            )
            .await
            .unwrap();

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
