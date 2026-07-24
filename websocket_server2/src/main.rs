use axum::{
    extract::{
        ws::{Message, WebSocket},
        Extension, Query, WebSocketUpgrade,
    },
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    serve, Router,
};
use database::{init_database, schema::{ConnectedClient, User}, Database, db};
use futures::{stream::{SplitSink, SplitStream}, SinkExt, StreamExt};
use std::{collections::HashMap, sync::Arc};
use std::net::SocketAddr;
use tokio::sync::{oneshot, Mutex};
use tracing::{info, warn};
use uuid::Uuid;

type SessionID = String;
type RoomID = String;

#[derive(Debug)]
enum ChatMessage {
    Send {
        from: SessionID,
        room_id: RoomID,
        text: String,
        bin: Option<Vec<u8>>,
        ping: Option<Vec<u8>>,
        pong: Option<Vec<u8>>,
    },
    Command {
        from: SessionID,
        ws_tx: Arc<Mutex<SplitSink<WebSocket, Message>>>,
        command: String,
        args: Vec<String>,
    },
}

use std::time::{Duration, Instant};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

#[derive(Clone, Debug, Default)]
struct Room {
    master: Option<Arc<Mutex<SplitSink<WebSocket, Message>>>>,
    client: Option<Arc<Mutex<SplitSink<WebSocket, Message>>>>,
}

/// Global rate limiter: max DB writes across ALL rooms within a rolling window.
/// Each connected_client UPDATE triggers live query notifications to every client
/// listening on that table, so capping total writes prevents SurrealDB overload.
const GLOBAL_MAX_WRITES_PER_WINDOW: u64 = 10;
const GLOBAL_WINDOW_SECS: u64 = 60;

static GLOBAL_WRITE_COUNT: AtomicU64 = AtomicU64::new(0);
static GLOBAL_WINDOW_START: AtomicU64 = AtomicU64::new(0);

fn global_write_allowed() -> bool {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let window_start = GLOBAL_WINDOW_START.load(Ordering::Relaxed);

    if now.saturating_sub(window_start) >= GLOBAL_WINDOW_SECS {
        GLOBAL_WINDOW_START.store(now, Ordering::Relaxed);
        GLOBAL_WRITE_COUNT.store(1, Ordering::Relaxed);
        return true;
    }

    let count = GLOBAL_WRITE_COUNT.fetch_add(1, Ordering::Relaxed);
    if count < GLOBAL_MAX_WRITES_PER_WINDOW {
        true
    } else {
        GLOBAL_WRITE_COUNT.fetch_sub(1, Ordering::Relaxed);
        false
    }
}

/// Startup configuration read once from the environment.
struct Config {
    ws_activity_write_secs: u64,
    tunnel_pending_ttl_secs: u64,
    ws_pong_timeout_secs: u64,
    tunnel_idle_secs: u64,
}

