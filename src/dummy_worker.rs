use gloo_worker::Registrable;
use mtechserver_two::webworker;
fn main() {
    webworker::WebWorker::registrar().register();
}