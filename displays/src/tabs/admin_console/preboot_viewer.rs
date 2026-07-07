//! Pre-boot terminal viewer.
//!
//! A UEFI firmware app can only dial out, so it streams its TUI to the axum
//! relay over HTTP rather than accepting a direct connection. This viewer polls
//! the relay for the latest frame, renders it with the shared [`RataguiBackend`]
//! (identical to the OS-client terminal viewer), and POSTs captured input back.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crossbeam::channel::{Receiver, Sender, unbounded};
use eframe::egui::Ui;

use crate::remote_viewer::preboot::{from_preboot, terminal_event_to_preboot};
use crate::remote_viewer::ratagui::{RataguiBackend, TerminalEvent};
use crate::{PlatformSpawner, Spawner};

/// Percent-encode a serial for safe use as a path segment.
fn enc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub struct PreBootViewer {
    pub serial: String,
    base_url: String,
    backend: RataguiBackend,
    event_rx: Receiver<TerminalEvent>,
    frame_rx: Receiver<Vec<u8>>,
    frame_tx: Sender<Vec<u8>>,
    fetching: Arc<AtomicBool>,
    /// Highest firmware frame counter rendered (for display/debug).
    pub last_seq: u64,
    /// While set, poll frames aggressively — a keystroke was just sent and a
    /// screen update is imminent, so we want it the instant it lands.
    burst_until: Option<web_time::Instant>,
}

impl PreBootViewer {
    pub fn new(serial: String, base_url: String) -> Self {
        let (ev_tx, event_rx) = unbounded();
        let backend = RataguiBackend::new(80, 25, ev_tx);
        let (frame_tx, frame_rx) = unbounded();
        Self {
            serial,
            base_url,
            backend,
            event_rx,
            frame_rx,
            frame_tx,
            fetching: Arc::new(AtomicBool::new(false)),
            last_seq: 0,
            burst_until: None,
        }
    }

    /// Repaint cadence: fast right after a keystroke (the screen update is
    /// imminent), relaxed when idle (still smooth for self-animating views).
    pub fn repaint_after(&self) -> std::time::Duration {
        let bursting = self.burst_until.map(|t| web_time::Instant::now() < t).unwrap_or(false);
        std::time::Duration::from_millis(if bursting { 33 } else { 150 })
    }

    /// Fetch the latest frame (one GET in flight at a time) and render the most
    /// recent one received. Call once per egui frame.
    pub fn poll(&mut self) {
        if !self.fetching.swap(true, Ordering::AcqRel) {
            let url = format!("{}/api/v1/qc/preboot/{}/frame", self.base_url, enc(&self.serial));
            let tx = self.frame_tx.clone();
            let flag = self.fetching.clone();
            PlatformSpawner::spawn(async move {
                if let Ok(resp) = reqwest::get(&url).await {
                    if resp.status().as_u16() == 200 {
                        if let Ok(bytes) = resp.bytes().await {
                            if !bytes.is_empty() {
                                let _ = tx.try_send(bytes.to_vec());
                            }
                        }
                    }
                }
                flag.store(false, Ordering::Release);
            });
        }
        let mut latest = None;
        while let Ok(b) = self.frame_rx.try_recv() {
            latest = Some(b);
        }
        if let Some(bytes) = latest {
            if let Some(frame) = tcp_protocol::preboot::decode_frame(&bytes) {
                self.last_seq = frame.frame;
                self.backend.update_buffer(from_preboot(&frame));
            }
        }
    }

    /// Render the terminal and ship any captured input back to the relay.
    pub fn ui(&mut self, ui: &mut Ui) {
        ui.add(&mut self.backend);
        while let Ok(ev) = self.event_rx.try_recv() {
            let Some(pb) = terminal_event_to_preboot(&ev) else {
                continue;
            };
            let body = tcp_protocol::preboot::encode_event(&pb);
            let url = format!("{}/api/v1/qc/preboot/{}/input", self.base_url, enc(&self.serial));
            PlatformSpawner::spawn(async move {
                let client = reqwest::Client::new();
                let _ = client.post(&url).body(body).send().await;
            });
            // A screen update is coming — poll frames hard for a moment.
            self.burst_until = Some(web_time::Instant::now() + std::time::Duration::from_millis(600));
        }
    }
}
