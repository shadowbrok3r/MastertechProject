//! Push-to-talk session with voice-bridge over the relay room: mic PCM up, the spoken
//! reply down (wire contract in docs/ESP32_BENCH_HARDWARE_PLAN.md).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use esp_idf_svc::ws::client::{EspWebSocketClient, FrameType};
use serde_json::Value;

use crate::ffi::{self, color};

const SAMPLE_RATE: u32 = 16_000;
/// 64 ms of 16 kHz mono PCM16.
const MIC_CHUNK: usize = 2048;
const PING_EVERY: Duration = Duration::from_secs(20);
const BRIDGE_STALE: Duration = Duration::from_secs(50);
const MAX_TALK: Duration = Duration::from_secs(30);
const REPLY_WAIT: Duration = Duration::from_secs(60);
const HEAP_LOG_EVERY: Duration = Duration::from_secs(60);

/// Socket state the websocket callback forwards alongside relay text.
pub const WS_CONNECTED: &str = "__ws_connected__";
pub const WS_DISCONNECTED: &str = "__ws_disconnected__";

/// A frame for the relay, produced off the session thread.
pub enum Outgoing {
    Text(&'static str),
    Audio(Vec<u8>),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    Idle,
    Listening,
    Thinking,
    Speaking,
}

/// Streams mic PCM while `capture` is set, bracketed by `utt_start`/`utt_end`.
pub fn mic_loop(capture: Arc<AtomicBool>, out: SyncSender<Outgoing>) {
    loop {
        if !capture.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(10));
            continue;
        }
        if out.send(Outgoing::Text(r#"{"cmd":"utt_start"}"#)).is_err() {
            return;
        }
        while capture.load(Ordering::Acquire) {
            let mut buf = vec![0u8; MIC_CHUNK];
            let n = ffi::mic_read(&mut buf);
            if n == 0 {
                continue;
            }
            buf.truncate(n);
            if out.send(Outgoing::Audio(buf)).is_err() {
                return;
            }
        }
        if out.send(Outgoing::Text(r#"{"cmd":"utt_end"}"#)).is_err() {
            return;
        }
    }
}

pub fn log_heap() {
    use esp_idf_svc::sys::{
        heap_caps_get_free_size, heap_caps_get_minimum_free_size, MALLOC_CAP_INTERNAL, MALLOC_CAP_SPIRAM,
    };
    let (free, low, psram) = unsafe {
        (
            heap_caps_get_free_size(MALLOC_CAP_INTERNAL),
            heap_caps_get_minimum_free_size(MALLOC_CAP_INTERNAL),
            heap_caps_get_free_size(MALLOC_CAP_SPIRAM),
        )
    };
    log::info!("heap: internal {free} B free ({low} B low-water), psram {psram} B free");
}

/// Plays a 440 Hz tone for `ms` to the speaker.
fn play_tone(ms: u32) {
    let n = (SAMPLE_RATE * ms / 1000) as usize;
    let mut pcm = Vec::<u8>::with_capacity(n * 2);
    for i in 0..n {
        let t = i as f32 / SAMPLE_RATE as f32;
        let s = (t * 440.0 * std::f32::consts::TAU).sin() * 9000.0;
        pcm.extend_from_slice(&(s as i16).to_le_bytes());
    }
    ffi::set_amp(true);
    ffi::speaker_write(&pcm);
}

/// Mic→speaker loopback until `run` clears.
fn echo_loop(run: Arc<AtomicBool>) {
    let mut buf = [0u8; MIC_CHUNK];
    ffi::set_amp(true);
    while run.load(Ordering::Relaxed) {
        let n = ffi::mic_read(&mut buf);
        if n > 0 {
            ffi::speaker_write(&buf[..n]);
        }
    }
}

pub struct Console {
    client: EspWebSocketClient<'static>,
    inbox: Receiver<String>,
    outbox: Receiver<Outgoing>,
    capture: Arc<AtomicBool>,
    echo: Arc<AtomicBool>,
    phase: Phase,
    phase_since: Instant,
    online: bool,
    last_bridge: Option<Instant>,
    last_ping: Option<Instant>,
    last_heap_log: Instant,
    await_release: bool,
}

impl Console {
    pub fn new(
        client: EspWebSocketClient<'static>,
        inbox: Receiver<String>,
        outbox: Receiver<Outgoing>,
        capture: Arc<AtomicBool>,
    ) -> Self {
        let now = Instant::now();
        Self {
            client,
            inbox,
            outbox,
            capture,
            echo: Arc::new(AtomicBool::new(false)),
            phase: Phase::Idle,
            phase_since: now,
            online: false,
            last_bridge: None,
            last_ping: None,
            last_heap_log: now,
            await_release: false,
        }
    }

