use web_time::{Duration, Instant};
use ratatui::prelude::*;
use eframe::egui::Ui;

use super::RemoteTerminal;

impl RemoteTerminal {
    /// Drain the buffer queue down to the newest frame and apply it. Returns
    /// `true` when a frame was applied. Called every frame from
    /// `WebSocketClient::receive` so the queue drains regardless of view state.
    pub fn poll_frames(&mut self) -> bool {
        let mut latest_buffer = None;
        while let Ok((frame_index, buffer)) = self.buffer_rx.try_recv() {
            if frame_index > self.latest_frame_index {
                latest_buffer = Some((frame_index, buffer));
            }
        }

        let Some((frame_index, buffer)) = latest_buffer else {
            return false;
        };

        // No resize to `last_target_area` here. `Buffer::resize` is a flat truncate/extend with no
        // 2-D reflow, so on any area disagreement it reinterprets the cell array at the wrong stride
        // and shears the frame — which also lands clicks on the wrong cell. `build_row_job` indexes
        // through `Buffer::cell`, which is bounds-checked against the buffer's own area, so a
        // mismatched buffer already crops or pads correctly without being rewritten.
        self.terminal.backend_mut().set_frame_index(frame_index);
        self.terminal.backend_mut().update_buffer(buffer);
        self.latest_frame_index = frame_index;
        self.buffer_count += 1;
        self.has_received_frame = true;
        log::debug!(
            "Received pre-processed buffer: frame_index={}, area={:?}",
            frame_index,
            self.terminal.backend().buffer().area
        );
        true
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        let mut needs_repaint = false;

        if self.poll_frames() {
            needs_repaint = true;
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
        log::debug!("Draw duration: {:?}", draw_duration);

        eframe::egui::CentralPanel::default().show(ui, |ui| {
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

        // Ask for the grid the backend just laid out, rather than converting points to cells a
        // second way. One frame of lag on a resize, but the requested area and the displayed area
        // always converge on the same numbers.
        let (cols, rows) = self.terminal.backend().grid();
        let grid = Rect::new(0, 0, cols, rows);
        if grid != self.last_target_area {
            let _ = self.size_tx.send(grid);
            self.cached_buffer.resize(grid);
            self.last_target_area = grid;
            log::debug!("Target area updated: {grid:?}");
            needs_repaint = true;
        }

        if needs_repaint {
            ui.ctx().request_repaint();
        }

        if self.last_log.elapsed() >= Duration::from_secs(1) {
            log::debug!(
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