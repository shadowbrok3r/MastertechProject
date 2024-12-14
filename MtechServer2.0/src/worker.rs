use wasm_bindgen::prelude::*;
pub use wasm_bindgen_rayon::init_thread_pool;

pub fn main() {
    gloo_console::info!("Worker started");

    // // Initialize the thread pool for Rayon
    // let num_threads = web_sys::window()
    //     .unwrap()
    //     .navigator()
    //     .hardware_concurrency() as usize; // Cast f64 to usize

    // // Define the closure for handling successful thread pool initialization
    // let success_closure = Closure::wrap(Box::new(move |_value: JsValue| {
    //     gloo_console::info!(format!("Initialized worker with {num_threads} threads"));
    // }) as Box<dyn FnMut(JsValue)>);

    // // Define the closure for handling errors in thread pool initialization
    // let error_closure = Closure::wrap(Box::new(move |err: JsValue| {
    //     gloo_console::error!(format!("Error initializing thread pool: {:?} with num threads: {num_threads}", err));
    // }) as Box<dyn FnMut(JsValue)>);

    // // Define the closure for finalization
    // let finally_closure = Closure::wrap(Box::new(move || {
    //     gloo_console::info!("Initialization complete.");
    // }) as Box<dyn FnMut()>);

    // // Handle the Promise from init_thread_pool, chaining the .then(), .catch(), and .finally() methods
    // let promise = init_thread_pool(num_threads);
    // let _ = promise
    //     .then(&success_closure)
    //     .catch(&error_closure)
    //     .finally(&finally_closure); // Forget the closure so it isn't dropped prematurely

    // // Forget all closures so they aren't dropped prematurely
    // success_closure.forget();
    // error_closure.forget();
    // finally_closure.forget();
}
