use async_trait::async_trait; // Allows us to use async functions in traits
use axum::extract::Extension; // Extracts data from requests, here for extracting shared state
use axum::extract::Query; // Extracts query parameters from requests
use axum::response::IntoResponse; // Converts types into HTTP responses
use axum::routing::get; // Specifies the HTTP GET method for routing
use axum::Router; // Manages route definitions and handlers
use ezsockets::axum::Upgrade; // Handles WebSocket upgrades
use ezsockets::CloseFrame; // Represents WebSocket close frames
use ezsockets::Error; // Represents WebSocket errors
use ezsockets::Server;
use uuid::Uuid; // Manages WebSocket server instances
use std::collections::HashMap; use std::fs::File;
// Provides a hash map data structure
use std::io::BufRead; // Trait for reading lines from standard input
use std::net::SocketAddr; // Represents socket addresses
use tracing::info; // For logging information
use simplelog::{WriteLogger, Config, LevelFilter};

type SessionID = String;
type RoomID = String;
type Session = ezsockets::Session<SessionID, ()>;

// Enum to represent different chat messages
#[derive(Debug)]
enum ChatMessage {
    Send { from: SessionID, room_id: RoomID, text: String , bin: Option<Vec<u8>>}
}

// Struct to represent a room
struct Room {
    master: Option<Session>,
    client: Option<Session>,
}

// Struct to represent the chat server
struct ChatServer {
    rooms: HashMap<RoomID, Room>, // Stores rooms with their sessions
    handle: Server<Self>, // Reference to the WebSocket server
}

// Implement the WebSocket server extension for the chat server
#[async_trait]
impl ezsockets::ServerExt for ChatServer {
    type Session = ChatSession;
    type Call = ChatMessage;

    // Handles new WebSocket connections
    async fn on_connect(
        &mut self,
        socket: ezsockets::Socket,
        request: ezsockets::Request,
        _address: SocketAddr,
    ) -> Result<Session, Option<CloseFrame>> {
        info!("New connection request received");

        // Extract room_id and role from query parameters
        let query = request.uri().query().unwrap_or("");
        let params: HashMap<_, _> = url::form_urlencoded::parse(query.as_bytes()).collect();
        let room_id: RoomID = params.get("room_id").and_then(|v| v.parse().ok()).unwrap_or(String::new());
        let role = params.get("role").map(|v| v.as_ref()).unwrap_or("client");

        info!("Room ID: {}, Role: {}", room_id, role);
        
        let id = Uuid::new_v4().to_string();

        info!("Assigned session ID: {}", id);

        // Borrow rooms mutably to insert the new session
        let room = self.rooms.entry(room_id.clone()).or_insert_with(|| Room {
            master: None,
            client: None,
        });

        let id = id.clone();
        let id2 = id.clone();
        // Create a new session
        let session = Session::create(
            |_| ChatSession {
                id,
                room_id: room_id.clone(),
                role: role.to_string(),
                server: self.handle.clone(),
            },
            id2,
            socket,
        );

        // Assign the session to the appropriate role in the room
        match role {
            "master" => {
                room.master = Some(session.clone());
                info!("Master session assigned to room {}", room_id.clone());
            },
            "client" => {
                room.client = Some(session.clone());
                info!("Client session assigned to room {}", room_id);
            },
            _ => {
                info!("Invalid role: {}", role);
                return Err(Some(CloseFrame {
                    code: ezsockets::CloseCode::Bad(1002), // Protocol error
                    reason: "Invalid role".into(),
                }));
            },
        }

        Ok(session) // Return the session
    }

    // Handles WebSocket disconnections
    async fn on_disconnect(
        &mut self,
        id: <Self::Session as ezsockets::SessionExt>::ID,
        _reason: Result<Option<CloseFrame>, Error>,
    ) -> Result<(), Error> {
        info!("Session {} disconnected", id);

        // Remove the session from its room
        for room in self.rooms.values_mut() {
            if let Some(session) = &room.master {
                if session.id == id {
                    room.master = None;
                    info!("Master session {} removed from room", id);
                }
            }
            if let Some(session) = &room.client {
                if session.id == id {
                    room.client = None;
                    info!("Client session {} removed from room", id);
                }
            }
        }
        Ok(())
    }

