use displays::{app_state::SharedContext, channel_manager::ChannelManager};
use egui_dock::{DockState, Node, NodeIndex, SurfaceIndex};
use crossbeam::channel::{Receiver, Sender};
use database::schema::UserSettings;
use eframe::CreationContext;
use std::collections::HashSet;
use serde::Serialize;

#[derive(Serialize)]
pub struct MtechServer {
    pub context: MtechServerContext,
    #[serde(skip)]
    pub tree: DockState<String>,
}

#[derive(Serialize)]
pub struct MtechServerContext {
    pub shared_ctx: SharedContext,
    #[serde(skip)]
    pub bytes_channel: (Sender<(Vec<u8>, u64)>, Receiver<(Vec<u8>, u64)>),
    
    // UI and Application State Fields
    /// {Widgets / Modals / Ui for portions throughout the app}
    pub search_input: String,

    /// {Open tabs in the UI}
    pub open_tabs: HashSet<String>,
    #[serde(skip)]
    pub added_nodes: Vec<(SurfaceIndex, NodeIndex)>,

    // System Data and Settings
    pub user_settings: UserSettings,
    pub update_settings: bool,
    pub get_settings: bool,

    // Miscellaneous Fields
    /// When downloading mastertech from the website
    pub total_download_size: f32,
    /// progress of downloading mastertech
    pub download_progress: f32,

    // // Webworker Communication
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
        let tree = default_tree();
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

        let shared_ctx = SharedContext::new(cc, tree.0.clone());
        let admin_console_data_helper = AdminConsoleDataHelper::new();
        
        let context = MtechServerContext {
            shared_ctx,
            bridge,
            data_update,

            // CHANNEL SENDERS / RECEIVERS
            bytes_channel,

            search_input: String::new(),
            open_tabs: tree.1,
            added_nodes: Vec::new(),
            total_download_size: 0.0,
            download_progress: 0.0,
            user_settings: UserSettings::default(),
            update_settings: false,
            get_settings: true,
            admin_console_data_helper,
        };

        Self {
            context,
            tree: tree.0,
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

pub fn default_tree() -> (DockState<String>, HashSet<String>) {
    let mut open_tabs = HashSet::new();
    let mut tree = DockState::new(vec![
        "Store Tasks".to_owned(),
        "Completed Tasks".to_owned(),
        "Company Stock".to_owned(),
        // "Customers".to_owned(),
        // "Database Editor".to_owned(),
        "Store Stock".to_owned(),
        "Logs".to_owned(),
    ]);

    // let [_a, b] =
    //     tree.main_surface_mut()
    //         .split_below(NodeIndex::root(), 0.65, vec!["My Tools".to_owned()]);

    // let [_, _] = tree
    //     .main_surface_mut()
    //     .split_right(b, 0.5, vec!["Bug Report".to_owned()]);

    // "Terminal".to_owned(),

    let [_, _] = tree.main_surface_mut().split_below(// .split_left(
        NodeIndex::root(), // b,
        0.6,
        vec![
            "My Tasks".to_owned(),
            "Bug Report".to_owned(),
            // "Task Audit".to_owned(),
            "Ai".to_owned(),
        ],
    );

    tree.translations.tab_context_menu.eject_button = "Undock".to_owned();

    for node in tree[SurfaceIndex::main()].iter() {
        if let Node::Leaf { tabs, .. } = node {
            for tab in tabs {
                open_tabs.insert(tab.clone());
            }
        }
    }

    (tree, open_tabs)
}

#[cfg(target_arch="wasm32")]
pub fn check_authentication(db_tx: Sender<anyhow::Result<database::Database, anyhow::Error>>) -> Result<(displays::app_state::AppState, Option<database::schema::User>), anyhow::Error> {

    let cookie = wasm_cookies::get("jwt");
    let user_cookie: Option<Result<String, wasm_cookies::FromUrlEncodingError>> = wasm_cookies::get("user");
    let mut state = displays::app_state::AppState::default();
    let mut current_user: Option<database::schema::User> = None;
    if let (Some(cookie), Some(Ok(usr))) = (cookie, user_cookie) {
        use base64::{engine::general_purpose, Engine as _};
        fn decompress_string(input: &[u8]) -> String {
            let mut decompressed = Vec::new();
            let mut decompressor = brotli::Decompressor::new(input, 4096);
            std::io::copy(&mut decompressor, &mut decompressed).unwrap();
            String::from_utf8(decompressed).unwrap()
        }
        
        let decoded = general_purpose::STANDARD.decode(&usr)?;
        let decompressed = decompress_string(&decoded);
        
        current_user = Some(serde_json::from_str(&decompressed)?);
        log::info!("Deompressed data: {}\nDecoded: {}\nOriginal: {}", decompressed.len(), decoded.len(), usr.len());
        
        let _user = current_user.clone();
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
                        Err(e) => gloo_console::error!(format!("Error converting user to json: {e:?}"))
                    }
                }
            }
            let _ = db_tx.try_send(database);
        });
        state = displays::app_state::AppState::Authenticated(displays::app_state::MainPages::Tasks);
    }
    // log::info!("State // user   {:?} // {:?}", state, current_user);
    Ok((state, current_user))
}