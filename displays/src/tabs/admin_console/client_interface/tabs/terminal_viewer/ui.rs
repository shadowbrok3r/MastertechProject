use web_time::{Duration, Instant};
use ratatui::prelude::*;
use eframe::egui::Ui;

use super::RemoteTerminal;

impl RemoteTerminal {
    pub fn ui(&mut self, ui: &mut Ui) {
        let available_size = ui.available_size();
        let target_width = (available_size.x as u16).min(250);
        let target_height = (available_size.y as u16).min(250);
        let target_area = Rect::new(0, 0, target_width, target_height);
        let mut needs_repaint = false;
        let mut latest_buffer = None;
        // Track if this is the first frame
        let mut is_first_frame = self.latest_frame_index == 0; 

        if target_area != self.last_target_area {
            let _ = self.size_tx.send(target_area);
            self.terminal.backend_mut().resize(target_width, target_height);
            self.cached_buffer.resize(target_area);
            self.last_target_area = target_area;
            log::debug!("Target area updated: {:?}", target_area);
            needs_repaint = true;
        }

        while let Ok((frame_index, mut new_buffer)) = self.buffer_rx.try_recv() {
            let frame_index_usize = frame_index as usize;
            let latest_frame_usize = self.latest_frame_index as usize;

            if is_first_frame || frame_index_usize > latest_frame_usize {
                if new_buffer.area != self.last_target_area {
                    new_buffer.resize(self.last_target_area);
                    log::debug!("Resized incoming buffer to: {:?}", self.last_target_area);
                }

                latest_buffer = Some((frame_index_usize, new_buffer));
                self.buffer_count += 1;

                if is_first_frame {
                    log::debug!("Accepted first frame: frame_index={}", frame_index_usize);
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
            // let serialized = serde_json::to_string(&event).expect("Failed to serialize event");
            // let _ = self.msg_to_client.try_send(ewebsock::WsMessage::Text(serialized));

            let serialized = serde_json::to_vec(&event).expect("Failed to serialize event");
            let _ = self.msg_to_client.try_send(ewebsock::WsMessage::Binary(serialized));
        }

        if let Some((frame_index, buffer)) = latest_buffer {
            self.terminal.backend_mut().set_frame_index(frame_index as u64);
            self.terminal.backend_mut().update_buffer(buffer);
            self.latest_frame_index = frame_index as u64;
            needs_repaint = true;
            log::debug!(
                "Received pre-processed buffer: frame_index={frame_index}, area={:?}",
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
        log::debug!("Draw duration: {:?}", draw_duration);

        eframe::egui::CentralPanel::default().show_inside(ui, |ui| {
            let render_start = Instant::now();

            ui.add(self.terminal.backend_mut());

            let render_duration = render_start.elapsed();
            let since_last_repaint = self.last_repaint.elapsed();

            if since_last_repaint >= Duration::from_millis(16) {
                log::debug!("Frame Count: {}", self.frame_count);
                log::debug!("Time since last repaint: {:?}", since_last_repaint);
                log::debug!("Render duration: {:?}", render_duration);
                self.last_repaint = Instant::now();
            }
        });

        if needs_repaint { ui.ctx().request_repaint(); }

        if self.last_log.elapsed() >= Duration::from_secs(1) {
            log::debug!(
                "Performance: buffer_count={}, frame_count={}, last_draw_duration={draw_duration:?}",
                self.buffer_count,
                self.frame_count - self.last_log_frame_count,
            );
            self.last_log = Instant::now();
            self.last_log_frame_count = self.frame_count;
            self.buffer_count = 0;
        }
    }
}