    /// Runs the session until the relay event channel closes.
    pub fn run(mut self) -> Result<()> {
        self.show_phase();
        loop {
            self.poll_ptt();
            match self.inbox.recv_timeout(Duration::from_millis(15)) {
                Ok(msg) => self.on_message(&msg),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => bail!("relay event channel closed"),
            }
            self.flush_outbox();
            self.tick();
        }
    }

    fn poll_ptt(&mut self) {
        let down = ffi::ptt_pressed();
        if !down {
            self.await_release = false;
        }
        match self.phase {
            Phase::Listening if !down => self.set_phase(Phase::Thinking),
            Phase::Listening if self.phase_since.elapsed() >= MAX_TALK => {
                self.await_release = true;
                self.set_phase(Phase::Thinking);
            }
            Phase::Listening => {}
            _ if down && !self.await_release => self.start_talk(),
            _ => {}
        }
    }

    fn start_talk(&mut self) {
        self.await_release = true;
        if !self.online {
            ffi::set_status("Bridge offline", color::ERROR);
            return;
        }
        ffi::play_stop();
        ffi::set_transcript("");
        ffi::set_reply("");
        self.capture.store(true, Ordering::Release);
        self.set_phase(Phase::Listening);
    }

    /// Mic capture runs only while listening.
    fn set_phase(&mut self, phase: Phase) {
        if phase != Phase::Listening {
            self.capture.store(false, Ordering::Release);
        }
        self.phase = phase;
        self.phase_since = Instant::now();
        self.show_phase();
    }

    fn show_phase(&self) {
        match self.phase {
            Phase::Idle if self.online => ffi::set_status("Hold to talk", color::SUCCESS),
            Phase::Idle => ffi::set_status("Waiting for bridge...", color::MUTED),
            Phase::Listening => ffi::set_status("Listening...", color::ACCENT),
            Phase::Thinking => ffi::set_status("Thinking...", color::TERTIARY),
            Phase::Speaking => ffi::set_status("Speaking...", color::ACCENT),
        }
    }

    /// Returns to idle with `status` shown in place of the idle prompt.
    fn fail(&mut self, status: &str) {
        self.set_phase(Phase::Idle);
        ffi::set_status(status, color::ERROR);
    }

    fn on_message(&mut self, msg: &str) {
        for part in msg.split('\n').map(str::trim).filter(|s| !s.is_empty()) {
            if part.starts_with('{') {
                self.on_json(part);
            } else {
                self.on_notice(part);
            }
        }
    }

    fn on_notice(&mut self, notice: &str) {
        match notice {
            WS_CONNECTED | "MASTER_CONNECTED" => self.ping(),
            WS_DISCONNECTED | "MASTER_DISCONNECTED" => self.set_online(false),
            other => log::info!("relay: {other}"),
        }
    }

