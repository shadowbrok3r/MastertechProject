use base64::{engine::general_purpose, Engine};
use ewebsock::{Options, WsSender};
use ratatui::prelude::*;
use eframe::egui::Ui;
use std::time::{Instant, Duration};
use crossbeam::channel::{unbounded, Sender, Receiver};
use database::{schema::utilities::decompress_data, WS_MASTER_URL};
use crate::{remote_viewer::decode_buffer, PlatformSpawner, Spawner};

use super::ratagui::{RataguiBackend, TerminalEvent};

pub struct RemoteTerminal {
    pub terminal: Terminal<RataguiBackend>,
    cached_buffer: Buffer,
    buffer_rx: Receiver<(u64, Buffer)>, // Add frame_index to buffer updates
    buffer_tx: Sender<(u64, Buffer)>,
    event_rx: Receiver<TerminalEvent>,
    event_tx: Sender<TerminalEvent>,
    size_tx: Sender<Rect>,
    ws_sender: WsSender,
    buffer_count: usize,
    frame_count: usize,
    last_log: Instant,
    last_repaint: Instant,
    last_log_frame_count: usize,
    last_target_area: Rect,
    latest_frame_index: u64, // Track the latest rendered frame
}

impl RemoteTerminal {
    pub fn new() -> Self {
        let (buffer_tx, buffer_rx) = unbounded();
        let (size_tx, size_rx) = unbounded();
        let (event_tx, event_rx) = unbounded(); // New: Event channel
        let initial_area = Rect::new(0, 0, 250, 250);
        let (mut ws_sender, ws_receiver) = ewebsock::connect(
            format!("{WS_MASTER_URL}&room_id=test"), 
            Options::default()
        ).expect("Failed to connect to websocket server");

        let tx = buffer_tx.clone();

        ws_sender.send(ewebsock::WsMessage::Text("READY".to_string()));

        PlatformSpawner::spawn(async move {
            let mut current_area = initial_area;
            loop {
                while let Some(ws_event) = ws_receiver.try_recv() {
                    log::warn!("EVT");
                    while let Ok(new_area) = size_rx.try_recv() {
                        current_area = new_area;
                    }
                    let receive_start = Instant::now();
                    match ws_event {
                        ewebsock::WsEvent::Message(ws_message) => {
                            if let ewebsock::WsMessage::Binary(buffer_array) = ws_message {
                                match decode_buffer(&buffer_array) {
                                    Ok((frame_index, new_buffer)) => {
                                        let resized_buffer = resize_buffer(&new_buffer, current_area);
                                        if tx.send((frame_index, resized_buffer)).is_err() {
                                            log::warn!("Failed to send buffer to UI thread");
                                            break;
                                        }
                                        let duration = receive_start.elapsed();
                                        log::info!("Buffer processed: duration={:?}, frame_index={}", duration, frame_index);
                                    }
                                    Err(e) => log::warn!("Error decoding message: {e:?}"),
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        });

        let mut backend = RataguiBackend::new(initial_area.width, initial_area.height, event_tx.clone()); // Changed: Use initial_area size
        backend.set_frame_index(0);
        let terminal = Terminal::new(backend).unwrap();
        
        Self {
            ws_sender,
            terminal,
            cached_buffer: Buffer::empty(initial_area),
            buffer_rx,
            buffer_tx,
            size_tx,
            event_rx,
            event_tx,
            buffer_count: 0,
            frame_count: 0,
            last_log: Instant::now(),
            last_repaint: Instant::now(),
            last_log_frame_count: 0,
            last_target_area: initial_area,
            latest_frame_index: 0,
        }
    }

    pub fn render(&mut self, ui: &mut Ui) {
        let available_size = ui.available_size();
        let target_width = (available_size.x as u16).min(250);
        let target_height = (available_size.y as u16).min(250);
        let target_area = Rect::new(0, 0, target_width, target_height);

        let mut needs_repaint = false;
        if target_area != self.last_target_area {
            let _ = self.size_tx.send(target_area);
            self.terminal.backend_mut().resize(target_width, target_height);
            self.cached_buffer.resize(target_area);
            self.last_target_area = target_area;
            log::info!("Target area updated: {:?}", target_area);
            needs_repaint = true;
        }

        let mut latest_buffer = None;
        let mut is_first_frame = self.latest_frame_index == 0; // Track if this is the first frame
        while let Ok((frame_index, mut new_buffer)) = self.buffer_rx.try_recv() {
            let frame_index_usize = frame_index as usize;
            let latest_frame_usize = self.latest_frame_index as usize;
            if is_first_frame || frame_index_usize > latest_frame_usize {
                if new_buffer.area != self.last_target_area {
                    new_buffer.resize(self.last_target_area);
                    log::info!("Resized incoming buffer to: {:?}", self.last_target_area);
                }
                latest_buffer = Some((frame_index_usize, new_buffer));
                self.buffer_count += 1;
                if is_first_frame {
                    log::info!("Accepted first frame: frame_index={}", frame_index_usize);
                    is_first_frame = false; // Only accept first frame once
                }
            } else {
                log::warn!(
                    "Dropped out-of-order or duplicate frame: received={}, latest_accepted={}",
                    frame_index_usize,
                    latest_frame_usize
                );
            }
        }

        // Send events over WebSocket
        while let Ok(event) = self.event_rx.try_recv() {
            let serialized = serde_json::to_string(&event).expect("Failed to serialize event");
            self.ws_sender.send(ewebsock::WsMessage::Text(serialized));
        }

        if let Some((frame_index, buffer)) = latest_buffer {
            self.terminal.backend_mut().set_frame_index(frame_index as u64);
            self.terminal.backend_mut().update_buffer(buffer);
            self.latest_frame_index = frame_index as u64;
            needs_repaint = true;
            log::info!(
                "Received pre-processed buffer: frame_index={}, area={:?}",
                frame_index,
                self.terminal.backend().buffer().area
            );
        }

        let draw_start = Instant::now();
        self.terminal
            .draw(|_f| {
                self.frame_count += 1;
            })
            .expect("Failed to draw terminal frame");

        let draw_duration = draw_start.elapsed();
        log::info!("Draw duration: {:?}", draw_duration);

        eframe::egui::CentralPanel::default().show_inside(ui, |ui| {
            let render_start = Instant::now();
            ui.add(self.terminal.backend_mut());
            let render_duration = render_start.elapsed();
            let since_last_repaint = self.last_repaint.elapsed();
            if since_last_repaint >= Duration::from_millis(16) {
                log::info!("Frame Count: {}", self.frame_count);
                log::info!("Time since last repaint: {:?}", since_last_repaint);
                log::info!("Render duration: {:?}", render_duration);
                self.last_repaint = Instant::now();
            }
        });

        if needs_repaint {
            ui.ctx().request_repaint();
        }

        if self.last_log.elapsed() >= Duration::from_secs(1) {
            log::info!(
                "Performance: buffer_count={}, frame_count={}, last_draw_duration={:?}",
                self.buffer_count,
                self.frame_count - self.last_log_frame_count,
                draw_duration
            );
            self.last_log = Instant::now();
            self.last_log_frame_count = self.frame_count;
            self.buffer_count = 0;
        }
    }

}

/// Decompress and decode the given Vec<u8> (which is base64-encoded compressed JSON)
/// and deserialize it back into a Buffer.
pub fn decompress_buffer(input: Vec<u8>) -> anyhow::Result<Buffer, anyhow::Error> {
    // Convert the input Vec<u8> into a String.
    let encoded_str = String::from_utf8(input)?;
    // Base64-decode into the compressed data.
    let compressed = general_purpose::STANDARD.decode(&encoded_str)?;
    // Decompress the data.
    let decompressed = decompress_data(&compressed)?;
    // Convert decompressed bytes into a string.
    let decompressed_string = String::from_utf8(decompressed)?;
    // Deserialize the JSON string into a Buffer.
    let buf = serde_json::from_str::<Buffer>(&decompressed_string)?;
    Ok(buf)
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