pub struct ApiAccess {
    pub url: String,
    access_key: String,
    secret_key: String,
}

pub fn send_input_to_worker(&self, url: String, access_key: String, secret_key: String) {
    let input = Input {
        url,
        access_key,
        secret_key,
    };
    self.worker_bridge.send(input);
}
