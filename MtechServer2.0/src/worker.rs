// use gloo_worker::Registrable;
// use mtechserver::webworker;
use wasm_bindgen::prelude::*;
use wasm_bindgen_rayon::init_thread_pool;
use web_sys::console;
// fn main() {
//     webworker::WebWorker::registrar().register();
// }

#[wasm_bindgen]
pub fn start_worker() {
    console::log_1(&"Worker started".into());

    // Initialize the thread pool for Rayon
    let num_threads = web_sys::window()
        .unwrap()
        .navigator()
        .hardware_concurrency() as usize; // Cast f64 to usize

    // Wrap the Rust closure in a `Closure`
    let closure = Closure::wrap(Box::new(move |_value: JsValue| {
        console::log_1(&format!("Initialized worker with {} threads", num_threads).into());
    }) as Box<dyn FnMut(JsValue)>);

    // Handle the Promise from init_thread_pool, chaining the .then() and .catch() methods
    let promise = init_thread_pool(num_threads);
    promise.then(&closure);

    // Forget the closure so it isn't dropped prematurely
    closure.forget();
}

fn main() {
    // Web Workers don't need a `main()` function when targeting Wasm.
    // This is just to satisfy the Rust compiler for non-Wasm targets.
}
