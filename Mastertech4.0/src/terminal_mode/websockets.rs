use displays::remote_viewer::encode_buffer_with_frame;
use ewebsock::{WsEvent, WsMessage};
use crate::terminal_mode::WS_CLIENT_URL;
use std::time::Duration;
use super::TerminalApp;
use ratatui::buffer::Buffer;
use tokio;

impl<'a> TerminalApp<'a> {
    #[unsafe(no_mangle)]
    pub async fn start_websocket_sender(
        mut buffer_rx: tokio::sync::mpsc::UnboundedReceiver<(usize, Buffer)>, // Changed: Receive frame count
        tx: tokio::sync::mpsc::UnboundedSender<bool>
    ) -> anyhow::Result<()> {
        let connection = ewebsock::connect(
            format!("{WS_CLIENT_URL}&room_id=test"),
            ewebsock::Options::default(),
        );
        if let Ok((mut sender, receiver)) = connection {
            let ready = &mut false;

            loop {
                while let Some(event) = receiver.try_recv() {
                    log::info!("Received event: {:?}", event);
                    if let WsEvent::Message(WsMessage::Text(txt)) = event {
                        if txt == "READY".to_string() {
                            let _ = tx.send(true);
                            *ready = true;
                        }
                    }
                }
                if *ready {
                    match buffer_rx.recv().await {
                        Some((frame_count, buffer)) => {
                            log::info!("Sending buffer, frame_count={}", frame_count);
                            let serialized = encode_buffer_with_frame(frame_count as u64, &buffer)?;
                            sender.send(ewebsock::WsMessage::Binary(serialized));
                            tokio::time::sleep(Duration::from_secs_f32(0.3)).await;
                        }
                        None => {
                            log::info!("Buffer channel disconnected");
                            break;
                        }
                    }
                }
            }
        } else {
            log::error!("Failed to establish WebSocket connection");
        }
        Ok(())
    }
}