impl Config {
    fn from_env() -> Self {
        let ws_activity_write_secs = std::env::var("WS_ACTIVITY_WRITE_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(300);
        let tunnel_pending_ttl_secs = std::env::var("TUNNEL_PENDING_TTL_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(30);
        let ws_pong_timeout_secs = std::env::var("WS_PONG_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(35);
        let tunnel_idle_secs = std::env::var("TUNNEL_IDLE_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(90);
        Self { ws_activity_write_secs, tunnel_pending_ttl_secs, ws_pong_timeout_secs, tunnel_idle_secs }
    }
}

fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

static CONFIG: OnceLock<Config> = OnceLock::new();

fn config() -> &'static Config {
    CONFIG.get_or_init(Config::from_env)
}

const MAX_TUNNEL_PENDING: usize = 256;
const MAX_TUNNEL_ACTIVE: usize = 512;
const MAX_TUNNEL_BUFFER: usize = 64 * 1024;

type WsSink = SplitSink<WebSocket, Message>;
type WsStream = SplitStream<WebSocket>;
type TunnelSlots = Arc<Mutex<HashMap<String, TunnelSlot>>>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum TunnelRole {
    Master,
    Client,
}

impl TunnelRole {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "master" => Some(Self::Master),
            "client" => Some(Self::Client),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Master => "master",
            Self::Client => "client",
        }
    }
}

/// Halves and buffered bytes handed from a parked socket to the pairing task.
struct ParkedParts {
    sink: WsSink,
    stream: WsStream,
    buffered: Vec<Message>,
}

/// A single socket waiting for its opposite-role peer.
struct Pending {
    role: TunnelRole,
    reclaim: oneshot::Sender<oneshot::Sender<ParkedParts>>,
}

enum TunnelSlot {
    Pending(Pending),
    Active,
}

#[derive(Clone)]
struct ChatServer {
    rooms: Arc<Mutex<HashMap<RoomID, Room>>>,
    session_map: Arc<Mutex<HashMap<SessionID, Arc<Mutex<SplitSink<WebSocket, Message>>>>>>,
    // Map session_id to username if authenticated
    user_map: Arc<Mutex<HashMap<SessionID, User>>>,
}

impl ChatServer {
    async fn handle_ws(
        self: Arc<Self>,
        ws: WebSocket,
        session_id: SessionID,
        room_id: RoomID,
        role: String,
    ) {
        let (ws_tx, mut ws_rx) = ws.split();
        let ws_tx = Arc::new(Mutex::new(ws_tx));


        let mut rooms = self.rooms.lock().await;
        let entry = rooms.entry(room_id.clone()).or_insert_with(Room::default);

        // Assign session, overwriting stale entries
        match role.as_str() {
            "master" => {
                // If there was an old master, notify it's being replaced (shouldn't happen often)
                if entry.master.is_some() {
                    info!("Replacing stale master session in room {}", room_id);
                }
                
                // Notify client that a master has connected
                let mut client_stale = false;
                if let Some(client_tx) = entry.client.clone() {
                    match client_tx.lock().await.send(Message::Text("MASTER_CONNECTED".into())).await {
                        Ok(_) => info!("Notified client in room {} that master connected", room_id),
                        Err(e) => {
                            info!("Failed to notify client in room {} (may be stale): {:?}", room_id, e);
                            client_stale = true;
                        }
                    }
                }
                // Client connection is stale, remove it
                if client_stale {
                    entry.client = None;
                }
                entry.master = Some(ws_tx.clone());
                info!("Master session {} assigned to room {}", session_id, room_id);
            }
            "client" => {
                // If there was an old client, notify it's being replaced
                if entry.client.is_some() {
                    info!("Replacing stale client session in room {}", room_id);
                }
                
                // Notify master that a client has connected
                let mut master_stale = false;
                if let Some(master_tx) = entry.master.clone() {
                    match master_tx.lock().await.send(Message::Text("CLIENT_CONNECTED".into())).await {
                        Ok(_) => info!("Notified master in room {} that client connected", room_id),
                        Err(e) => {
                            info!("Failed to notify master in room {} (may be stale): {:?}", room_id, e);
                            master_stale = true;
                        }
                    }
                }
                // Master connection is stale, remove it
                if master_stale {
                    entry.master = None;
                }
                entry.client = Some(ws_tx.clone());
                info!("Client session {} assigned to room {}", session_id, room_id);
            }
            _ => {
                let _ = ws_tx.lock().await.send(Message::Text("Invalid role".into())).await;
                return;
            }
        }
        
        // Release rooms lock before spawning tasks
        drop(rooms);

        // Update ConnectedClient in SurrealDB for this connection
        let room_id_clone2 = room_id.clone();
        let user_map_clone = Arc::clone(&self.user_map);
        let sess = session_id.clone();
        tokio::spawn(async move {
            // Query the ConnectedClient for assigned_user
            if let Ok(client) = get_client(&room_id_clone2).await {
                if let Some(user_id) = client.assigned_user {
                    // Query the User by id
                    if let Ok(Some(user)) = db()
                        .query("SELECT * FROM user WHERE id == $user_id")
                        .bind(("user_id", user_id.clone()))
                        .await
                        .and_then(|mut r| r.take::<Option<User>>(0))
                    {
                        log::info!("SELECTED USER: {:?}", user.get_username());
                        let mut user_map = user_map_clone.lock().await;
                        user_map.insert(sess, user);
                    }
                }
            }
        });

        self.session_map
            .lock()
            .await
            .insert(session_id.clone(), ws_tx.clone());
        // Remove any previous user association for this session
        self.user_map.lock().await.remove(&session_id);

        let last_activity = Arc::new(AtomicU64::new(now_unix_secs()));

        let server_clone = Arc::clone(&self);
        let session_id_clone = session_id.clone();
        let room_id_clone = room_id.clone();
        let role_clone = role.clone();
        let ws_tx_for_incoming = ws_tx.clone();
        let last_activity_incoming = Arc::clone(&last_activity);

        // Handle incoming messages
        tokio::spawn(async move {
            let ws_tx = ws_tx_for_incoming;
            while let Some(Ok(message)) = ws_rx.next().await {
                last_activity_incoming.store(now_unix_secs(), Ordering::Relaxed);
                match message {
                    Message::Text(text) => {
                        // Command parsing: commands start with '/'
                        if text.starts_with("/") {
                            let mut parts = text[1..].split_whitespace();
                            if let Some(cmd) = parts.next() {
                                let args: Vec<String> = parts.map(|s| s.to_string()).collect();
                                // Clone ws_tx before moving it into the async handler
                                let ws_tx_clone = ws_tx.clone();
                                server_clone
                                    .handle_message(ChatMessage::Command {
                                        from: session_id_clone.clone(),
                                        ws_tx: ws_tx_clone,
                                        command: cmd.to_string(),
                                        args,
                                    })
                                    .await;
                                continue;
                            }
                        }
                        server_clone
                            .handle_message(ChatMessage::Send {
                                from: session_id_clone.clone(),
                                room_id: room_id_clone.clone(),
                                text: text.to_string(),
                                bin: None,
                                ping: None,
                                pong: None,
                            })
                            .await;
                    }
                    Message::Binary(bin) => {
                        server_clone
                            .handle_message(ChatMessage::Send {
                                from: session_id_clone.clone(),
                                room_id: room_id_clone.clone(),
                                text: String::new(),
                                bin: Some(bin.to_vec()),
                                ping: None,
                                pong: None,
                            })
                            .await;
                    }
                    Message::Close(close_frame) => {
                        if let Some(frame) = close_frame {
                            info!("WebSocket closed: {:?} {:?}", frame.reason, frame.code);
                        }

                        server_clone
                            .cleanup_session(&room_id_clone, &session_id_clone, &role_clone, &ws_tx)
                            .await?;
                        break;
                    }
                    Message::Ping(_bytes) => {},
                    Message::Pong(_bytes) => {},
                }
            }
            server_clone
                .cleanup_session(&room_id_clone, &session_id_clone, &role_clone, &ws_tx)
                .await?;
            Ok::<(), anyhow::Error>(())
        });

        let server_clone = Arc::clone(&self);
        let role_clone = role.clone();
        let ws_tx_for_ping = ws_tx.clone();
        let room_id_for_ping = room_id.clone();
        let session_id_for_ping = session_id.clone();
        let last_activity_ping = Arc::clone(&last_activity);

        // Ping task to detect disconnection and stamp activity at a bounded rate
        tokio::spawn(async move {
            let ws_tx = ws_tx_for_ping;
            let stamp_secs = config().ws_activity_write_secs;
            let pong_timeout = config().ws_pong_timeout_secs;
            let mut last_db_update: Option<Instant> = None;
            loop {
                tokio::time::sleep(Duration::from_secs(10)).await;
                let mut sender = ws_tx.lock().await;
                if let Err(e) = sender.send(Message::Ping(vec![].into())).await {
                    info!("WebSocket {} disconnected: {:?}", session_id_for_ping, e);
                    drop(sender);
                    server_clone
                        .cleanup_session(&room_id_for_ping, &session_id_for_ping, &role_clone, &ws_tx)
                        .await?;
                    break;
                }
                drop(sender);

                // Half-open detection: no inbound frame within the window means the socket is dead.
                if pong_timeout > 0 {
                    let idle = now_unix_secs().saturating_sub(last_activity_ping.load(Ordering::Relaxed));
                    if idle >= pong_timeout {
                        info!("WebSocket {} idle {}s with no pong; treating as dead", session_id_for_ping, idle);
                        server_clone
                            .cleanup_session(&room_id_for_ping, &session_id_for_ping, &role_clone, &ws_tx)
                            .await?;
                        break;
                    }
                }

                if stamp_secs == 0 {
                    continue;
                }
                let per_room_ok = match last_db_update {
                    Some(t) if t.elapsed() < Duration::from_secs(stamp_secs) => false,
                    _ => true,
                };
                if per_room_ok && global_write_allowed() {
                    let room_id_for_db = room_id_for_ping.clone();
                    tokio::spawn(async move {
                        let result: Result<Option<ConnectedClient>, _> = db()
                            .query("UPDATE connected_client SET last_update = time::now() WHERE connection_string == $room_id")
                            .bind(("room_id", room_id_for_db.clone()))
                            .await
                            .and_then(|mut r| r.take(0));

                        if let Err(e) = result {
                            log::warn!("Failed to update last_update from ping task for room {}: {:?}", room_id_for_db, e);
                        }
                    });
                    last_db_update = Some(Instant::now());
                }
            }
            Ok::<(), anyhow::Error>(())
        });

        // Note: Status broadcast task removed - activity status is now tracked
        // via SurrealDB connected_client.last_update field instead of websocket messages
    }

