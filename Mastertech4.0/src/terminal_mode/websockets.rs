use database::{schema::{utilities::{check_id_existence, query_id}, ConnectedClient, CONNECTED_CLIENT_TABLE}, DATABASE, WS_CLIENT_URL};
use displays::remote_viewer::{encode_buffer_with_timestamp, ratagui::TerminalEvent};
use crate::filesystem::get_client_hash;
use ewebsock::{WsEvent, WsMessage};
use ratatui::{buffer::Buffer, Frame};
use std::time::{Duration, Instant};
use super::TerminalApp;
use tokio;

impl<'a> TerminalApp<'a> {
    pub async fn start_websocket_sender(
        mut buffer_rx: tokio::sync::mpsc::UnboundedReceiver<(usize, Buffer)>, // Changed: Receive frame count
        start_tx: tokio::sync::mpsc::UnboundedSender<bool>,
        event_tx: tokio::sync::mpsc::UnboundedSender<TerminalEvent>,
        mut shutdown_rx: tokio::sync::broadcast::Receiver<()>
    ) -> anyhow::Result<()> {
        let client = get_client_hash();

        let connection_url = format!(
            "{WS_CLIENT_URL}&room_id={}",
            client.connection_string
        );

        let connection = ewebsock::connect(
            connection_url,
            ewebsock::Options::default(),
        );

        if let Ok((mut sender, receiver)) = connection {
            let ready = &mut false;

            loop {
                // Handle WebSocket events (e.g., READY or TerminalEvent from egui)
                while let Some(event) = receiver.try_recv() {
                    log::info!("Received WebSocket event: {:?}", event);
                    if let WsEvent::Message(WsMessage::Text(txt)) = event {
                        if txt == "READY".to_string() {
                            let _ = start_tx.send(true);
                            *ready = true;
                            log::info!("WebSocket sender marked as ready");
                        } else if *ready {
                            // Deserialize incoming TerminalEvent from egui and forward to rendering loop
                            if let Ok(event) = serde_json::from_str::<TerminalEvent>(&txt) {
                                log::info!("Received TerminalEvent from egui: {:?}", event);
                                if event_tx.send(event).is_ok() {
                                    log::info!("Forwarded TerminalEvent to rendering loop");
                                } else {
                                    log::warn!("Failed to forward TerminalEvent to rendering loop");
                                }
                            }
                        }
                    }
                }

                if let Ok(()) = shutdown_rx.try_recv() {
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
        } else {
            log::error!("Failed to establish WebSocket connection");
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

pub async fn create_client(mut client: ConnectedClient) -> anyhow::Result<(), anyhow::Error> {
    client.connected = true;
    
    log::info!("Client: {client:?}");
    
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