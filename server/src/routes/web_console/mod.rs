use serde::{Deserialize, Serialize};
use tracing::debug;
use socketioxide::extract::{Data, SocketRef};
use state::{ClientMessage, Command, CommandMessage, ManagerMessage, MessageStore, SystemInformation};

pub mod state;
// pub mod web_console;

#[derive(Serialize)]
struct ClientMessages {
    client_messages: Vec<ClientMessage>,
}

#[derive(Serialize, Deserialize, Debug)]
struct ClientMessageIn {
    room: String,
    sysinfo: SystemInformation,
    client_uuid: String
}

#[derive(Serialize, Deserialize, Debug)]
struct ManagerMessageIn {
    room: String,
    machine_id: String,
    command: Command,
}

#[derive(Serialize, Deserialize, Debug)]
struct Join{
    room: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct ClientResponse{
    message: String,
}

pub async fn on_connect(socket: SocketRef) {
    debug!("socket connected: {}, ns: {}", socket.id, socket.ns());
    
    socket.on(
        "join",
        |socket: SocketRef, 
        Data::<Join>(data), 
        // State(Sessions(_session_state)),
        // store: State<MessageStore>
    | async move {
            debug!("Received join: {:?}", data.room);
            // let _ = socket.leave_all();
            let _ = socket.join(data.room.clone());
            // let client_messages = store.get(&data.room).await;
            // let _ = socket.emit("clientMessages", ClientMessages { client_messages });
        },
    );

    socket.on(
        "clientSysInfo",
        |socket: SocketRef, 
        Data::<ClientMessageIn>(data), 
        // store: State<MessageStore>,
        // State(Sessions(_session_state))
    | async move {
            debug!("Received message: {:?}", data);
            // This is emitted to everyone in the same room
            let response = ClientMessage {
                date: Some(chrono::Utc::now()),
                machine_id: format!("{}-{}", data.sysinfo.hostname, data.client_uuid), // socket.id
                room: data.room.clone(),
                sysinfo: data.sysinfo,
            };
            
            debug!("Received message: {:?}", response.clone());

            // store.insert(&data.room, response.clone()).await;

            let _ = socket.within(data.room).emit("clientSysInfo", response);
        },
    );

    socket.on(
        "managerMessage",
        |socket: SocketRef, 
        Data::<ManagerMessageIn>(data), 
        // _store: State<MessageStore>
    | async move {
            debug!("Received message: {:?}", data);
            // This is emitted to everyone in the same room
            let response = ManagerMessage {
                machine_id: data.machine_id,
                command: data.command,
                room: data.room.clone(),
            };

        println!("Received message: {}", data.room.clone());
            // store.insert(&data.room, response.clone()).await;

            let _ = socket.within(data.room).emit("managerMessage", response);
        },
    );

    socket.on(
        "command",
        |socket: SocketRef, 
        Data::<CommandMessage>(data), 
        // _store: State<MessageStore>
    | async move {
                debug!("Received message: {:?}", data);
                // This is emitted to everyone in the same room
                let response = CommandMessage {
                    command: data.command,
                    room: data.room.clone(),
                };
    
            // println!("Received message: {}", data.room.clone());
            // store.insert(&"RIV".to_string(), response.clone()).await;

            let _ = socket.within(data.room).emit("command", response.command);
        },
    );

    socket.on(
        "clientCmdResponse",
        |socket: SocketRef, 
        Data::<ClientResponse>(data), 
        // _store: State<MessageStore>
    | async move { // Vec<u8>
            // store.insert(&"RIV".to_string(), response.clone()).await;
            // let new_data = String::from_utf8(data).unwrap();
            debug!("Received clientCmdResponse: {:?}", data.message);

            let _ = socket
                .within("RIV")
                .emit(
                  "clientCmdResponse", 
                  data.message
                );
        },
    );

    socket.on(
        "message",
        |socket: SocketRef, 
        Data::<String>(data), 
        // _store: State<MessageStore>
    | async move {
                debug!("Received message: {:?}", data);
            // store.insert(&"RIV".to_string(), response.clone()).await;

            let _ = socket.broadcast().emit("message", format!("Server received message => {}", data));
        },
    );

    socket.on(
        "error",
        |_socket: SocketRef, 
        Data::<String>(data), 
        // _store: State<MessageStore>
    | async move {
            debug!("Received error: {:?}", data);
        },
    );
}

/* pub async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| websocket(socket, state))
}

/**
 * This function deals with a single websocket connection, i.e., a single
 * connected client / user, for which we will spawn two independent tasks (for
 * receiving / sending chat messages). 
*/
async fn websocket(stream: WebSocket, state: Arc<AppState>) {
    // By splitting, we can send and receive at the same time.
    let (mut sender, mut receiver) = stream.split();

    // We have more state now that needs to be pulled out of the connect loop
    let mut tx = None::<broadcast::Sender<String>>;
    let mut hostname: String = String::new(); // username -> hostname
    let mut identifier: String = String::new(); // channel -> identifier
    let mut role: Role = Role::Client;
    // Loop until a text message is found.
    while let Some(Ok(message)) = receiver.next().await {
        match message{
            Message::Text(text) => {
                debug!("Received a Text message: {text:#?}");

                let ws_payload: WsPayload = match serde_json::from_str(text.as_str()){
                    Ok(ws_payload) => {
                        ws_payload
                    },
                    Err(error) => {
                        tracing::error!(%error);
                        let _ = sender
                            .send(Message::Text(String::from(
                                "Failed to parse ws_payload",
                            )))
                            .await;
                        break;
                    }
                };

                // Scope to drop the mutex guard before the next await
                {
                    // If username that is sent by client is not taken, fill username string.
                    let mut clients = state.client_map.lock().unwrap();

                    // This is basically the "chat room" so other clients cant control this computer
                    identifier = ws_payload.identifier.unwrap().to_string().clone();


                    role = ws_payload.role;

                    // identifier -> channel
                    let client = clients
                        .entry(ws_payload.identifier.unwrap().to_string())
                        .or_insert_with(ChannelState::new);

                    // This becomes none if 
                    tx = Some(client.tx.clone());

                    // if our hashmap does NOT contain this requests UUID, then insert it 
                    if !client.machine.contains(&ws_payload.identifier.unwrap().to_string()) {
                        client.machine.insert(ws_payload.identifier.unwrap().to_string());
                        hostname = ws_payload.hostname.clone();
                    }
                }

                // If not empty we want to quit the loop else we want to quit function.
                if tx.is_some() && !hostname.is_empty() {
                    debug!("Tx is some and hostname is not empty");
                    break;
                } else {
                    // Only send our client that username is taken.
                    let _ = sender
                        .send(Message::Text(String::from("Username already taken.")))
                        .await;

                    return;
                }  
            },
            Message::Binary(binary) => {
                debug!("Received a Binary message: {binary:#?}");
            },
            Message::Ping(ping) => {
                debug!("Received a Ping message: {ping:#?}");
            },
            Message::Pong(pong) => {
                debug!("Received a Pong message: {pong:#?}");
            },
            Message::Close(close) => {
                debug!("Received a Close message: {close:#?}");
            },
        }
    }

    // We know if the loop exited `tx` is not `None`.
    let tx = tx.unwrap();
    // We subscribe *before* sending the "joined" message, so that we will also
    // display it to our client.
    let mut rx = tx.subscribe();

    // Now send the "joined" message to all subscribers.
    let msg = format!("{hostname} joined.");
    debug!("{msg}");
    let _ = tx.send(msg);

    // Spawn the first task that will receive broadcast messages and send text
    // messages over the websocket to our client.
    let mut send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            debug!("New message: {msg:#?}");
            // In any websocket error, break loop.
            if sender.send(Message::Text(msg)).await.is_err() {
                break;
            }
        }
    });

    // We need to access the `tx` variable directly again, so we can't shadow it here.
    // I moved the task spawning into a new block so the original `tx` is still visible later.
    let mut recv_task = {
        // Clone things we want to pass to the receiving task.
        let tx = tx.clone();
        let name = hostname.clone();
        let id = identifier.clone();
        // This task will receive messages from client and send them to broadcast subscribers.
        tokio::spawn(async move {
            while let Some(Ok(Message::Text(text))) = receiver.next().await {
                // Add username before message.
                let _ = tx.send(format!("{name}-{id}: {text}"));
            }
        })
    };

    // If any one of the tasks exit, abort the other.
    tokio::select! {
        _ = (&mut send_task) => recv_task.abort(),
        _ = (&mut recv_task) => send_task.abort(),
    };

    // Send user left message.
    let msg = format!("{hostname} left.");
    debug!("{}", msg);
    let _ = tx.send(msg);
    let mut clients = state.client_map.lock().unwrap();

    // Remove username from map so new clients can take it.
    clients.get_mut(&identifier).unwrap().machine.remove(&identifier);
    // TODO: Check if the room is empty now and remove the `ChannelState` from the map.
} */