    async fn handle_message(&self, call: ChatMessage) {
        match call {
            ChatMessage::Send {
                from,
                room_id,
                text,
                bin,
                ping,
                pong
            } => {
                let mut rooms = self.rooms.lock().await;
                if let Some(room) = rooms.get_mut(&room_id) {
                    let is_from_master = self.is_session_match(room.master.as_ref(), &from).await;
                    let is_from_client = self.is_session_match(room.client.as_ref(), &from).await;

                    let target_session = if is_from_master {
                        room.client.clone()
                    } else if is_from_client {
                        room.master.clone()
                    } else {
                        None
                    };

                    if let Some(session) = target_session {
                        let target_arc = session.clone();
                        let mut session = session.lock().await;
                        let send_result = if let Some(bin) = bin {
                            info!("Binary message sent to target session in room {}", room_id);
                            session.send(Message::Binary(bin.into())).await
                        } else if let Some(ping) = ping {
                            session.send(Message::Ping(ping.into())).await
                        } else if let Some(pong) = pong {
                            session.send(Message::Pong(pong.into())).await
                        } else {
                            info!("{text} sent to target session in room {}", room_id);
                            session.send(Message::Text(text.into())).await
                        };
                        if let Err(e) = send_result {
                            log::error!("Failed to send message to session in room {}: {:?}", room_id, e);
                            drop(session);
                            // Clear the dead target only if it is still the same socket.
                            if is_from_master {
                                if room.client.as_ref().map(|s| Arc::ptr_eq(s, &target_arc)).unwrap_or(false) {
                                    room.client = None;
                                    info!("Client removed from room {} due to send failure", room_id);
                                }
                            } else if is_from_client {
                                if room.master.as_ref().map(|s| Arc::ptr_eq(s, &target_arc)).unwrap_or(false) {
                                    room.master = None;
                                    info!("Master removed from room {} due to send failure", room_id);
                                }
                            }
                        }
                    } else {
                        // No target session found - the other party isn't connected yet
                        // This is normal - just means the master hasn't connected to this room yet
                        // Don't mark the client as disconnected - they're still connected, just waiting
                        log::debug!("No target session in room {} - target may not have connected yet", room_id);
                    }
                } else {
                    info!("Room {} not found", room_id);
                }
            }
            ChatMessage::Command { from, ws_tx, command, args } => {
                match command.as_str() {
                    "users_online" => {
                        // List all unique users currently connected
                        let user_map = self.user_map.lock().await;
                        if user_map.is_empty() {
                            let mut sender = ws_tx.lock().await;
                            let _ = sender.send(Message::Text("No users are currently authenticated/connected.".into())).await;
                        } else {
                            let mut users = Vec::new();
                            for (_session, user) in user_map.iter() {
                                users.push(format!("{}", user.get_username()));
                            }
                            users.sort();
                            users.dedup();
                            let mut sender = ws_tx.lock().await;
                            let msg = if users.is_empty() {
                                "No users are currently authenticated/connected.".to_string()
                            } else {
                                format!("Users currently online ({}):\n{}", users.len(), users.join(", "))
                            };
                            let _ = sender.send(Message::Text(msg.into())).await;
                        }
                    }
                    "my_connections" => {
                        let rooms = self.rooms.lock().await;
                        for (room_id, room) in rooms.iter() {
                            if let Ok(client) = get_client(room_id).await {
                                let user_map = self.user_map.lock().await;
                                if let (Some(record_user), Some(usr)) = (&client.assigned_user, user_map.get(&from)) {
                                    if record_user == &usr.get_id() {
                                        let mut summary = String::from("Active rooms and sessions:\n");
                                            if room_id == &client.connection_string {
                                                summary.push_str(&format!("Room: {}\n", room_id));
                                                summary.push_str(&format!("  Master: {}\n", if room.master.is_some() { "connected" } else { "none" }));
                                                summary.push_str(&format!("  Client: {}\n", if room.client.is_some() { "connected" } else { "none" }));
                                                summary.push_str(&format!("Client: {:#?}\n", client));
                                            }
                                    }
                                }
                            }
                        }
                    }
                    "help" => {
                        let help_msg = r#"Available commands:

/help
    Show this help message.
/auth <username> <password>
    Authenticate with SurrealDB and associate your session with your account.
/my_connections
    List your own active connections (requires authentication).
/list
    List all active rooms and their connections.
/remove <room_id> <role>
    Remove a client from a room. Role is 'master' or 'client'.
/remove_room <room_id>
    Remove a room entirely.
"#;
                        let mut sender = ws_tx.lock().await;
                        let _ = sender.send(Message::Text(help_msg.into())).await;
                    }
                    "auth" => {
                        // Usage: /auth <username> <password>
                        if args.len() < 2 {
                            let mut sender = ws_tx.lock().await;
                            let _ = sender.send(Message::Text("Usage: /auth <username> <password>".into())).await;
                        } else {
                            let username = &args[0];
                            let password = &args[1];
                            // Try to sign in with SurrealDB
                            let mut sender = ws_tx.lock().await;
                            match Database::new(username.to_string(), password.to_string(), None).await {
                                Ok(db) => {
                                    // Associate session with username
                                    self.user_map.lock().await.insert(from.clone(), db.user.unwrap_or_default());
                                    let _ = sender.send(Message::Text(format!("Authenticated as {}", username).into())).await;
                                }
                                Err(e) => {
                                    let _ = sender.send(Message::Text(format!("Authentication failed: {e}").into())).await;
                                }
                            }
                        }
                    }
                    "list" => {
                        let rooms = self.rooms.lock().await;
                        let mut summary = String::from("Active rooms and sessions:\n");
                        for (room_id, room) in rooms.iter() {
                            summary.push_str(&format!("Room: {}\n", room_id));
                            summary.push_str(&format!("  Master: {}\n", if room.master.is_some() { "connected" } else { "none" }));
                            summary.push_str(&format!("  Client: {}\n", if room.client.is_some() { "connected" } else { "none" }));
                        }
                        let mut sender = ws_tx.lock().await;
                        let _ = sender.send(Message::Text(summary.into())).await;
                    }
                    "remove" => {
                        // Usage: /remove <room_id> <role>
                        if args.len() < 2 {
                            let mut sender = ws_tx.lock().await;
                            let _ = sender.send(Message::Text("Usage: /remove <room_id> <role>".into())).await;
                        } else {
                            let room_id = &args[0];
                            let role = &args[1];
                            let mut rooms = self.rooms.lock().await;
                            if let Some(room) = rooms.get_mut(room_id) {
                                match role.as_str() {
                                    "master" => room.master = None,
                                    "client" => room.client = None,
                                    _ => {}
                                }
                                let mut sender = ws_tx.lock().await;
                                let _ = sender.send(Message::Text(format!("Removed {} from room {}", role, room_id).into())).await;
                            } else {
                                let mut sender = ws_tx.lock().await;
                                let _ = sender.send(Message::Text(format!("Room {} not found", room_id).into())).await;
                            }
                        }
                    }
                    "remove_room" => {
                        // Usage: /remove_room <room_id>
                        if args.is_empty() {
                            let mut sender = ws_tx.lock().await;
                            let _ = sender.send(Message::Text("Usage: /remove_room <room_id>".into())).await;
                        } else {
                            let room_id = &args[0];
                            let mut rooms = self.rooms.lock().await;
                            if rooms.remove(room_id).is_some() {
                                let mut sender = ws_tx.lock().await;
                                let _ = sender.send(Message::Text(format!("Room {} removed", room_id).into())).await;
                            } else {
                                let mut sender = ws_tx.lock().await;
                                let _ = sender.send(Message::Text(format!("Room {} not found", room_id).into())).await;
                            }
                        }
                    }
                    _ => {
                        let mut sender = ws_tx.lock().await;
                        let _ = sender.send(Message::Text(format!("Unknown command: {}", command).into())).await;
                    }
                }
            }
        }
    }

    
    async fn cleanup_session(
        &self,
        room_id: &RoomID,
        session_id: &SessionID,
        role: &str,
        my_sink: &Arc<Mutex<SplitSink<WebSocket, Message>>>,
    ) -> anyhow::Result<(), anyhow::Error> {
        let mut rooms = self.rooms.lock().await;
        let mut session_map = self.session_map.lock().await;
        let mut user_map = self.user_map.lock().await;

        // In-memory maps are keyed on this session id; removal is always safe.
        session_map.remove(session_id);
        user_map.remove(session_id);

        // Only touch shared room/DB state when this socket still owns its slot;
        // a newer reconnect may have replaced it.
        let owns_slot = rooms.get(room_id).is_some_and(|room| {
            let slot = match role {
                "master" => room.master.as_ref(),
                "client" => room.client.as_ref(),
                _ => None,
            };
            slot.is_some_and(|s| Arc::ptr_eq(s, my_sink))
        });
        if !owns_slot {
            log::info!("cleanup_session: slot for {role} in room {room_id} already replaced; skipping");
            return Ok(());
        }

        // Best-effort DB write; a DB error must not skip the in-memory cleanup below.
        if role == "client" {
            let result: Result<Option<ConnectedClient>, _> = db()
                .query("UPDATE connected_client SET connected = false WHERE connection_string == $connection_id")
                .bind(("connection_id", room_id.clone()))
                .await
                .and_then(|mut r| r.take(0));
            match result {
                Ok(client) => log::info!("Client role disconnected, DB updated: {client:?}"),
                Err(e) => log::warn!("cleanup_session: connected=false write failed for room {room_id}: {e:?}"),
            }
        } else {
            log::info!("Master role disconnected from room {room_id}, DB not updated (client still connected)");
        }

        log::info!("Client disconnected");

        if let Some(room) = rooms.get_mut(room_id) {
            match role {
                "master" => {
                    room.master = None;
                    if let Some(client_tx) = &room.client {
                        let send_result = client_tx.lock().await.send(Message::Text("MASTER_DISCONNECTED".into())).await;
                        if send_result.is_ok() {
                            info!("Notified client in room {} that master disconnected", room_id);
                        }
                    }
                    info!("Master session {} removed from room {}", session_id, room_id);
                }
                "client" => {
                    room.client = None;
                    if let Some(master_tx) = &room.master {
                        let send_result = master_tx.lock().await.send(Message::Text("CLIENT_DISCONNECTED".into())).await;
                        if send_result.is_ok() {
                            info!("Notified master in room {} that client disconnected", room_id);
                        }
                    }
                    info!("Client session {} removed from room {}", session_id, room_id);
                }
                _ => {}
            }
        }
        Ok(())
    }

