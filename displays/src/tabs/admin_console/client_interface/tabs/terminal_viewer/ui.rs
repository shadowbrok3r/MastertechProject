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
        
        // Track if this is the first frame
        // let mut is_first_frame = self.latest_frame_index == 0; 

        if target_area != self.last_target_area {
            let _ = self.size_tx.send(target_area);
            self.terminal.backend_mut().resize(target_width, target_height);
            self.cached_buffer.resize(target_area);
            self.last_target_area = target_area;
            log::debug!("Target area updated: {:?}", target_area);
            needs_repaint = true;
        }

        // Process only the latest buffer
        let mut latest_buffer = None;
        while let Ok((frame_index, buffer)) = self.buffer_rx.try_recv() {
            if frame_index > self.latest_frame_index {
                latest_buffer = Some((frame_index, buffer));
            }
        }

        if let Some((frame_index, mut buffer)) = latest_buffer {
            if buffer.area != self.last_target_area {
                buffer.resize(self.last_target_area);
                log::debug!("Resized incoming buffer to: {:?}", self.last_target_area);
            }
            self.terminal.backend_mut().set_frame_index(frame_index);
            self.terminal.backend_mut().update_buffer(buffer);
            self.latest_frame_index = frame_index;
            self.buffer_count += 1;
            needs_repaint = true;
            log::info!(
                "Received pre-processed buffer: frame_index={}, area={:?}",
                frame_index,
                self.terminal.backend().buffer().area
            );
        }

        // Send events
        while let Ok(event) = self.event_rx.try_recv() {
            let serialized = serde_json::to_vec(&event).expect("Failed to serialize event");
            self.msg_to_client.try_send(ewebsock::WsMessage::Binary(serialized)).unwrap();
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