use displays::{app_state::SharedContext, channel_manager::ChannelManager};
use crossbeam::channel::{Receiver, Sender};
use eframe::CreationContext;
use serde::Serialize;

#[derive(Serialize)]
pub struct MtechServer {
    pub shared_ctx: SharedContext,
    #[serde(skip)]
    pub bytes_channel: (Sender<(Vec<u8>, u64)>, Receiver<(Vec<u8>, u64)>),
    


    // Webworker Communication
    #[serde(skip)]
    pub data_update: std::rc::Rc<
        std::cell::Cell<
            Option<
                Vec<u8>
            >
        >
    >,
    /// The actual communication bridge to / from our dummy worker
    #[serde(skip)]
    pub bridge: gloo_worker::WorkerBridge<crate::webworker::WebWorker>,
    #[serde(skip)]
    pub admin_console_data_helper: AdminConsoleDataHelper,
}

impl MtechServer {
    pub fn new(cc: &CreationContext<'_>) -> Self {
        let bytes_channel = <(Vec<u8>, u64)>::create_unbounded_channel();
        let data_update = std::rc::Rc::new(std::cell::Cell::new(None));
        let sender = data_update.clone();
        let ctx = cc.egui_ctx.clone();
        let bridge = <crate::webworker::WebWorker as gloo_worker::Spawnable>::spawner()
            .callback(move |response| {
                sender.set(Some(response.tasks));
                ctx.request_repaint();
            })
            .spawn("./webworker.js");
        // let tree = default_tree_wasm();
        let admin_console_data_helper = AdminConsoleDataHelper::new();
        
        Self {
            shared_ctx: SharedContext::new(cc),
            bridge,
            data_update,

            // CHANNEL SENDERS / RECEIVERS
            bytes_channel,

            admin_console_data_helper,
        }
    }
}

pub struct AdminConsoleDataHelper {
    pub deser_data_update: std::rc::Rc<std::cell::Cell<Option<Vec<u8>>>>,
    /// The actual communication bridge to / from our dummy worker
    pub deser_bridge: gloo_worker::WorkerBridge<crate::deser_worker::DeserWorker>,
}

impl AdminConsoleDataHelper {
    pub fn new() -> Self {
        let deser_data_update = std::rc::Rc::new(std::cell::Cell::new(None));
        let deser_sender = deser_data_update.clone();
        let deser_bridge = <crate::deser_worker::DeserWorker as gloo_worker::Spawnable>::spawner()
        .callback(move |response| {
            deser_sender.set(Some(response.0));
        })
        .spawn("./deser_worker.js");

        Self {
            deser_data_update,
            deser_bridge,
        }
    }
}

// impl BinMsgHandler for AdminConsoleDataHelper {
//     fn handle_binary_message(&mut self, bin: &Vec<u8>) -> Vec<u8> {
//         self.deser_bridge.send(crate::deser_worker::Input(bin.clone()));
//         bin.to_vec()
//     }
// }

#[cfg(target_arch="wasm32")]
pub fn check_authentication(db_tx: Sender<anyhow::Result<database::Database, anyhow::Error>>) -> Result<displays::app_state::AppState, anyhow::Error> {
    let mut state = displays::app_state::AppState::default();
    if let Some(cookie) = wasm_cookies::get("jwt") {
        let db_tx = db_tx.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let database = database::Database::new("".to_string(), "".to_string(), Some(cookie.unwrap())).await;
            log::warn!("Checking User");
            if let Ok(db) = &database {
                log::warn!("Database Ok");
                if let Some(usr) = &db.user {
                    log::warn!("Got a user");
                    match serde_json::to_string(&usr) {
                        Ok(usr_json) => {
                            log::warn!("Deleting existing user cookie");
                            wasm_cookies::delete("user");
                            log::warn!("Compressing user data");
                            use brotli::CompressorReader;
                            use base64::{engine::general_purpose, Engine as _};

                            fn compress_string(input: &str) -> Vec<u8> {
                                let mut compressed = Vec::new();
                                {
                                    let mut compressor = CompressorReader::new(input.as_bytes(), 4096, 11, 22);
                                    std::io::copy(&mut compressor, &mut compressed).unwrap();
                                }
                                compressed
                            }

                            let compressed: Vec<u8> = compress_string(&usr_json);
                            let encoded: String = general_purpose::STANDARD.encode(&compressed);
                            log::info!("Compressed data: {}\nEncoded: {}\nOriginal: {}", compressed.len(), encoded.len(), usr_json.len());

                            wasm_cookies::set(
                                "user", 
                                &encoded, 
                                &wasm_cookies::CookieOptions::default()
                                .with_same_site(wasm_cookies::SameSite::Strict)
                                .secure()
                                .expires_after(web_time::Duration::from_secs(172800))
                            );
                            log::warn!("Set new user cookie");
                        },
                        Err(e) => {
                            gloo_console::error!(format!("Error converting user to json: {e:?}"));
                        }
                    }
                }
            }
            let _ = db_tx.try_send(database);
        });
        state = displays::app_state::AppState::Authenticated(displays::app_state::MainPages::Tasks);
    }
    // log::info!("State // user   {:?} // {:?}", state, current_user);
    Ok(state)
}