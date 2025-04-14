
use crate::remote_viewer::decode_buffer;
use web_time::{Instant, SystemTime};
use crossbeam::channel::{Receiver, Sender};
use ratatui::{buffer::Cell, prelude::*};

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
                    &new_buffer.into(), 
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

pub fn resize_buffer(source: &Buffer, target_area: Rect) -> Buffer {
    let width = target_area.width as usize;
    let height = target_area.height as usize;
    let source_width = source.area.width as usize;
    let source_height = source.area.height as usize;
    let copy_width = source_width.min(width);
    let copy_height = source_height.min(height);

    let mut content = vec![Cell::default(); width * height];
    for y in 0..copy_height {
        for x in 0..copy_width {
            if let Some(cell) = source.cell(Position::new(x as u16, y as u16)) {
                content[y * width + x] = cell.clone();
            }
        }
    }

    Buffer {
        area: target_area,
        content,
    }
}