// use database::schema::utilities::compress_data;
use displays::remote_viewer::encode_buffer;
use crate::terminal_mode::WS_CLIENT_URL;
// use ratatui::buffer::Buffer;
use std::time::Duration;

use super::TerminalApp;

/// This function spawns a thread that continuously receives a Buffer,
/// serializes it, and sends it via WebSocket.
impl <'a>TerminalApp <'a> {
    #[unsafe(no_mangle)]
    pub async fn start_websocket_sender(mut buffer_rx: tokio::sync::mpsc::UnboundedReceiver<ratatui::buffer::Buffer>) -> anyhow::Result<()> {
        let connection = ewebsock::connect(
            format!("{WS_CLIENT_URL}&room_id=test"), 
            ewebsock::Options::default()
        );
        if let Ok((mut sender, receiver)) = connection {
            loop {
                // Use select-like behavior to handle both buffer reception and WebSocket events
    
                match buffer_rx.recv().await {
                    Some(buffer) => {
                        log::info!("Sending another buffer");
                        sender.send(ewebsock::WsMessage::Binary(encode_buffer(&buffer)?));
                        tokio::time::sleep(Duration::from_secs_f32(0.5)).await;
                    },
                    None => {
                        log::info!("Buffer channel disconnected");
                        
                        // break;
                    }
                }
                // Process incoming WebSocket events to avoid backlog
                while let Some(event) = receiver.try_recv() {
                    log::info!("Received event: {:?}", event);
                }
            }
        } else {
            log::error!("Failed to establish WebSocket connection");
        }
        Ok(())
    }
    
}


// /// Serialize the Buffer to JSON, compress it with Brotli, then base64 encode it and return as Vec<u8>.
// #[unsafe(no_mangle)]
// pub fn encode_buffer(buffer: &Buffer) -> anyhow::Result<Vec<u8>, anyhow::Error> {
//     let mut bytes: Vec<u8> = Vec::new();
//     std::thread::scope(|s| {
//         s.spawn(|| {
//             // Base64-encode the compressed data.
//             // let encoded = general_purpose::STANDARD.encode(&serde_json::to_vec(&buffer)?);
//             // Compress the JSON bytes.
//             let compressed = compress_data(&serde_json::to_vec(&buffer)?)?;
//             bytes = compressed;
//             Ok::<(), anyhow::Error>(())
//         });
//    });

//     // Return the base64 string as bytes.
//     Ok(bytes)
// }
