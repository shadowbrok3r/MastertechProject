
use crate::remote_viewer::decode_buffer;
use web_time::{Instant, SystemTime};
use crossbeam::channel::{Receiver, Sender};
use ratatui::prelude::*;

use super::RemoteTerminal;

impl RemoteTerminal {
    pub fn receive_buffer(
        tx: Sender<(u64, Buffer)>, 
        size_rx: &Receiver<Rect>,
        buffer_array: Vec<u8>, 
        mut current_area: Rect
    ) {
        while let Ok(new_area) = size_rx.try_recv() {
            current_area = new_area;
        }
        let decode_start = Instant::now();
        match decode_buffer(&buffer_array) {
                Ok(buffer_message) => {
                    let new_buffer = buffer_message.buffer;
                    let frame_index = buffer_message.frame_count;
                    let sent_timestamp = buffer_message.timestamp;
                    let encode_duration = buffer_message.encode_duration;
                    let decode_duration = decode_start.elapsed().as_millis() as u128;
    
                    let current_time = SystemTime::now()
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .unwrap()
                        .as_millis();
    
                    let total_latency = current_time.saturating_sub(sent_timestamp);
                    let network_latency = total_latency.saturating_sub(encode_duration as u128 + decode_duration);
    
                    log::info!(
                        r#"
                        Received buffer, 
                        frame_count={}, 
                        timestamp={}, 
                        current_time={}, 
                        total_latency={}ms, 
                        network_latency={}ms, 
                        encode_duration={}ms, 
                        decode_duration={}ms
                        "#,
                        frame_index,
                        sent_timestamp,
                        current_time,
                        total_latency,
                        network_latency,
                        encode_duration,
                        decode_duration
                    );
    
                    let resized_buffer = resize_buffer(
                        &new_buffer, 
                        current_area
                    );
    
                    if tx.send((frame_index, resized_buffer)).is_err() {
                        log::warn!("Failed to send buffer to UI thread");
                        return;
                    }
                }
                Err(e) => log::warn!("Error decoding message: {e:?}"),
            }
    }
}

// Helper function to resize a buffer
pub fn resize_buffer(source: &Buffer, target_area: Rect) -> Buffer {
    let mut new_buffer = Buffer::empty(target_area);

    // Copy content from source to new buffer, respecting bounds
    for y in 0..source.area.height.min(target_area.height) {
        for x in 0..source.area.width.min(target_area.width) {
            if let Some(source_cell) = source.cell((x, y)) {
                if let Some(target_cell) = new_buffer.cell_mut(Position::new(x, y)) {
                    target_cell.clone_from(source_cell);
                }
            }
        }
    }

    new_buffer
}