    fn on_json(&mut self, line: &str) {
        let v: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("bad relay json ({e}): {line}");
                return;
            }
        };
        let text = v["text"].as_str().unwrap_or("");
        match v["cmd"].as_str().unwrap_or("") {
            "pong" | "tts_end" => self.mark_bridge(),
            "state" => {
                self.mark_bridge();
                if v["state"].as_str() == Some("idle") && self.phase == Phase::Thinking {
                    self.set_phase(Phase::Idle);
                }
            }
            "transcript" => {
                self.mark_bridge();
                ffi::set_transcript(&format!("You: {text}"));
            }
            "tts_start" => {
                self.mark_bridge();
                if self.phase == Phase::Listening {
                    ffi::play_stop();
                } else {
                    ffi::set_reply(text);
                    self.set_phase(Phase::Speaking);
                }
            }
            "error" => {
                self.mark_bridge();
                let err = v["error"].as_str().unwrap_or("unknown error");
                log::warn!("bridge error: {err}");
                if self.phase != Phase::Listening {
                    ffi::set_reply(err);
                    self.fail("Something went wrong");
                }
            }
            cmd => self.on_command(cmd),
        }
    }

    /// Bench test commands from a relay master.
    fn on_command(&mut self, cmd: &str) {
        match cmd {
            "ping" => self.send_text(r#"{"cmd":"pong"}"#),
            "beep" => {
                play_tone(400);
                self.send_text(r#"{"ok":true,"result":{"beeped":true}}"#);
            }
            "echo_on" => {
                if !self.echo.swap(true, Ordering::Relaxed) {
                    let run = self.echo.clone();
                    let _ = std::thread::Builder::new().stack_size(8192).spawn(move || echo_loop(run));
                }
                self.send_text(r#"{"ok":true,"result":{"echo":true}}"#);
            }
            "echo_off" => {
                self.echo.store(false, Ordering::Relaxed);
                self.send_text(r#"{"ok":true,"result":{"echo":false}}"#);
            }
            "status" => {
                let reply = format!(
                    r#"{{"ok":true,"result":{{"role":"voice-console","firmware":"{}","phase":"{:?}","online":{}}}}}"#,
                    env!("CARGO_PKG_VERSION"),
                    self.phase,
                    self.online
                );
                self.send_text(&reply);
            }
            _ => {}
        }
    }

    fn tick(&mut self) {
        if self.phase == Phase::Speaking && !ffi::play_active() {
            self.set_phase(Phase::Idle);
        }
        if self.phase == Phase::Thinking {
            let quiet_since = self.last_bridge.map_or(self.phase_since, |t| t.max(self.phase_since));
            if quiet_since.elapsed() >= REPLY_WAIT {
                self.fail("No reply");
            }
        }
        if self.last_ping.map_or(true, |t| t.elapsed() >= PING_EVERY) {
            self.ping();
        }
        if self.online && self.last_bridge.map_or(true, |t| t.elapsed() >= BRIDGE_STALE) {
            self.set_online(false);
        }
        if self.last_heap_log.elapsed() >= HEAP_LOG_EVERY {
            self.last_heap_log = Instant::now();
            log_heap();
        }
    }

    fn mark_bridge(&mut self) {
        self.last_bridge = Some(Instant::now());
        self.set_online(true);
    }

    fn set_online(&mut self, online: bool) {
        if self.online == online {
            return;
        }
        self.online = online;
        log::info!("bridge {}", if online { "online" } else { "offline" });
        if self.phase == Phase::Idle {
            self.show_phase();
        }
    }

    fn ping(&mut self) {
        self.last_ping = Some(Instant::now());
        self.send_text(r#"{"cmd":"ping"}"#);
    }

    fn flush_outbox(&mut self) {
        while let Ok(frame) = self.outbox.try_recv() {
            match frame {
                Outgoing::Text(text) => self.send_text(text),
                Outgoing::Audio(pcm) => self.send(FrameType::Binary(false), &pcm),
            }
        }
    }

    fn send_text(&mut self, text: &str) {
        self.send(FrameType::Text(false), text.as_bytes());
    }

    fn send(&mut self, kind: FrameType, data: &[u8]) {
        if !self.client.is_connected() {
            return;
        }
        if let Err(e) = self.client.send(kind, data) {
            log::warn!("relay send failed: {e}");
        }
    }
}
