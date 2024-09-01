use axum::{
    extract::{
        ws::{Message, WebSocket},
        Extension, Query, WebSocketUpgrade,
    },
    handler::Handler,
    response::IntoResponse,
    routing::get,
    serve, Router,
};
use futures::StreamExt;
use std::net::SocketAddr;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;
use tracing::info;
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
}

#[derive(Clone)]
struct Room {
    master: Option<Arc<Mutex<WebSocket>>>,
    client: Option<Arc<Mutex<WebSocket>>>,
}

#[derive(Clone)]
struct ChatServer {
    rooms: Arc<Mutex<HashMap<RoomID, Room>>>, // Use Arc<Mutex<...>> to match type expectations
    session_map: Arc<Mutex<HashMap<SessionID, Arc<Mutex<WebSocket>>>>>, // Map to track sessions by their IDs
}

impl ChatServer {
    async fn handle_ws(
        self: Arc<Self>, // Changed to Arc<Self> to handle ownership and cloning
        ws: WebSocket,
        session_id: SessionID,
        room_id: RoomID,
        role: String,
    ) {
        let ws = Arc::new(Mutex::new(ws)); // Wrap WebSocket in Arc<Mutex<>> to share ownership

        // Register the session with its ID
        self.session_map
            .lock()
            .await
            .insert(session_id.clone(), ws.clone());

        // Process messages
        while let Some(Ok(message)) = ws.lock().await.next().await {
            match message {
                Message::Text(text) => {
                    info!("Received text message: {}", text);
                    self.handle_message(ChatMessage::Send {
                        from: session_id.clone(),
                        room_id: room_id.clone(),
                        text,
                        bin: None,
                    })
                    .await;
                }
                Message::Binary(bin) => {
                    info!("Received binary message");
                    self.handle_message(ChatMessage::Send {
                        from: session_id.clone(),
                        room_id: room_id.clone(),
                        text: String::new(),
                        bin: Some(bin),
                    })
                    .await;
                }
                Message::Close(_) => {
                    info!("WebSocket closed");
                    break;
                }
                _ => {}
            }
        }

        // Cleanup session when disconnected
        self.cleanup_session(&room_id, &session_id, &role).await;
    }

    async fn handle_message(&self, call: ChatMessage) {
        match call {
            ChatMessage::Send {
                from,
                room_id,
                text,
                bin,
            } => {
                info!(
                    "Handling message from session {} in room {}: {}",
                    from, room_id, text
                );

                let rooms = self.rooms.lock().await;
                if let Some(room) = rooms.get(&room_id) {
                    let target_session = async {
                        if let Some(master) = &room.master {
                            if self.is_session_match(master, &from).await {
                                return room.client.as_ref();
                            }
                        }
                        if let Some(client) = &room.client {
                            if self.is_session_match(client, &from).await {
                                return room.master.as_ref();
                            }
                        }
                        None
                    }
                    .await;

                    if let Some(session) = target_session {
                        let mut session = session.lock().await;
                        if let Some(bin) = bin {
                            info!("Relaying binary message");
                            let _ = session.send(Message::Binary(bin)).await;
                        } else {
                            info!("Relaying text message");
                            let _ = session.send(Message::Text(text)).await;
                        }
                    } else {
                        info!(
                            "No target session found for session {} in room {}",
                            from, room_id
                        );
                    }
                } else {
                    info!("Room {} not found", room_id);
                }
            }
        }
    }

    async fn cleanup_session(&self, room_id: &RoomID, session_id: &SessionID, role: &str) {
        let mut rooms = self.rooms.lock().await;
        let mut session_map = self.session_map.lock().await;

        // Remove the session from the session map
        session_map.remove(session_id);

        if let Some(room) = rooms.get_mut(room_id) {
            match role {
                "master" => {
                    if let Some(master) = &room.master {
                        if self.is_session_match(master, session_id).await {
                            room.master = None;
                            info!(
                                "Master session {} removed from room {}",
                                session_id, room_id
                            );
                        }
                    }
                }
                "client" => {
                    if let Some(client) = &room.client {
                        if self.is_session_match(client, session_id).await {
                            room.client = None;
                            info!(
                                "Client session {} removed from room {}",
                                session_id, room_id
                            );
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // Complete is_session_match function
    async fn is_session_match(
        &self,
        session: &Arc<Mutex<WebSocket>>,
        session_id: &SessionID,
    ) -> bool {
        let session_map = self.session_map.lock().await;

        // Check if the session ID maps to the given WebSocket session
        if let Some(stored_session) = session_map.get(session_id) {
            Arc::ptr_eq(stored_session, session) // Compares if both Arcs point to the same WebSocket
        } else {
            false
        }
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let chat_server = ChatServer {
        rooms: Mutex::new(HashMap::new()).into(),
        session_map: Arc::new(Mutex::new(HashMap::new())), // Initialize the session map
    };

    let app = Router::new().route(
        "/websocket",
        get(websocket_handler.layer(Extension(chat_server))),
    );

    let address = SocketAddr::from(([0, 0, 0, 0], 8081));
    let listener = tokio::net::TcpListener::bind(address).await.unwrap();
    info!("Listening on {}", address);

    serve(listener, app).await.unwrap();
}

async fn websocket_handler(
    ws: WebSocketUpgrade,
    Query(params): Query<HashMap<String, String>>,
    Extension(chat_server): Extension<Arc<ChatServer>>, // Ensure chat_server is Arc<ChatServer>
) -> impl IntoResponse {
    let session_id = Uuid::new_v4().to_string();
    let room_id = params.get("room_id").cloned().unwrap_or_default();
    let role = params
        .get("role")
        .cloned()
        .unwrap_or_else(|| "client".to_string());

    info!("Client connected: {:?}-{:?}", role, room_id);
    ws.on_upgrade(move |socket| {
        chat_server
            .clone()
            .handle_ws(socket, session_id, room_id, role)
    })
}

