//! Pre-boot terminal viewer.
//!
//! A UEFI firmware app can only dial out, so it streams its TUI to the axum
//! relay over HTTP rather than accepting a direct connection. This viewer polls
//! the relay for the latest frame, renders it with the shared [`RataguiBackend`]
//! (identical to the OS-client terminal viewer), and POSTs captured input back.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crossbeam::channel::{Receiver, Sender, unbounded};
use eframe::egui::{Color32, RichText, Ui};

use crate::remote_viewer::preboot::{from_preboot, terminal_event_to_preboot};
use crate::remote_viewer::ratagui::{RataguiBackend, TerminalEvent};
use crate::{PlatformSpawner, Spawner};

/// One connected pre-boot box as reported by the relay's session list.
pub struct PreBootAgent {
    pub serial: String,
    pub idle_secs: u64,
    pub has_frame: bool,
    pub streaming: bool,
    pub log_lines: u64,
}

/// Root-gated roster of currently-connected UEFI apps, polled from the relay's
/// `GET /api/v1/qc/preboot`. Renders clickable rows; returns a serial to view.
#[derive(Default)]
pub struct PreBootRoster {
    agents: Vec<PreBootAgent>,
    rx: Option<Receiver<Vec<PreBootAgent>>>,
    fetching: std::sync::Arc<AtomicBool>,
}