    // Handles incoming chat messages
    async fn on_call(&mut self, call: Self::Call) -> Result<(), Error> {
        match call {
            ChatMessage::Send { text, from, room_id, bin } => {
                info!("Message received from session {} in room {}: {}", from, room_id, text);
                
                if from == "ffffffff-ffff-ffff-ffff-ffffffffffff" {
                    // Broadcast to all rooms
                    for (room_id, room) in self.rooms.iter() {
                        if let Some(client) = &room.client {
                            if let Some(bin) = bin.clone() {
                                info!("Broadcasting binary message to client session {} in room {}", client.id, room_id);
                                client.binary(bin).unwrap();
                            } else {
                                info!("Broadcasting text message to client session {} in room {}", client.id, room_id);
                                client.text(format!("from {}: {}", from, text)).unwrap();
                            }
                        }
                        if let Some(master) = &room.master {
                            if let Some(bin) = bin.clone() {
                                info!("Broadcasting binary message to master session {} in room {}", master.id, room_id);
                                master.binary(bin).unwrap();
                            } else {
                                info!("Broadcasting text message to master session {} in room {}", master.id, room_id);
                                master.text(format!("from {}: {}", from, text)).unwrap();
                            }
                        }
                    }
                } else if let Some(room) = self.rooms.get(&room_id) {
                    let mut role = String::new();
                    // Determine the target session based on the sender's role
                    let target_session = if room.master.as_ref().map_or(false, |s| s.id == from) {
                        role = "Master".to_string();
                        room.client.as_ref()
                    } else if room.client.as_ref().map_or(false, |s| s.id == from) {
                        role = "Client".to_string();
                        room.master.as_ref()
                    } else {
                        None
                    };
            
                    // Relay the message to the target session
                    if let Some(session) = target_session {
                        if let Some(bin) = bin {
                            info!("Relaying binary message to session {}: {:?}", session.id, bin);
                            session.binary(bin).unwrap();
                        } else {
                            info!("Relaying text message to session {}: {}", session.id, text);
                            session.text(format!("{text}")).unwrap(); // "{role}-{}: {}", room_id, text
                        }
                    } else {
                        info!("No target session found for session {} in room {}", from, room_id);
                    }
                } else {
                    info!("Room {} not found", room_id);
                }
            }
        };        
             
        Ok(())
    }
}

// Struct to represent individual chat sessions
struct ChatSession {
    id: SessionID, // Unique session ID
    room_id: RoomID, // Room ID the session belongs to
    role: String, // Role of the session: "master" or "client"
    server: Server<ChatServer>, // Reference to the chat server
}

// Implement the WebSocket session extension for chat sessions
#[async_trait]
impl ezsockets::SessionExt for ChatSession {
    type ID = SessionID; // Defines the session ID type
    type Call = (); // Defines the message type

    // Returns the session ID
    fn id(&self) -> &Self::ID {
        &self.id
    }

    // Handles incoming text messages
    async fn on_text(&mut self, text: String) -> Result<(), Error> {
        info!("Session {} (role: {}) in room {} received message: {}", self.id, self.role, self.room_id, text);
        // self.server
        self.server
            .call(ChatMessage::Send {
                from: self.id.clone(),
                room_id: self.room_id.clone(),
                text,
                bin: None
            })
            .unwrap();
        Ok(())
    }

    // Handles incoming binary messages (not implemented)
    async fn on_binary(&mut self, bytes: Vec<u8>) -> Result<(), Error> {
        self.server
            .call(ChatMessage::Send {
                from: self.id.clone(),
                room_id: self.room_id.clone(),
                text: "Binary".to_string(),
                bin: Some(bytes)
            })
        .unwrap();
        Ok(())
    }

    // Handles internal calls (no-op here)
    async fn on_call(&mut self, call: Self::Call) -> Result<(), Error> {
        info!("call: {call:#?}");
        let () = call;
        Ok(())
    }
}

#[tokio::main]
async fn main() {
    // Configure log level and log file
    let log_level = LevelFilter::Debug; 
    let log_file = File::create("output.log").unwrap();

    // Init the logger
    WriteLogger::init( 
        log_level,
        Config::default(),
        log_file
    ).unwrap();
    
    println!("Starting the WebSocket server");

    // Create the WebSocket server
    let (server, _) = Server::create(|handle| ChatServer {
        rooms: HashMap::new(),
        handle,
    });

    // Define the HTTP router and WebSocket endpoint
    let app = Router::new()
        .route("/websocket", get(websocket_handler))
        .layer(Extension(server.clone()));

    // Define the server address
    let address = SocketAddr::from(([127, 0, 0, 1], 8081));

    // Spawn a new async task to run the server
    tokio::spawn(async move {
        println!("Listening on {}", address);
        axum::Server::bind(&address)
            .serve(app.into_make_service_with_connect_info::<SocketAddr>())
            .await
            .unwrap();
    });

    // Read lines from standard input and broadcast them to all rooms
    // let stdin = std::io::stdin();
    // let lines = stdin.lock().lines();
    // for line in lines {
    //     let line = line.unwrap();
    //     server
    //         .call(ChatMessage::Send {
    //             text: line,
    //             from: Uuid::max().to_string(), // Reserve some ID for the server
    //             room_id: 0.to_string(), // Broadcast to all rooms (can be customized)
    //             bin: None
    //         })
    //         .unwrap();
    // }
}

// Handles WebSocket upgrade requests
async fn websocket_handler(
    Extension(server): Extension<Server<ChatServer>>,
    Query(query): Query<HashMap<String, String>>,
    ezsocket: Upgrade,
) -> impl IntoResponse {
    let kick_me = query.get("kick_me");
    let kick_me = kick_me.map(|s| s.as_str());
    if matches!(kick_me, Some("Yes")) {
        info!("Connection rejected due to 'kick_me' query parameter");
        return (
            axum::http::StatusCode::BAD_REQUEST,
            "we won't accept you because of kick_me query parameter",
        )
            .into_response();
    }
    info!("Upgrading to WebSocket connection");
    ezsocket.on_upgrade(server)
}