    async fn is_session_match(
        &self,
        session: Option<&Arc<Mutex<SplitSink<WebSocket, Message>>>>,
        session_id: &SessionID,
    ) -> bool {
        let session_map = self.session_map.lock().await;
        session
            .and_then(|s| session_map.get(session_id).map(|stored| Arc::ptr_eq(s, stored)))
            .unwrap_or(false)
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let cfg = config();
    info!(
        "Config: ws_activity_write_secs={}, tunnel_pending_ttl_secs={}, ws_pong_timeout_secs={}, tunnel_idle_secs={}",
        cfg.ws_activity_write_secs, cfg.tunnel_pending_ttl_secs, cfg.ws_pong_timeout_secs, cfg.tunnel_idle_secs
    );
    match init_database().await {
        Ok(_) => log::info!("Initialized Database"),
        Err(e) => log::info!("Error Initializing Database: {e:?}"),
    }

    let chat_server = ChatServer {
        rooms: Arc::new(Mutex::new(HashMap::new())),
        session_map: Arc::new(Mutex::new(HashMap::new())),
        user_map: Arc::new(Mutex::new(HashMap::new())),
    };

    let tunnel_slots: TunnelSlots = Arc::new(Mutex::new(HashMap::new()));

    let app = Router::new()
        .route("/websocket", get(websocket_handler))
        .route("/tunnel", get(tunnel_handler))
        .layer(Extension(Arc::new(chat_server)))
        .layer(Extension(tunnel_slots));

    let addr = SocketAddr::from(([0, 0, 0, 0], 8081));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!("Listening on {}", addr);
    let _ = tokio::spawn(async move {
        serve(listener, app).await?;
        Ok::<(), anyhow::Error>(())
    }).await;
    Ok(())
}

async fn websocket_handler(
    ws: WebSocketUpgrade,
    Query(params): Query<HashMap<String, String>>,
    Extension(chat_server): Extension<Arc<ChatServer>>,
) -> impl IntoResponse {
    let session_id = Uuid::new_v4().to_string();
    let room_id = params.get("room_id").cloned().unwrap_or_default();
    let role = if let Some(r) = params.get("role").cloned() {
        r
    } else {
        warn!(
            room_id = %room_id,
            session_id = %session_id,
            "WebSocket connected without role= query param; defaulting to client. \
             Admin consoles must use role=master or they replace the real remote client in the room."
        );
        "client".to_string()
    };

    info!("Client connected. Role: {:?}, Room: {:?}, Session: {:?}", role, room_id, session_id);

    let res = connect_client(room_id.clone()).await;
    println!("Res: {res:?}");
    ws.on_upgrade(move |socket| chat_server.handle_ws(socket, session_id, room_id, role))
}

async fn get_client(room_id: &String) -> anyhow::Result<ConnectedClient, anyhow::Error> {
    let potential_client: Option<ConnectedClient> = db()
        .query("SELECT * FROM connected_client WHERE connection_string == $room_id")
        .bind(("room_id", room_id.clone()))
        .await?
        .take(0)?;

    Ok(potential_client.unwrap_or_default())
}

/// Best-effort confirmation that a `connected_client` row exists for an
/// agent that just joined the relay. **Does not** write `connected = true`
/// any more — the agent's own `make_ws_connection` is now the sole writer
/// of that flag, so this function is read-only.
///
/// Previously the helper did a `SELECT … then UPDATE … if connected==false`,
/// which raced the agent's update under SurrealKV's snapshot isolation. Both
/// transactions could see `$before.connected = false` and both fire the
/// schema event in `database/schema/connected_client.surql`, producing
/// duplicate "Client Connected" notifications. The schema-side dedup
/// window catches that case as a safety net; removing the redundant
/// writer here is the primary fix.
pub async fn connect_client(room_id: String) -> anyhow::Result<(), anyhow::Error> {
    let potential_client: Option<ConnectedClient> = db()
        .query("SELECT * FROM connected_client WHERE connection_string == $room_id")
        .bind(("room_id", room_id.clone()))
        .await?
        .take(0)?;

    match potential_client {
        Some(client) => log::info!("Found client (read-only confirm): {client:?}"),
        None => log::warn!(
            "connect_client -> no row for room_id {room_id}; agent may not have CREATEd yet"
        ),
    }

    Ok(())
}

async fn tunnel_handler(
    ws: WebSocketUpgrade,
    Query(params): Query<HashMap<String, String>>,
    Extension(slots): Extension<TunnelSlots>,
) -> Response {
    let session = params.get("session").cloned().unwrap_or_default();
    let role_str = params.get("role").cloned().unwrap_or_default();

    if session.is_empty() || session.len() > 128 {
        warn!("tunnel rejected: session must be 1..=128 chars");
        return (StatusCode::BAD_REQUEST, "invalid session").into_response();
    }
    let Some(role) = TunnelRole::parse(&role_str) else {
        warn!("tunnel rejected: invalid role {:?}", role_str);
        return (StatusCode::BAD_REQUEST, "invalid role").into_response();
    };

    ws.on_upgrade(move |socket| tunnel_join(slots, session, role, socket))
}

async fn tunnel_join(slots: TunnelSlots, session: String, role: TunnelRole, ws: WebSocket) {
    let (sink, stream) = ws.split();
    let mut guard = slots.lock().await;

    let existing = match guard.get(&session) {
        Some(TunnelSlot::Active) => Some(None),
        Some(TunnelSlot::Pending(p)) => Some(Some(p.role)),
        None => None,
    };

    match existing {
        Some(None) => {
            drop(guard);
            warn!("tunnel session {} already paired; rejecting {}", session, role.as_str());
            close_sink(sink).await;
        }
        Some(Some(pending_role)) if pending_role == role => {
            drop(guard);
            warn!("tunnel session {} already has pending {}; rejecting duplicate", session, role.as_str());
            close_sink(sink).await;
        }
        Some(Some(_)) => {
            let Some(TunnelSlot::Pending(peer)) = guard.remove(&session) else { return };
            let active = guard.values().filter(|v| matches!(v, TunnelSlot::Active)).count();
            if active >= MAX_TUNNEL_ACTIVE {
                guard.insert(session.clone(), TunnelSlot::Pending(peer));
                drop(guard);
                warn!("tunnel active cap {} reached; rejecting {} for session {}", MAX_TUNNEL_ACTIVE, role.as_str(), session);
                close_sink(sink).await;
                return;
            }
            guard.insert(session.clone(), TunnelSlot::Active);
            drop(guard);
            pair_tunnel(slots, session, peer, role, sink, stream).await;
        }
        None => {
            let pending = guard.values().filter(|v| matches!(v, TunnelSlot::Pending(_))).count();
            if pending >= MAX_TUNNEL_PENDING {
                drop(guard);
                warn!("tunnel pending cap {} reached; rejecting {} for session {}", MAX_TUNNEL_PENDING, role.as_str(), session);
                close_sink(sink).await;
                return;
            }
            let (reclaim_tx, reclaim_rx) = oneshot::channel();
            guard.insert(session.clone(), TunnelSlot::Pending(Pending { role, reclaim: reclaim_tx }));
            drop(guard);
            info!("tunnel session {} parked ({})", session, role.as_str());
            tokio::spawn(park_tunnel(slots, session, role, sink, stream, reclaim_rx));
        }
    }
}

async fn park_tunnel(
    slots: TunnelSlots,
    session: String,
    role: TunnelRole,
    mut sink: WsSink,
    mut stream: WsStream,
    mut reclaim: oneshot::Receiver<oneshot::Sender<ParkedParts>>,
) {
    let mut buffered: Vec<Message> = Vec::new();
    let mut total = 0usize;
    let ttl = tokio::time::sleep(Duration::from_secs(config().tunnel_pending_ttl_secs));
    tokio::pin!(ttl);

    loop {
        tokio::select! {
            biased;
            ret = &mut reclaim => {
                if let Ok(return_tx) = ret {
                    let _ = return_tx.send(ParkedParts { sink, stream, buffered });
                }
                return;
            }
            _ = &mut ttl => {
                warn!("tunnel session {} pending TTL expired ({})", session, role.as_str());
                let _ = sink.send(Message::Close(None)).await;
                remove_pending(&slots, &session).await;
                return;
            }
            msg = stream.next() => {
                match msg {
                    Some(Ok(Message::Binary(data))) => {
                        total += data.len();
                        if total > MAX_TUNNEL_BUFFER {
                            warn!("tunnel session {} parked {} exceeded {}-byte buffer; closing", session, role.as_str(), MAX_TUNNEL_BUFFER);
                            let _ = sink.send(Message::Close(None)).await;
                            remove_pending(&slots, &session).await;
                            return;
                        }
                        buffered.push(Message::Binary(data));
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        info!("tunnel session {} parked {} closed before pairing", session, role.as_str());
                        remove_pending(&slots, &session).await;
                        return;
                    }
                    Some(Err(e)) => {
                        info!("tunnel session {} parked {} read error: {}", session, role.as_str(), e);
                        remove_pending(&slots, &session).await;
                        return;
                    }
                    Some(Ok(_)) => {}
                }
            }
        }
    }
}

async fn remove_pending(slots: &TunnelSlots, session: &str) {
    let mut guard = slots.lock().await;
    if matches!(guard.get(session), Some(TunnelSlot::Pending(_))) {
        guard.remove(session);
    }
}

async fn pair_tunnel(
    slots: TunnelSlots,
    session: String,
    peer: Pending,
    new_role: TunnelRole,
    mut new_sink: WsSink,
    new_stream: WsStream,
) {
    let peer_role = peer.role;

    let (ret_tx, ret_rx) = oneshot::channel();
    if peer.reclaim.send(ret_tx).is_err() {
        warn!("tunnel session {} peer vanished during pairing; rejecting {}", session, new_role.as_str());
        slots.lock().await.remove(&session);
        close_sink(new_sink).await;
        return;
    }
    let ParkedParts { sink: mut peer_sink, stream: peer_stream, buffered } = match ret_rx.await {
        Ok(parts) => parts,
        Err(_) => {
            warn!("tunnel session {} peer socket lost during pairing; rejecting {}", session, new_role.as_str());
            slots.lock().await.remove(&session);
            close_sink(new_sink).await;
            return;
        }
    };

    let _ = peer_sink.send(Message::Text("PAIRED".into())).await;
    let _ = new_sink.send(Message::Text("PAIRED".into())).await;

    for msg in buffered {
        if new_sink.send(msg).await.is_err() {
            warn!("tunnel session {} failed delivering buffered bytes; tearing down", session);
            slots.lock().await.remove(&session);
            let _ = peer_sink.send(Message::Close(None)).await;
            close_sink(new_sink).await;
            return;
        }
    }

    info!("tunnel session {} paired (parked {} <-> {})", session, peer_role.as_str(), new_role.as_str());

    // Bidirectional binary pump; the first direction to end closes the other.
    tokio::spawn(async move {
        let a = pump_copy(peer_stream, new_sink);
        let b = pump_copy(new_stream, peer_sink);
        tokio::pin!(a, b);
        let (dir, reason) = tokio::select! {
            r = &mut a => ("parked->arrived", r),
            r = &mut b => ("arrived->parked", r),
        };
        slots.lock().await.remove(&session);
        info!("tunnel session {} torn down (direction {}, reason {})", session, dir, reason);
    });
}

async fn pump_copy(mut rx: WsStream, mut tx: WsSink) -> &'static str {
    let idle = Duration::from_secs(config().tunnel_idle_secs);
    let reason = loop {
        match tokio::time::timeout(idle, rx.next()).await {
            Err(_) => break "idle timeout",
            Ok(Some(Ok(Message::Binary(data)))) => {
                if tx.send(Message::Binary(data)).await.is_err() {
                    break "peer send failed";
                }
            }
            Ok(Some(Ok(Message::Close(_)))) => break "close frame",
            Ok(Some(Ok(_))) => {}
            Ok(Some(Err(_))) => break "read error",
            Ok(None) => break "stream ended",
        }
    };
    let _ = tx.send(Message::Close(None)).await;
    reason
}

async fn close_sink(mut sink: WsSink) {
    let _ = sink.send(Message::Close(None)).await;
}