impl PreBootRoster {
    pub fn ui(&mut self, ui: &mut Ui, base_url: &str) -> Option<String> {
        if let Some(rx) = &self.rx {
            if let Ok(list) = rx.try_recv() {
                self.agents = list;
                self.rx = None;
            }
        }
        if self.rx.is_none() && !self.fetching.swap(true, Ordering::AcqRel) {
            let (tx, rxc) = unbounded();
            self.rx = Some(rxc);
            let url = format!("{base_url}/api/v1/qc/preboot");
            let flag = self.fetching.clone();
            PlatformSpawner::spawn(async move {
                let mut out = Vec::new();
                if let Ok(resp) = reqwest::get(&url).await {
                    if let Ok(v) = resp.json::<serde_json::Value>().await {
                        if let Some(arr) = v.get("sessions").and_then(|s| s.as_array()) {
                            for s in arr {
                                let serial = s.get("serial").and_then(|x| x.as_str()).unwrap_or("").to_string();
                                if serial.is_empty() {
                                    continue;
                                }
                                out.push(PreBootAgent {
                                    serial,
                                    idle_secs: s.get("idle_secs").and_then(|x| x.as_u64()).unwrap_or(u64::MAX),
                                    has_frame: s.get("has_frame").and_then(|x| x.as_bool()).unwrap_or(false),
                                    streaming: s.get("streaming").and_then(|x| x.as_bool()).unwrap_or(false),
                                    log_lines: s.get("log_lines").and_then(|x| x.as_u64()).unwrap_or(0),
                                });
                            }
                        }
                    }
                }
                let _ = tx.try_send(out);
                flag.store(false, Ordering::Release);
            });
        }

        let mut selected = None;
        if self.agents.is_empty() {
            ui.label(
                RichText::new(
                    "no connected UEFI apps — a box appears here once it reaches the relay \
                     (on the box: 'c' connect, 'd' DHCP, target http://<axum-LAN-IP>:8082 via 'e')",
                )
                .weak(),
            );
        }
        for a in &self.agents {
            ui.horizontal(|ui| {
                let live = a.idle_secs < 60;
                let color = if live { Color32::from_rgb(120, 220, 130) } else { Color32::GRAY };
                ui.colored_label(color, &a.serial);
                ui.label(if a.streaming { "streaming" } else { "connected" });
                ui.weak(format!("{}s", a.idle_secs));
                if a.log_lines > 0 {
                    ui.weak(format!("logs: {}", a.log_lines));
                }
                if ui.button("View").clicked() {
                    selected = Some(a.serial.clone());
                }
            });
        }
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(1500));
        selected
    }
}

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
    /// Firmware terminal dimensions from the last decoded frame.
    grid: Option<(u16, u16)>,
    /// A frame has been decoded; the waiting placeholder shows until then.
    received_frame: bool,
    /// Relay returned 404 for this serial (firmware side not connected).
    session_missing: Arc<AtomicBool>,
    /// The frame poll itself failed (relay unreachable from this console).
    poll_err: Arc<AtomicBool>,
    /// While set, poll frames aggressively — a keystroke was just sent and a
    /// screen update is imminent, so we want it the instant it lands.
    burst_until: Option<web_time::Instant>,
    /// Direct-link source: when set, frames/input flow over the console's TCP
    /// session with the firmware instead of the HTTP relay.
    #[cfg(not(target_arch = "wasm32"))]
    direct: Option<super::preboot_direct::DirectHub>,
    /// Last direct frame seq rendered (decode only on change).
    direct_seq: u64,
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
            grid: None,
            received_frame: false,
            session_missing: Arc::new(AtomicBool::new(false)),
            poll_err: Arc::new(AtomicBool::new(false)),
            burst_until: None,
            #[cfg(not(target_arch = "wasm32"))]
            direct: None,
            direct_seq: 0,
        }
    }

    /// Direct-link viewer: frames/input ride the console's TCP session with the
    /// firmware (via `hub`) instead of the HTTP relay.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new_direct(serial: String, hub: super::preboot_direct::DirectHub) -> Self {
        let mut v = Self::new(serial, String::new());
        v.direct = Some(hub);
        v
    }

    /// The direct hub + serial when this viewer streams over a direct socket,
    /// so the window can offer plugin-push against it.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn direct_target(&self) -> Option<(super::preboot_direct::DirectHub, String)> {
        self.direct.clone().map(|h| (h, self.serial.clone()))
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
        // Direct-link source: pull the newest frame straight from the console's
        // TCP session with the firmware; no HTTP.
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(hub) = &self.direct {
            self.session_missing.store(!hub.is_connected(&self.serial), Ordering::Release);
            self.poll_err.store(false, Ordering::Release);
            let seq = hub.frame_seq(&self.serial);
            if seq != self.direct_seq {
                if let Some(bytes) = hub.latest_frame(&self.serial) {
                    if let Some(frame) = tcp_protocol::preboot::decode_frame(&bytes) {
                        self.last_seq = frame.frame;
                        self.grid = Some((frame.cols, frame.rows));
                        self.received_frame = true;
                        self.backend.update_buffer(from_preboot(&frame));
                    }
                }
                self.direct_seq = seq;
            }
            return;
        }
        if !self.fetching.swap(true, Ordering::AcqRel) {
            let url = format!("{}/api/v1/qc/preboot/{}/frame", self.base_url, enc(&self.serial));
            let tx = self.frame_tx.clone();
            let flag = self.fetching.clone();
            let missing = self.session_missing.clone();
            let err = self.poll_err.clone();
            PlatformSpawner::spawn(async move {
                match reqwest::get(&url).await {
                    Ok(resp) => {
                        err.store(false, Ordering::Release);
                        let code = resp.status().as_u16();
                        missing.store(code == 404, Ordering::Release);
                        if code == 200 {
                            if let Ok(bytes) = resp.bytes().await {
                                if !bytes.is_empty() {
                                    let _ = tx.try_send(bytes.to_vec());
                                }
                            }
                        }
                    }
                    Err(_) => err.store(true, Ordering::Release),
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
                self.grid = Some((frame.cols, frame.rows));
                self.received_frame = true;
                self.backend.update_buffer(from_preboot(&frame));
            }
        }
    }

    /// Render the terminal and ship any captured input back to the relay.
    pub fn ui(&mut self, ui: &mut Ui) {
        if !self.received_frame {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.spinner();
                if self.poll_err.load(Ordering::Acquire) {
                    ui.label(RichText::new(format!(
                        "relay unreachable at {} — check the Relay URL / network",
                        self.base_url
                    )).color(Color32::from_rgb(240, 140, 130)));
                } else if self.session_missing.load(Ordering::Acquire) {
                    ui.label(RichText::new(format!(
                        "'{}' is not connected to the relay — on the box check the target \
                         ('e', http://<axum-LAN-IP>:8082) and its Log tab",
                        self.serial
                    )).color(Color32::from_rgb(240, 140, 130)));
                } else {
                    ui.label(RichText::new(format!(
                        "waiting for first frame from '{}' — the box auto-starts streaming \
                         within ~5 s of this viewer opening",
                        self.serial
                    )).weak());
                }
            });
            // Input typed before the first frame has no live session to go to.
            while self.event_rx.try_recv().is_ok() {}
            return;
        }
        if self.poll_err.load(Ordering::Acquire) {
            ui.colored_label(
                Color32::from_rgb(240, 140, 130),
                "relay unreachable — showing the last received frame",
            );
        } else if self.session_missing.load(Ordering::Acquire) {
            ui.colored_label(
                Color32::from_rgb(240, 140, 130),
                format!("'{}' dropped off the relay — showing its last frame", self.serial),
            );
        }
        // Scale the font so the full firmware grid fits the window.
        if let Some((cols, rows)) = self.grid {
            self.backend.fit_font_to_grid(ui, cols, rows);
        }
        ui.add(&mut self.backend);
        while let Ok(ev) = self.event_rx.try_recv() {
            let Some(pb) = terminal_event_to_preboot(&ev) else {
                continue;
            };
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(hub) = &self.direct {
                hub.send_input(&self.serial, &pb);
                self.burst_until =
                    Some(web_time::Instant::now() + std::time::Duration::from_millis(600));
                continue;
            }
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
