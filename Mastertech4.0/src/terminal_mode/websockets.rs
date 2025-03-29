use database::{schema::{utilities::{check_id_existence, query_id}, ConnectedClient, CONNECTED_CLIENT_TABLE}, DATABASE, WS_CLIENT_URL};
use displays::{remote_viewer::{encode_buffer_with_timestamp, ratagui::TerminalEvent}, tabs::admin_console::client_interface::client_handler::ClientHandler, Cmd};
use crate::filesystem::get_client_hash;
use ewebsock::{WsEvent, WsMessage};
use ratatui::{buffer::Buffer, Frame};
use std::time::{Duration, Instant};
use tokio;

use super::{data::LocalTermEvent, TerminalApp};

// Something to store the handle to our pty session:
struct MyPtySession {
    pty: pty_process::Pty,
    child: tokio::process::Child,
}

impl<'a> TerminalApp<'a> {
    pub async fn start_websocket_sender(
        mut buffer_rx: tokio::sync::mpsc::UnboundedReceiver<(usize, Buffer)>, // Changed: Receive frame count
        start_tx: tokio::sync::mpsc::UnboundedSender<bool>,
        connection_state_tx: tokio::sync::mpsc::UnboundedSender<(bool, String)>,
        event_tx: tokio::sync::mpsc::UnboundedSender<LocalTermEvent>,
        mut shutdown_rx: tokio::sync::broadcast::Receiver<()>,
        // manual_start: &mut bool,
    ) -> anyhow::Result<()> {
        let client = get_client_hash();
        let mut maybe_pty: Option<MyPtySession> = None;
        let connection_url = format!(
            "{WS_CLIENT_URL}&room_id={}",
            client.connection_string
        );

        let connection = ewebsock::connect(
            connection_url,
            ewebsock::Options::default(),
        );

        
        match connection {
            Ok((mut sender, receiver)) => {
                let ready = &mut false;
                log::info!("start_websocket_sender -> ready");
                loop {
                    // Handle WebSocket events (e.g., READY or TerminalEvent from egui)
                    while let Some(event) = receiver.try_recv() {
                        log::info!("Received WebSocket event: {:?}", event);
                        // update client to connected = false in db
                        match event {
                            WsEvent::Opened => { 
                                log::info!("start_websocket_sender -> Connection Opened");
                                let _ = connection_state_tx.send((true, "Connected".to_string())); 
                            },
                            WsEvent::Error(e) => { 
                                log::info!("start_websocket_sender -> Error: {e:?}");
                                let _ = connection_state_tx.send((false, format!("{e:?}"))); 
                            },
                            WsEvent::Closed => { 
                                log::info!("start_websocket_sender -> Connection Closed");
                                let _ = connection_state_tx.send((false, "Disconnected".to_string())); 
                            },
                            WsEvent::Message(ws_message) => {
                                match ws_message {
                                    WsMessage::Pong(_) => { let _ = connection_state_tx.send((true, "Pong".to_string())); },
                                    WsMessage::Text(txt) => {
                                        if txt == "READY".to_string() {
                                            let _ = start_tx.send(true);
                                            *ready = true;
                                            log::info!("WebSocket sender marked as ready");
                                        } else if *ready {
                                            // Deserialize incoming TerminalEvent from egui and forward to rendering loop
                                            if let Ok(cmd) = serde_json::from_str::<Cmd>(&txt) {
                                                match cmd {
                                                    Cmd::InteractiveInput(input) => {
                                                        // 1) If not already spawned, create the PTY:
                                                        if maybe_pty.is_none() {
                                                            maybe_pty = Some(spawn_interactive_pty(&mut sender).await?);
                                                        }
                                                        
                                                        // 2) Write the new input to the PTY
                                                        if let Some(ref mut session) = maybe_pty {
                                                            session
                                                                .pty
                                                                .write_all(input.as_bytes())
                                                                .await?;
                                                            // Possibly also add a newline or \r if needed
                                                        }
                                                    },
                                                    _ => {}
                                                }
                                            }
                                        }
                                    },
                                    WsMessage::Binary(bin) => {
                                        if *ready {
                                            // Deserialize incoming TerminalEvent from egui and forward to rendering loop
                                            if let Ok(event) = serde_json::from_slice::<TerminalEvent>(&bin) {
                                                log::info!("Received TerminalEvent from egui: {:?}", event);
                                                if event_tx.send(event.into()).is_ok() {
                                                    log::info!("Forwarded TerminalEvent to rendering loop");
                                                } else {
                                                    log::warn!("Failed to forward TerminalEvent to rendering loop");
                                                }
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            },
                        }
                    }
    
                    // Let other tasks run before looping again // THIS IS REQUIRED, or else the 
                    // server will not actually receive an Open event, and will terminate the loop
                    // immediately. we need to give some CPU time to yield, allowing the websocket
                    // handshake to complete
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    
                    if let Ok(()) = shutdown_rx.try_recv() {
                        client.disconnect_client();
                        // kill the PTY child
                        if let Some(ref mut session) = maybe_pty {
                            let _ = session.child.kill().await;
                        }
                        *ready = false;
                        break;
                    }
    
                    if *ready {
                        tokio::select! {
                            Some((frame_count, buffer)) = buffer_rx.recv() => {
                                log::info!("Sending buffer, frame_count={}", frame_count);
                                let send_start = Instant::now();
                                let serialized = encode_buffer_with_timestamp(frame_count as u64, &buffer)?;
                                sender.send(WsMessage::Binary(serialized));
                                let send_duration = send_start.elapsed();
                                log::info!("Buffer sent, frame_count={}, send_duration={:?}", frame_count, send_duration);
                            }
                        }
                    }
                }
            
            }
            Err(e) => log::info!("Failed to establish WebSocket connection: {e:?}")
        }
        Ok(())
    }

    pub fn send_buffer(
        f: &mut Frame, 
        last_sent: &mut Instant, 
        send_interval: Duration, 
        can_start: &mut bool,
        buffer_tx: tokio::sync::mpsc::UnboundedSender<(usize, Buffer)>
    ) {
        let now = Instant::now(); // Changed: Throttle buffer sending
        if now.duration_since(*last_sent) >= send_interval {
            if *can_start {
                let buffer_to_send = f.buffer_mut().clone();
                let tx = buffer_tx.clone();
                let count = f.count();
                std::thread::scope(|s| {
                    s.spawn(|| {
                        if let Err(e) = tx.send((count, buffer_to_send)) {
                            log::warn!("Failed to send buffer: {:?}", e);
                        }
                    });
                });
                *last_sent = now;
            }
        }
    }
}


async fn spawn_interactive_pty(
    sender: &mut ewebsock::WsSender
) -> anyhow::Result<MyPtySession> {
    // Create a new PTY:
    let (mut pty, pts) = pty_process::open()?;
    // pty.resize(pty_process::Size::new(24, 80))?;

    // e.g., spawn a shell, or a program that expects a TTY:
    let mut child = pty_process::Command::new("powershell")
        .spawn(pts)?;  // returns a tokio::process::Child

    // Background task: read from PTY and forward output to client
    let mut pty_reader = pty.clone(); // clone so we can read here & keep the “master” handle
    let mut buffer = [0u8; 4096];
    let mut ws_sender = sender.clone();

    tokio::spawn(async move {
        loop {
            match pty_reader.read(&mut buffer).await {
                Ok(n) if n == 0 => {
                    // The child exited or the PTY was closed
                    log::info!("PTY closed or child exited");
                    break;
                }
                Ok(n) => {
                    // Forward to the client via WebSocket
                    let chunk = &buffer[..n];
                    // e.g. send as text, or as binary with raw bytes:
                    ws_sender.send(ewebsock::WsMessage::Binary(chunk.to_vec()));
                }
                Err(e) => {
                    log::error!("Error reading PTY: {:?}", e);
                    break;
                }
            }
        }
        log::info!("PTY -> WebSocket loop ended");
    });

    // Return a handle so we can write to the PTY or kill the child later
    Ok(MyPtySession { pty, child })
}

pub async fn create_client(mut client: ConnectedClient) -> anyhow::Result<(), anyhow::Error> {
    client.connected = true;
    
    // log::info!("Client: {client:?}");
    
    let query_id = query_id::<ConnectedClient>(
        CONNECTED_CLIENT_TABLE.to_string(), 
        client.id.clone()
    ).await;

    log::info!("websockets -> query_id: {query_id:?}");

    let check_id_existence = check_id_existence(
        CONNECTED_CLIENT_TABLE.to_string(), 
        client.id.clone()
    ).await;
    
    log::info!("websockets -> check_id_existence: {check_id_existence:?}");
    
    if let Ok(Some(_)) = query_id {
        log::info!("WE HAVE A CLIENT");
    } else {
        let res: Option<ConnectedClient> = DATABASE
            .upsert(client.id.clone())
            .content(client)
            .await?
            .take();

        log::info!("websockets -> Upsert: {res:?}");
    }
    Ok(())
}