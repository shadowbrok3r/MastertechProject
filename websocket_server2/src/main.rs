use axum::{
    extract::{
        ws::{Message, WebSocket},
        Extension, Query, WebSocketUpgrade,
    },
    response::IntoResponse,
    routing::get,
    serve, Router,
};
use futures::{stream::SplitSink, SinkExt, StreamExt};
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

#[derive(Clone, Debug, Default)]
struct Room {
    master: Option<Arc<Mutex<SplitSink<WebSocket, Message>>>>, // Store write half
    client: Option<Arc<Mutex<SplitSink<WebSocket, Message>>>>, // Store write half
}


#[derive(Clone)]
struct ChatServer {
    rooms: Arc<Mutex<HashMap<RoomID, Room>>>, // Use Arc<Mutex<...>> to match type expectations
    session_map: Arc<Mutex<HashMap<SessionID, Arc<Mutex<SplitSink<WebSocket, Message>>>>>>, // Map to track sessions by their IDs
}

impl ChatServer {
    async fn handle_ws(
        self: Arc<Self>,
        ws: WebSocket,
        session_id: SessionID,
        room_id: RoomID,
        role: String,
    ) {
        let (ws_tx, mut ws_rx) = ws.split(); // Split the WebSocket into separate read/write handles
        let ws_tx = Arc::new(Mutex::new(ws_tx)); // Arc<Mutex<WebSocket WriteHalf>> for shared sending
    
        info!("Registering session for role: {role}");
    
        // let mut x: i32 = 1; // `x` is a mutable i32
        // let b;
        // {
        //     let a = &mut x;  // `a` is a mutable reference to `x`
        //     b = &mut *a;     // `b` is another mutable reference to `x`
        // }
        // *b = 0;             // Modifying `x` via `b`
        
        // x = 1;              // Modifying `x` directly

        // info!("{x}");


        // let mut x = 1;
        // let b;
        // {
        //     let a = &mut x;
        //     b = &mut *a;
        // } // `a` goes out of scope
        // *b = 0; // ERROR: `b` is invalid now

        /*
            The original reference (a) is no longer used after the reborrow.
            The reborrowed reference (b) follows the same lifetime rules.
         */

        // let mut x: i32 = 1;
        // let b;
        // let a = &mut x; // `a` is a mutable reference to `x`
        // b = &mut *a;    // `b` reborrows `x` mutably

        // *a = 2; // ERROR: Cannot use `a` while `b` exists
        // *b = 0; // if `b` is used here, `a` was still alive

        // x = 1; // Now reassigning `x`, but we had multiple mutable borrows


        // This should work because a is FULLY DROPPED, before b is created
        // let mut x: i32 = 1;
        // let b;
        // // {
        //     let a = &mut x;
        //     // drop(a);
        //     *a = 1;
        // // } // `a` is dropped
        // b = &mut x; // This is fine now
        
        // *b = 0;
        



        // Register the session with its ID
        self.session_map
            .lock()
            .await
            .insert(session_id.clone(), ws_tx.clone());
    
        let mut rooms = self.rooms.lock().await;
        let entry = rooms.entry(room_id.clone()).or_insert_with(Room::default);
    
        match role.as_str() {
            "master" => entry.master = Some(ws_tx.clone()),
            "client" => entry.client = Some(ws_tx.clone()),
            _ => {}
        };
    
        info!("Updated room: {:?}", entry);
        info!("Processing messages");
    
        // Clone `Arc<Self>` and `session_id` for the read loop
        let server_clone = Arc::clone(&self);
        let session_id_clone = session_id.clone();
        let room_id_clone = room_id.clone();
    
        // Spawn task to handle incoming messages
        tokio::spawn(async move {
            while let Some(Ok(message)) = ws_rx.next().await {
                match message {
                    Message::Text(text) => {
                        info!("Received text message: {}", text);
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
                        info!("Received binary message: {:?}", String::from_utf8(bin.to_vec()).unwrap_or_default());
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
                        break;
                    }
                    _ => {}
                }
            }
    
            server_clone
                .cleanup_session(&room_id_clone, &session_id_clone, &role)
                .await;
        });
    }
    
    async fn handle_message(&self, call: ChatMessage) {
        info!("Handling Message");
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
                    let target_session = match (room.master.as_ref(), room.client.as_ref()) {
                        (Some(master), Some(client)) => {
                            if self.is_session_match(master, &from).await {
                                Some(client.clone())
                            } else {
                                Some(master.clone())
                            }
                        }
                        (Some(master), None) => {
                            if self.is_session_match(master, &from).await {
                                None
                            } else {
                                Some(master.clone())
                            }
                        }
                        (None, Some(client)) => {
                            if self.is_session_match(client, &from).await {
                                None
                            } else {
                                Some(client.clone())
                            }
                        }
                        (None, None) => None,
                    };
    
                    if let Some(session) = target_session {
                        info!("Relaying message to target session in room {}", room_id);
    
                        let ws_clone = session.clone();
                        tokio::spawn(async move {
                            let mut session = ws_clone.lock().await;
                            let send_result = if let Some(bin) = bin {
                                session.send(Message::Binary(bin.into())).await
                            } else {
                                session.send(Message::Text(text.into())).await
                            };
    
                            if let Err(e) = send_result {
                                info!("Failed to send message: {:?}", e);
                            } else {
                                info!("Message successfully sent to target session");
                            }
                        });
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
        session: &Arc<Mutex<SplitSink<WebSocket, Message>>>, // Expect write half
        session_id: &SessionID,
    ) -> bool {
        let session_map = self.session_map.lock().await;
    
        // Get stored session
        if let Some(stored_session) = session_map.get(session_id) {
            // Compare if both are pointing to the same underlying write stream
            Arc::ptr_eq(stored_session, session)
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
        get(websocket_handler),
    ).layer(Extension(Arc::new(chat_server)));

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

    info!("Client connected.\nRole: {:?}\nRoom: {:?}\nSession: {:?}", role, room_id, session_id);
    ws.on_upgrade(move |socket| {
        chat_server
            .clone()
            .handle_ws(socket, session_id, room_id, role)
    })
}

