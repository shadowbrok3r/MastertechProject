use database::schema::{ChatAction, ChatThread, User, UserMessage};
use crossbeam::channel::{Receiver, Sender};
use std::collections::{HashMap, HashSet};
use crate::get_current_user_from_auth;
use database::{live_data::Action, schema::RecordId};
use serde::Serialize;
pub mod data;
pub mod ui;

#[derive(Debug, Clone, Serialize)]
pub struct UserChat {
    chat_title: String,
    pub selected_thread: Option<ChatThread>,
    edit_title: bool,
    thread_messages: HashMap<RecordId, Vec<UserMessage>>,
    threads: Vec<ChatThread>,
    current_user: User,
    store_users: Vec<User>,
    #[serde(skip)]
    chat_action_tx: Sender<ChatAction>,
    #[serde(skip)]
    chat_action_rx: Receiver<ChatAction>,
    #[serde(skip)]
    thread_listener_tx: Sender<(Action, ChatThread)>,
    #[serde(skip)]
    thread_listener_rx: Receiver<(Action, ChatThread)>,
    #[serde(skip)]
    message_listener_tx: Sender<(Action, UserMessage)>,
    #[serde(skip)]
    message_listener_rx: Receiver<(Action, UserMessage)>,
    #[serde(skip)]
    thread_tx: Sender<ChatThread>,
    #[serde(skip)]
    thread_rx: Receiver<ChatThread>,
    #[serde(skip)]
    chat_msg_tx: Sender<UserMessage>,
    #[serde(skip)]
    chat_msg_rx: Receiver<UserMessage>,
    image_id: String,
    first_run:  bool,
    /// The chat tab has been opened and wants its live streams; the shared
    /// reconnect supervisor spawns (and re-spawns) them.
    streams_requested: bool,
    input: String,
    edit_text: HashMap<String, UserMessage>,
    allow_edit: HashSet<String>,
    open: bool
}

impl UserChat {
    pub fn wants_streams(&self) -> bool {
        self.streams_requested
    }

    /// Senders the supervisor-managed chat live streams feed into.
    pub fn live_stream_senders(&self) -> (Sender<(Action, ChatThread)>, Sender<(Action, UserMessage)>) {
        (self.thread_listener_tx.clone(), self.message_listener_tx.clone())
    }
}

impl Default for UserChat {
    fn default() -> Self {
        let (chat_action_tx, chat_action_rx) = crossbeam::channel::unbounded();
        let (thread_listener_tx, thread_listener_rx) = crossbeam::channel::unbounded();
        let (message_listener_tx, message_listener_rx) = crossbeam::channel::unbounded();
        let (chat_msg_tx, chat_msg_rx) = crossbeam::channel::unbounded();
        let (thread_tx, thread_rx) = crossbeam::channel::unbounded();

        Self {
            chat_title: String::new(),
            selected_thread: None,
            thread_messages: HashMap::new(),
            edit_title: false,
            threads: Vec::new(),
            chat_action_tx, chat_action_rx,
            thread_listener_tx, thread_listener_rx,
            message_listener_tx, message_listener_rx,
            chat_msg_tx, chat_msg_rx,
            thread_tx, thread_rx,
            image_id: String::new(),
            current_user: if let Some(user) = get_current_user_from_auth() {
                user
            } else {
                User::default()
            },
            store_users: vec![],
            first_run: true,
            streams_requested: false,
            input: String::new(),
            edit_text: HashMap::new(),
            allow_edit: HashSet::new(),
            open: false,
        }
    }
}
