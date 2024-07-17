use gloo_worker::Registrable;
use mtechserver::webworker;
fn main() {
    webworker::WebWorker::registrar().register();
    webworker::LiveWorker::registrar().register();
}