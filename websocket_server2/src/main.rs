use axum::{
    extract::{
        ws::{Message, WebSocket},
        Extension, Query, WebSocketUpgrade,
    },
    response::IntoResponse,
    routing::get,
    serve, Router,
};
use database::{schema::ConnectedClient, DATABASE, Database};
use futures::{stream::SplitSink, SinkExt, StreamExt};
use std::net::SocketAddr;
use std::{collections::HashMap, sync::Arc};
use surrealdb::RecordId;
use tokio::sync::Mutex;
use tracing::info;
use chrono::{Utc, Duration};
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
    },
    Command {
        from: SessionID,
        ws_tx: Arc<Mutex<SplitSink<WebSocket, Message>>>,
        command: String,
        args: Vec<String>,
    },
}

#[derive(Clone, Debug, Default)]
struct Room {
    master: Option<Arc<Mutex<SplitSink<WebSocket, Message>>>>,
    client: Option<Arc<Mutex<SplitSink<WebSocket, Message>>>>,
}

#[derive(Clone)]
struct ChatServer {
    rooms: Arc<Mutex<HashMap<RoomID, Room>>>,
    session_map: Arc<Mutex<HashMap<SessionID, Arc<Mutex<SplitSink<WebSocket, Message>>>>>>,
    // Map session_id to username if authenticated
    user_map: Arc<Mutex<HashMap<SessionID, RecordId>>>,
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
                entry.master = Some(ws_tx.clone());
                info!("Master session {} assigned to room {}", session_id, room_id);
            }
            "client" => {
                entry.client = Some(ws_tx.clone());
                info!("Client session {} assigned to room {}", session_id, room_id);
            }
            _ => {
                let _ = ws_tx.lock().await.send(Message::Text("Invalid role".into())).await;
                return;
            }
        }

        // Update ConnectedClient in SurrealDB for this connection
        let now = Utc::now();
        let room_id_clone2 = room_id.clone();
        tokio::spawn(async move {
            let _ = DATABASE
                .query("UPDATE connected_client SET last_update = $now, connected = true WHERE connection_string == $connection_string")
                .bind(("now", now))
                .bind(("connection_string", room_id_clone2.clone()))
                .await;
        });

        self.session_map
            .lock()
            .await
            .insert(session_id.clone(), ws_tx.clone());
        // Remove any previous user association for this session
        self.user_map.lock().await.remove(&session_id);

        let server_clone = Arc::clone(&self);
        let session_id_clone = session_id.clone();
        let room_id_clone = room_id.clone();
        let role_clone = role.clone();
        let ws_tx_for_incoming = ws_tx.clone();

        // Handle incoming messages
        tokio::spawn(async move {
            let ws_tx = ws_tx_for_incoming;
            while let Some(Ok(message)) = ws_rx.next().await {
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
                            })
                            .await;
                    }
                    Message::Close(close_frame) => {
                        if let Some(frame) = close_frame {
                            info!("WebSocket closed: {:?} {:?}", frame.reason, frame.code);
                        }

                        server_clone
                            .cleanup_session(&room_id_clone, &session_id_clone, &role_clone)
                            .await?;
                        break;
                    }
                    Message::Ping(bytes) => info!("Ping: {:?}", bytes),
                    Message::Pong(bytes) => info!("Pong: {:?}", bytes),
                }
            }
            server_clone
                .cleanup_session(&room_id_clone, &session_id_clone, &role_clone)
                .await?;
            Ok::<(), anyhow::Error>(())
        });

        let server_clone = Arc::clone(&self);
        let role_clone = role.clone();
        let ws_tx_for_ping = ws_tx.clone();

        // Ping task to detect disconnection
        tokio::spawn(async move {
            let ws_tx = ws_tx_for_ping;
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                let mut sender = ws_tx.lock().await;
                if let Err(e) = sender.send(Message::Ping(vec![].into())).await {
                    info!("WebSocket {} disconnected: {:?}", session_id, e);
                    drop(sender);
                    server_clone
                        .cleanup_session(&room_id, &session_id, &role_clone)
                        .await?;
                    break;
                }
            }
            Ok::<(), anyhow::Error>(())
        });
    }

    async fn handle_message(&self, call: ChatMessage) {
        match call {
            ChatMessage::Send {
                from,
                room_id,
                text,
                bin,
            } => {
                let mut rooms = self.rooms.lock().await;
                if let Some(room) = rooms.get_mut(&room_id) {
                    let target_session = if self.is_session_match(room.master.as_ref(), &from).await {
                        room.client.clone()
                    } else if self.is_session_match(room.client.as_ref(), &from).await {
                        room.master.clone()
                    } else {
                        None
                    };

                    if let Some(session) = target_session {
                        let mut session = session.lock().await;
                        let send_result = if let Some(bin) = bin {
                            session.send(Message::Binary(bin.into())).await
                        } else {
                            session.send(Message::Text(text.into())).await
                        };
                        match send_result {
                            Ok(()) => info!("Message sent to target session in room {}", room_id),
                            Err(e) => {
                                log::error!("Failed to send message to session in room {}: {:?}", room_id, e);
                                // Assume the target is dead and clean it up
                                if self.is_session_match(room.master.as_ref(), &from).await {
                                    room.client = None;
                                    info!("Client removed from room {} due to send failure", room_id);
                                } else if self.is_session_match(room.client.as_ref(), &from).await {
                                    room.master = None;
                                    info!("Master removed from room {} due to send failure", room_id);
                                }
                            }
                        }
                    } else {
                        info!("No target session found in room {}", room_id);
                    }
                } else {
                    info!("Room {} not found", room_id);
                }
            }
            ChatMessage::Command { from, ws_tx, command, args } => {
                match command.as_str() {
                    "my_connections" => {
                        // Only allow if authenticated
                        let user_map = self.user_map.lock().await;
                        let user_id = user_map.get(&from);
                        drop(user_map);
                        if let Some(user_id) = user_id {
                            // Remove old connections (older than 2 days)
                            let two_days_ago = Utc::now() - Duration::days(2);
                            let _ = DATABASE
                                .query("UPDATE connected_client SET connected = false WHERE assigned_user == $auth.id && created_at <= $two_days_ago && connected == true")
                                .bind(("two_days_ago", two_days_ago))
                                .await;

                            // List active connections for this user
                            let query: Result<Vec<ConnectedClient>, _> = DATABASE
                                .query("SELECT * FROM connected_client WHERE assigned_user == $auth.id && connected == true ORDER BY last_update DESC LIMIT 10")
                                .await
                                .and_then(|r| r.take(0));
                            let mut sender = ws_tx.lock().await;
                            match query {
                                Ok(clients) => {
                                    if clients.is_empty() {
                                        let _ = sender.send(Message::Text("No active connections found.".into())).await;
                                    } else {
                                        let mut msg = String::from("Your active connections (last 10):\n");
                                        for c in clients {
                                            msg.push_str(&format!("Room: {} | Last Update: {:?}\n", c.connection_string, c.last_update));
                                        }
                                        let _ = sender.send(Message::Text(msg.into())).await;
                                    }
                                }
                                Err(e) => {
                                    let _ = sender.send(Message::Text(format!("Failed to fetch connections: {e}").into())).await;
                                }
                            }
                        } else {
                            let mut sender = ws_tx.lock().await;
                            let _ = sender.send(Message::Text("You must authenticate first with /auth <username> <password>".into())).await;
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
                                    self.user_map.lock().await.insert(from.clone(), db.user.unwrap_or_default().get_id());
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

    async fn cleanup_session(&self, room_id: &RoomID, session_id: &SessionID, role: &str) -> anyhow::Result<(), anyhow::Error> {
        let mut rooms = self.rooms.lock().await;
        let mut session_map = self.session_map.lock().await;
    
        if session_map.remove(session_id).is_none() {
            return Ok(()); // Already cleaned up
        }
    
        // let client: Option<ConnectedClient> = DATABASE
        //     .query("UPDATE connected_client SET connected = false WHERE connection_string == $connection_id")
        //     .bind(("connection_id", room_id.clone()))
        //     .await?
        //     .take(0)?;

        log::info!("Client disconnected");

        if let Some(room) = rooms.get_mut(room_id) {
            match role {
                "master" if room.master.is_some() => {
                    room.master = None;
                    info!("Master session {} removed from room {}", session_id, room_id);
                }
                "client" if room.client.is_some() => {
                    room.client = None;
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
    // let init = initialize_db().await;
    // if init.is_ok() {    
    //     let _ = DATABASE
    //         .signin(Database {
    //             namespace: NS,
    //             database: DB,
    //             username: "shadowbroker",
    //             password: "toor10!9",
    //         })
    //         .await.unwrap();
    // } else if let Err(e) = init {
    //     println!("ERR {e:?}");
    // }

    let chat_server = ChatServer {
        rooms: Arc::new(Mutex::new(HashMap::new())),
        session_map: Arc::new(Mutex::new(HashMap::new())),
        user_map: Arc::new(Mutex::new(HashMap::new())),
    };

    let app = Router::new()
        .route("/websocket", get(websocket_handler))
        .layer(Extension(Arc::new(chat_server)));

    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], 8081))).await?;
    // info!("Listening on {}", address);
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
    let role = params.get("role").cloned().unwrap_or_else(|| "client".to_string());

    info!("Client connected. Role: {:?}, Room: {:?}, Session: {:?}", role, room_id, session_id);
    // let res = connect_client(room_id.clone()).await;
    // println!("Res: {res:?}");
    ws.on_upgrade(move |socket| chat_server.handle_ws(socket, session_id, room_id, role))
}

pub async fn connect_client(room_id: String) -> anyhow::Result<(), anyhow::Error> {
    let potential_client: Option<ConnectedClient> = DATABASE
        .query("SELECT * FROM connected_client WHERE connection_string == $room_id")
        .bind(("room_id", room_id.clone()))
        .await?
        .take(0)?;

    if let Some(client) = potential_client {
        if client.connected == false {
            let _: Option<ConnectedClient> = DATABASE
                .query("UPDATE connected_client SET connected = true WHERE connection_string == $room_id")
                .bind(("room_id", room_id.clone()))
                .await?
                .take(0)?;
        }
    }

    Ok(())
}